//! Model pricing for local-log and hop cost estimation.
//!
//! The engine, the embedded snapshot and the published list-price overlays
//! live in `fabrials-pricing`. This host module supplies the other layers
//! (later wins):
//! 1. the runtime-refreshed LiteLLM + models.dev tables cached at
//!    `~/.cache/spanreed/pricing-remote.json` (prices) and
//!    `~/.cache/spanreed/limits-remote.json` (context windows), refreshed at
//!    most weekly; `SPANREED_OFFLINE` disables the refresh;
//! 2. the user's `~/.config/spanreed/pricing.json` override
//!    (`{ "<model>": { input_cost_per_token, ... } }`).
//!
//! Local logs use the strict resolver (`PricingMap::exact_cost`), which never
//! invents a cache rate the source omitted.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

pub use fabrials_pricing::pricing::{PricingMap, Usage};
use fabrials_pricing::{LimitsMap, UpstreamTables, build_limits, compose_upstream};

use crate::creds;
use crate::http::Request;

/// Upstream source of truth for model prices.
const REMOTE_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
/// Upstream source for the channels LiteLLM does not price.
const CHANNELS_URL: &str = "https://models.dev/api.json";
/// Refresh the cached remote table at most this often.
const REMOTE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// After a failed refresh, wait this long before trying again.
const REMOTE_RETRY: Duration = Duration::from_secs(6 * 60 * 60);

/// Price and context-window tables loaded from disk. Owned by `AppContext`;
/// the tables change only through [`Catalog::reload`] or [`Catalog::refresh`].
pub struct Catalog {
    table: RwLock<Arc<PricingMap>>,
    limits: RwLock<Arc<LimitsMap>>,
}

impl Catalog {
    /// The shared embedded snapshot and overlays, the cached remote refresh,
    /// then the user's overrides.
    pub fn load() -> Self {
        Self {
            table: RwLock::new(Arc::new(load_table())),
            limits: RwLock::new(Arc::new(load_limits())),
        }
    }

    /// A fixed table (tests, offline composition).
    pub fn with_table(table: PricingMap) -> Self {
        Self {
            table: RwLock::new(Arc::new(table)),
            limits: RwLock::new(Arc::new(build_limits("", None, None))),
        }
    }

