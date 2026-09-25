//! Model pricing for local-log cost estimation.
//!
//! The table is built from three layers (later layers win):
//! 1. An embedded, filtered snapshot of LiteLLM's
//!    `model_prices_and_context_window.json` (compile-time, offline fallback —
//!    Nix-sandbox friendly).
//! 2. A runtime-refreshed copy of the same upstream data plus the channels
//!    that upstream does not price (models.dev: OpenCode Go), filtered to the
//!    relevant model families and cached at
//!    `~/.cache/spanreed/pricing-remote.json` with a 7-day TTL, so newly
//!    released models get priced without a new binary. The same refresh writes
//!    context windows to `~/.cache/spanreed/limits-remote.json`. Set
//!    `SPANREED_OFFLINE` to disable the refresh entirely.
//! 3. The user's `~/.config/spanreed/pricing.json` override
//!    (same shape: `{ "<model>": { input_cost_per_token, ... } }`).
//!
//! Prices are USD per token. Cache-write/read and a >200k-context tier are
//! supported, mirroring how the upstream pricing data is structured. The two
//! upstream filters live in `fabrials-metrics`: they are the same table shape
//! the alojado relay reads, so both products price a model identically.
//! This module keeps its own strict resolver (see [`PricingMap::exact_cost`])
//! because local logs must stay unpriced when a cache rate is unknown.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use fabrials_metrics::{
    build_limits, filter_models_dev, filter_upstream, merge_price_tables, models_dev_prices,
    normalize, LimitsMap,
};

use crate::creds;
use crate::http::Request;

const EMBEDDED: &str = include_str!("pricing-data.json");

/// Upstream source of truth for model prices.
const REMOTE_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
/// Upstream source for the channels LiteLLM does not price.
const CHANNELS_URL: &str = "https://models.dev/api.json";
/// Channels spanreed routes through but LiteLLM does not carry.
const CHANNELS: &[&str] = &["opencode-go"];
/// Local ids that alias a channel model. The relay picker uses the first.
const ALIASES: &[(&str, &str)] = &[("deepseek-flash", "deepseek-v4.1-flash")];
/// Refresh the cached remote table at most this often.
const REMOTE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// After a failed refresh, wait this long before trying again.
const REMOTE_RETRY: Duration = Duration::from_secs(6 * 60 * 60);

/// Raw LiteLLM-shaped entry (only the fields we use).
#[derive(Debug, Clone, Deserialize, Serialize)]
struct RawPricing {
    #[serde(skip_serializing_if = "Option::is_none")]
    input_cost_per_token: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_cost_per_token: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_creation_input_token_cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_read_input_token_cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_cost_per_token_above_200k_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_cost_per_token_above_200k_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_creation_input_token_cost_above_200k_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_read_input_token_cost_above_200k_tokens: Option<f64>,
}

/// Resolved per-token prices for a model.
#[derive(Debug, Clone, Copy)]
pub struct Pricing {
    pub input: f64,
    pub output: f64,
    pub cache_create: f64,
    pub cache_read: f64,
    pub cache_create_known: bool,
    pub cache_read_known: bool,
    pub input_above_200k: Option<f64>,
    pub output_above_200k: Option<f64>,
    pub cache_create_above_200k: Option<f64>,
    pub cache_read_above_200k: Option<f64>,
}

impl Pricing {
    fn from_raw(r: &RawPricing) -> Option<Self> {
        let input = r.input_cost_per_token?;
        let output = r.output_cost_per_token?;
        Some(Pricing {
            input,
            output,
            cache_create: r.cache_creation_input_token_cost.unwrap_or(0.0),
            cache_read: r.cache_read_input_token_cost.unwrap_or(0.0),
            cache_create_known: r.cache_creation_input_token_cost.is_some(),
            cache_read_known: r.cache_read_input_token_cost.is_some(),
            input_above_200k: r.input_cost_per_token_above_200k_tokens,
            output_above_200k: r.output_cost_per_token_above_200k_tokens,
            cache_create_above_200k: r.cache_creation_input_token_cost_above_200k_tokens,
            cache_read_above_200k: r.cache_read_input_token_cost_above_200k_tokens,
        })
    }
}

/// Token usage for a single message (raw counts).
#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_create: u64,
    pub cache_read: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_create + self.cache_read
    }
}

/// The resolved pricing table, built once from embedded + user override.
pub struct PricingMap {
    table: HashMap<String, Pricing>,
}

impl PricingMap {
    /// Resolve an exact normalized model. Unknown variants stay unpriced.
    pub fn exact(&self, model: &str) -> Option<&Pricing> {
        self.table
            .get(model)
            .or_else(|| self.table.get(&normalize(model)))
    }

    pub fn exact_cost(&self, model: &str, usage: Usage) -> Option<(f64, &Pricing)> {
        let p = self.exact(model)?;
        if usage.cache_create > 0 && !p.cache_create_known
            || usage.cache_read > 0 && !p.cache_read_known
        {
            return None;
        }
        let cost = tiered(usage.input, p.input, p.input_above_200k)
            + tiered(usage.output, p.output, p.output_above_200k)
            + tiered(
                usage.cache_create,
                p.cache_create,
                p.cache_create_above_200k,
            )
            + tiered(usage.cache_read, p.cache_read, p.cache_read_above_200k);
        (cost.is_finite() && cost >= 0.0).then_some((cost, p))
    }
}

/// Tiered pricing: tokens beyond 200k use the higher rate when present.
fn tiered(tokens: u64, base: f64, above_200k: Option<f64>) -> f64 {
    const TIER: u64 = 200_000;
    match above_200k {
        Some(high) if tokens > TIER => (TIER as f64) * base + ((tokens - TIER) as f64) * high,
        _ => (tokens as f64) * base,
    }
}

fn parse_table(json: &str) -> HashMap<String, Pricing> {
    let raw: HashMap<String, RawPricing> = serde_json::from_str(json).unwrap_or_default();
    raw.iter()
        .filter_map(|(k, v)| Pricing::from_raw(v).map(|p| (k.clone(), p)))
        .collect()
}

/// Build the table from its layers; later layers override earlier ones.
fn build_table(embedded: &str, remote: Option<&str>, user: Option<&str>) -> PricingMap {
    let mut table = parse_table(embedded);
    for layer in [remote, user].into_iter().flatten() {
        for (k, p) in parse_table(layer) {
            table.insert(k, p);
        }
    }
    PricingMap { table }
}

/// The process-wide pricing table: embedded snapshot, overlaid with the cached
/// remote refresh, overlaid with the user's `~/.config/spanreed/pricing.json`.
pub fn table() -> &'static PricingMap {
    static TABLE: OnceLock<PricingMap> = OnceLock::new();
    TABLE.get_or_init(|| {
        let remote = creds::read_file(&remote_cache_path());
        let override_path = crate::app::config_dir().join("pricing.json");
        let user = creds::read_file(&override_path);
        build_table(EMBEDDED, remote.as_deref(), user.as_deref())
    })
}

/// Hop pricing (Grok and relay hops) with the same layers as [`table`]: the
/// shared embedded snapshot, the cached remote refresh and the user override.
pub fn hop_table() -> &'static fabrials_metrics::pricing::PricingMap {
    static TABLE: OnceLock<fabrials_metrics::pricing::PricingMap> = OnceLock::new();
    TABLE.get_or_init(|| {
        let remote = creds::read_file(&remote_cache_path());
        let user = creds::read_file(&crate::app::config_dir().join("pricing.json"));
        hop_table_from(remote.as_deref(), user.as_deref())
    })
}

pub fn hop_table_from(
    remote: Option<&str>,
    user: Option<&str>,
) -> fabrials_metrics::pricing::PricingMap {
    fabrials_metrics::pricing::build_table(
        fabrials_metrics::pricing::embedded_json(),
        remote,
        user,
    )
}

/// List-price USD for a captured hop, priced with [`hop_table`].
pub fn hop_cost_usd(record: &fabrials_model::UsageRecord) -> Option<f64> {
    fabrials_metrics::cost::list_cost_usd_with(record, hop_table())
}

fn remote_cache_path() -> PathBuf {
    crate::app::cache_dir().join("pricing-remote.json")
}

fn limits_cache_path() -> PathBuf {
    crate::app::cache_dir().join("limits-remote.json")
}

/// Prices and context windows reduced from one pair of upstream documents.
pub struct UpstreamTables {
    pub prices: String,
    pub limits: String,
}