    pub fn table(&self) -> Arc<PricingMap> {
        Arc::clone(&self.table.read().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn limits(&self) -> Arc<LimitsMap> {
        Arc::clone(&self.limits.read().unwrap_or_else(|e| e.into_inner()))
    }

    /// Re-read the cached and user tables from disk.
    pub fn reload(&self) {
        *self.table.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(load_table());
        *self.limits.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(load_limits());
    }

    /// Refresh the remote cache when stale ([`ensure_fresh`]) and reload the
    /// tables when it changed.
    pub fn refresh(&self) {
        if ensure_fresh() {
            self.reload();
        }
    }
}

fn load_table() -> PricingMap {
    let remote = creds::read_file(&remote_cache_path());
    let user = creds::read_file(&crate::product::config_dir().join("pricing.json"));
    table_from(remote.as_deref(), user.as_deref())
}

fn load_limits() -> LimitsMap {
    let cached = creds::read_file(&limits_cache_path());
    let user = creds::read_file(&crate::product::config_dir().join("limits.json"));
    build_limits("", cached.as_deref(), user.as_deref())
}

pub fn table_from(remote: Option<&str>, user: Option<&str>) -> PricingMap {
    fabrials_pricing::pricing::build_table(fabrials_pricing::pricing::embedded_json(), remote, user)
}

/// List-price USD for a captured hop.
pub fn hop_cost_usd(record: &fabrials_types::HopRecord, table: &PricingMap) -> Option<f64> {
    fabrials_pricing::cost::list_cost_usd_with(record, table)
}

fn remote_cache_path() -> PathBuf {
    crate::product::cache_dir().join("pricing-remote.json")
}

fn limits_cache_path() -> PathBuf {
    crate::product::cache_dir().join("limits-remote.json")
}

/// Fetch one upstream document.
fn fetch_json(url: &str) -> Result<String, String> {
    let resp = Request::get(url)
        .header("Accept", "application/json")
        .send()?;
    if !(200..300).contains(&resp.status) {
        return Err(format!("{url} fetch failed (HTTP {})", resp.status));
    }
    Ok(resp.body)
}

/// Download both upstreams. A source that fails aborts the refresh, so the
/// cached tables are never replaced by a partial pair.
pub fn fetch_filtered() -> Result<String, String> {
    Ok(fetch_upstream()?.prices)
}

fn fetch_upstream() -> Result<UpstreamTables, String> {
    compose_upstream(&fetch_json(REMOTE_URL)?, &fetch_json(CHANNELS_URL)?)
}

fn write_atomic(path: &std::path::Path, body: &str) -> bool {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body).is_ok() && std::fs::rename(&tmp, path).is_ok()
}

fn younger_than(path: &std::path::Path, ttl: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
        .is_some_and(|age| age < ttl)
}

/// Refresh the cached price and limits tables when either is missing or older
/// than the TTL. Failures are silent (logged at debug): the embedded snapshot
/// and any stale cache keep working offline, and a stamp file backs off retries
/// so an offline machine doesn't pay a connect timeout on every probe.
/// No-op when `SPANREED_OFFLINE` is set. Returns whether new tables were written.
pub fn ensure_fresh() -> bool {
    if crate::product::env_offline() {
        return false;
    }
    let prices = remote_cache_path();
    let limits = limits_cache_path();
    let stamp = prices.with_extension("attempt");
    let fresh = younger_than(&prices, REMOTE_TTL) && younger_than(&limits, REMOTE_TTL);
    if fresh || younger_than(&stamp, REMOTE_RETRY) {
        return false;
    }
    match fetch_upstream() {
        Ok(tables) => {
            let written =
                write_atomic(&prices, &tables.prices) && write_atomic(&limits, &tables.limits);
            if written {
                let _ = std::fs::remove_file(&stamp);
            }
            written
        }
        Err(e) => {
            log::debug!("pricing refresh skipped: {e}");
            let _ = std::fs::write(&stamp, b"");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_table_prices_claude_and_gpt_from_the_shared_snapshot() {
        let t = table_from(None, None);
        assert!(t.exact("claude-opus-4-8").is_some());
        assert!(t.exact("gpt-5-codex").is_some());
        assert!(t.exact("claude-fable-5").is_some());
    }

    #[test]
    fn remote_and_user_layers_override_in_order() {
        let remote = r#"{
            "model-b": { "input_cost_per_token": 2e-6, "output_cost_per_token": 2e-6 },
            "model-c": { "input_cost_per_token": 2e-6, "output_cost_per_token": 2e-6 }
        }"#;
        let user = r#"{
            "model-c": { "input_cost_per_token": 9e-6, "output_cost_per_token": 9e-6 }
        }"#;
        let t = table_from(Some(remote), Some(user));
        assert!((t.exact("model-b").unwrap().input - 2e-6).abs() < 1e-12);
        assert!(
            (t.exact("model-c").unwrap().input - 9e-6).abs() < 1e-12,
            "user overrides remote"
        );
    }

    #[test]
    fn local_logs_stay_unpriced_for_dated_or_unknown_models() {
        let t = table_from(None, None);
        assert!(t.exact("claude-opus-4-8-20260601").is_none());
        assert!(
            t.exact_cost(
                "totally-made-up-model-xyz",
                Usage {
                    input: 10,
                    ..Default::default()
                }
            )
            .is_none()
        );
    }

    #[test]
    fn composed_refresh_feeds_prices_and_limits() {
        let litellm = serde_json::json!({
            "claude-fable-5": {"input_cost_per_token": 6e-6, "output_cost_per_token": 3e-5}
        })
        .to_string();
        let models_dev = serde_json::json!({
            "opencode-go": {"models": {"deepseek-v4.1-flash": {
                "limit": {"context": 1000000},
                "cost": {"input": 0.15, "output": 0.6}
            }}}
        })
        .to_string();
        let tables = compose_upstream(&litellm, &models_dev).expect("compose");
        let t = table_from(Some(&tables.prices), None);
        assert!(
            t.exact("deepseek-flash").is_some(),
            "relay picker alias priced"
        );
        let windows = build_limits("", Some(&tables.limits), None);
        assert_eq!(
            windows.get("deepseek-flash").map(|l| l.context_window),
            Some(1_000_000)
        );
    }
}