/// Reduce a LiteLLM price document and a models.dev catalog to the two tables
/// the cache stores. LiteLLM keeps every id it prices; the channel supplies
/// the models that document does not carry, plus their context windows.
pub fn compose_upstream(
    litellm_json: &str,
    models_dev_json: &str,
) -> Result<UpstreamTables, String> {
    let litellm = filter_upstream(litellm_json)?;
    let channels = models_dev_prices(models_dev_json, CHANNELS, ALIASES)?;
    Ok(UpstreamTables {
        prices: merge_price_tables(&[&channels, &litellm])?,
        limits: filter_models_dev(models_dev_json, CHANNELS, ALIASES)?,
    })
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

/// Context windows from the cached models.dev refresh, then the user's
/// `~/.config/spanreed/limits.json`. Empty until the first successful refresh.
pub fn limits() -> &'static LimitsMap {
    static LIMITS: OnceLock<LimitsMap> = OnceLock::new();
    LIMITS.get_or_init(|| {
        let cached = creds::read_file(&limits_cache_path());
        let user = creds::read_file(&crate::app::config_dir().join("limits.json"));
        build_limits("", cached.as_deref(), user.as_deref())
    })
}

/// Refresh the cached price and limits tables when either is missing or older
/// than the TTL. Failures are silent (logged at debug): the embedded snapshot
/// and any stale cache keep working offline, and a stamp file backs off retries
/// so an offline machine doesn't pay a connect timeout on every probe.
/// No-op when `SPANREED_OFFLINE` is set.
pub fn ensure_fresh() {
    if crate::app::env_offline() {
        return;
    }
    let prices = remote_cache_path();
    let limits = limits_cache_path();
    let stamp = prices.with_extension("attempt");
    let fresh = younger_than(&prices, REMOTE_TTL) && younger_than(&limits, REMOTE_TTL);
    if fresh || younger_than(&stamp, REMOTE_RETRY) {
        return;
    }
    match fetch_upstream() {
        Ok(tables) => {
            if write_atomic(&prices, &tables.prices) && write_atomic(&limits, &tables.limits) {
                let _ = std::fs::remove_file(&stamp);
            }
        }
        Err(e) => {
            log::debug!("pricing refresh skipped: {e}");
            let _ = std::fs::write(&stamp, b"");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_table_parses_and_has_claude_and_gpt() {
        let t = table();
        assert!(
            t.exact("claude-opus-4-8").is_some(),
            "claude-opus-4-8 priced"
        );
        assert!(t.exact("gpt-5-codex").is_some(), "gpt-5-codex priced");
        assert!(t.exact("claude-fable-5").is_some(), "claude-fable-5 priced");
    }

    #[test]
    fn merged_table_prices_the_channel_litellm_does_not_carry() {
        let litellm = serde_json::json!({
            "claude-fable-5": {
                "input_cost_per_token": 6e-6,
                "output_cost_per_token": 3e-5
            },
            "xai/grok-4.6": {
                "input_cost_per_token": 3e-6,
                "output_cost_per_token": 9e-6
            }
        })
        .to_string();
        let models_dev = serde_json::json!({
            "opencode-go": {"models": {
                "deepseek-v4.1-flash": {
                    "limit": {"context": 1000000, "output": 384000},
                    "cost": {"input": 0.15, "output": 0.6, "cache_read": 0.003}
                },
                "grok-4.6": {
                    "limit": {"context": 500000},
                    "cost": {"input": 2, "output": 6, "cache_read": 0.5}
                },
                "ox-alpha-free": {"limit": {"context": 1000000}, "cost": {}}
            }},
            "openrouter": {"models": {"claude-fable-5": {"cost": {"input": 9, "output": 9}}}}
        })
        .to_string();
        let tables = compose_upstream(&litellm, &models_dev).expect("compose");
        let t = build_table(EMBEDDED, Some(&tables.prices), None);

        let flash = t.exact("deepseek-v4.1-flash").expect("channel row priced");
        assert!(
            (flash.input - 1.5e-7).abs() < 1e-15,
            "per million to per token"
        );
        assert!(flash.cache_read_known);
        assert!(
            !flash.cache_create_known,
            "a rate the source omits stays unknown"
        );
        assert_eq!(
            t.exact("deepseek-flash").map(|p| p.input),
            Some(flash.input),
            "the relay picker id copies the canonical rate"
        );
        assert!(
            t.exact("ox-alpha-free").is_none(),
            "a row without rates is dropped"
        );
        assert_eq!(t.exact("xai/grok-4.6").expect("litellm row").input, 3e-6);
        assert_eq!(
            t.exact("grok-4.6").expect("channel spelling").input,
            2e-6,
            "the id a routed session reports keeps its own rate"
        );
        let windows = build_limits("", Some(&tables.limits), None);
        assert_eq!(
            windows.get("deepseek-flash").map(|l| l.context_window),
            Some(1_000_000)
        );
        assert_eq!(
            windows
                .get("deepseek-v4.1-flash")
                .and_then(|l| l.max_output),
            Some(384_000)
        );
        assert_eq!(
            windows.get("grok-4.6").map(|l| l.context_window),
            Some(500_000)
        );
        assert!(windows.get("ox-alpha-free").is_some());
    }

    #[test]
    fn filter_upstream_rejects_tables_without_claude() {
        let upstream = serde_json::json!({
            "mistral-large": { "input_cost_per_token": 2e-6, "output_cost_per_token": 6e-6 }
        })
        .to_string();
        assert!(filter_upstream(&upstream).is_err());
        assert!(filter_upstream("not json").is_err());
    }

    #[test]
    fn build_table_layers_remote_and_user_over_embedded() {
        let embedded = r#"{
            "model-a": { "input_cost_per_token": 1e-6, "output_cost_per_token": 1e-6 },
            "model-b": { "input_cost_per_token": 1e-6, "output_cost_per_token": 1e-6 }
        }"#;
        let remote = r#"{
            "model-b": { "input_cost_per_token": 2e-6, "output_cost_per_token": 2e-6 },
            "model-c": { "input_cost_per_token": 2e-6, "output_cost_per_token": 2e-6 }
        }"#;
        let user = r#"{
            "model-c": { "input_cost_per_token": 9e-6, "output_cost_per_token": 9e-6 }
        }"#;
        let t = build_table(embedded, Some(remote), Some(user));
        assert!((t.exact("model-a").unwrap().input - 1e-6).abs() < 1e-12);
        assert!(
            (t.exact("model-b").unwrap().input - 2e-6).abs() < 1e-12,
            "remote overrides embedded"
        );
        assert!(
            (t.exact("model-c").unwrap().input - 9e-6).abs() < 1e-12,
            "user overrides remote"
        );
    }

    #[test]
    fn prefix_match_handles_dated_suffix() {
        let t = table();
        // A dated variant should fall back to the base model's pricing.
        assert!(t.exact("claude-opus-4-8-20260601").is_none());
    }

    #[test]
    fn unknown_model_has_no_price() {
        let t = table();
        assert!(t.exact("totally-made-up-model-xyz").is_none());
        assert!(t
            .exact_cost(
                "totally-made-up-model-xyz",
                Usage {
                    input: 10,
                    ..Default::default()
                }
            )
            .is_none());
    }

    #[test]
    fn cost_math_is_linear_in_tokens() {
        let map = PricingMap {
            table: HashMap::from([(
                "m".to_string(),
                Pricing {
                    input: 1e-6,
                    output: 2e-6,
                    cache_create: 5e-7,
                    cache_read: 1e-7,
                    cache_create_known: true,
                    cache_read_known: true,
                    input_above_200k: None,
                    output_above_200k: None,
                    cache_create_above_200k: None,
                    cache_read_above_200k: None,
                },
            )]),
        };
        let usage = Usage {
            input: 1_000_000,
            output: 1_000_000,
            cache_create: 1_000_000,
            cache_read: 1_000_000,
        };
        // 1e6*(1e-6 + 2e-6 + 5e-7 + 1e-7) = 1.0 + 2.0 + 0.5 + 0.1 = 3.6
        let (cost, _) = map.exact_cost("m", usage).unwrap();
        assert!((cost - 3.6).abs() < 1e-9, "got {cost}");
    }

    #[test]
    fn tiered_pricing_applies_above_200k() {
        // 300k input tokens: 200k @ 1e-6 + 100k @ 2e-6 = 0.2 + 0.2 = 0.4
        let v = tiered(300_000, 1e-6, Some(2e-6));
        assert!((v - 0.4).abs() < 1e-9, "got {v}");
        // Without a tier, linear: 300k @ 1e-6 = 0.3
        let v2 = tiered(300_000, 1e-6, None);
        assert!((v2 - 0.3).abs() < 1e-9, "got {v2}");
    }

    #[test]
    fn normalize_strips_provider_prefix() {
        assert_eq!(normalize("openai/gpt-5-codex"), "gpt-5-codex");
        assert_eq!(normalize("claude-opus-4-8"), "claude-opus-4-8");
        assert_eq!(normalize("gpt-5@2025-08-07"), "gpt-5-2025-08-07");
    }
}
