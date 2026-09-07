//! Local routing preferences and metadata. Never projects provider secrets.
use serde_json::{json, Value};
use std::sync::Mutex;

pub const PROVIDERS: &[&str] = &["grok", "nous", "openai"];

fn config() -> Result<Value, String> {
    use std::io::Read;
    match std::fs::File::open(crate::app::config_dir().join("config.json")) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(1_048_577)
                .read_to_end(&mut bytes)
                .map_err(|_| "Settings unavailable")?;
            if bytes.len() > 1_048_576 {
                return Err("Settings too large".into());
            }
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid settings")?;
            if !value.is_object() {
                return Err("Invalid settings".into());
            }
            Ok(value)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(_) => Err("Settings unavailable".into()),
    }
}

pub fn settings(provider: &str) -> Result<(bool, f64), String> {
    if !PROVIDERS.contains(&provider) {
        return Err("Unsupported provider".into());
    }
    let config = config()?;
    let entry = &config["accounts"][provider];
    let on = match entry.get("autosteer") {
        Some(value) => value.as_bool().ok_or("Invalid autosteer preference")?,
        None => provider == "grok",
    };
    let threshold = match entry.get("exhausted_pct") {
        Some(value) => value
            .as_f64()
            .filter(|n| n.is_finite() && *n > 0.0 && *n <= 100.0)
            .ok_or("Invalid exhaustion threshold")?,
        None => 100.0,
    };
    Ok((on, threshold))
}

pub fn set_policy(provider: &str, on: bool, threshold: Option<f64>) -> Result<(), String> {
    if !PROVIDERS.contains(&provider) {
        return Err("Unsupported provider".into());
    }
    if threshold.is_some_and(|value| !value.is_finite() || value <= 0.0 || value > 100.0) {
        return Err("Exhaustion threshold must be greater than 0 and at most 100".into());
    }
    static WRITE: Mutex<()> = Mutex::new(());
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut config = config()?;
    if config.get("accounts").is_some_and(|v| !v.is_object()) {
        return Err("Invalid account settings".into());
    }
    if config["accounts"]
        .get(provider)
        .is_some_and(|v| !v.is_object())
    {
        return Err("Invalid provider settings".into());
    }
    config["accounts"][provider]["autosteer"] = json!(on);
    if let Some(threshold) = threshold {
        config["accounts"][provider]["exhausted_pct"] = json!(threshold);
    }
    let bytes = serde_json::to_vec_pretty(&config).map_err(|_| "Invalid settings")?;
    fabrials_runtime::files::atomic_write_private(
        &crate::app::config_dir().join("config.json"),
        &bytes,
    )
    .map_err(|_| "Could not save settings".into())
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct RoutingPolicy {
    pub provider: String,
    pub autosteer: bool,
    pub exhausted_pct: f64,
    pub mode: String,
}
pub fn policy_view(provider: &str) -> Result<RoutingPolicy, String> {
    let (on, threshold) = settings(provider)?;
    Ok(RoutingPolicy {
        provider: provider.into(),
        autosteer: on,
        exhausted_pct: threshold,
        mode: if on { "autosteer" } else { "active" }.into(),
    })
}
pub fn status(provider: &str) -> Result<Value, String> {
    serde_json::to_value(policy_view(provider)?).map_err(|_| "Invalid routing policy".into())
}

pub fn select_account(
    registry: &crate::accounts::Registry,
    provider: &str,
    alias: Option<&str>,
) -> Result<Option<crate::accounts::Account>, String> {
    let accounts: Vec<_> = registry
        .accounts
        .iter()
        .filter(|a| a.provider == provider)
        .cloned()
        .collect();
    if let Some(alias) = alias {
        return accounts
            .into_iter()
            .find(|a| a.alias == alias || a.aliases.iter().any(|name| name == alias))
            .map(Some)
            .ok_or("Unknown account".into());
    }
    if accounts.is_empty() {
        return Ok(None);
    }
    let (on, threshold) = settings(provider)?;
    if on {
        return crate::drivers::grok::pick_autosteer(&accounts, threshold, crate::util::now_ms())
            .cloned()
            .map(Some)
            .ok_or("All provider accounts are exhausted".into());
    }
    accounts
        .into_iter()
        .find(|a| a.active)
        .map(Some)
        .ok_or("No active provider account".into())
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct PoolAccount {
    pub alias: String,
    pub plan: Option<String>,
    pub used_pct: Option<f64>,
    pub resets_at: Option<String>,
    pub exhausted: bool,
    pub role: String,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct ProviderPool {
    pub provider: String,
    pub route: String,
    pub autosteer: bool,
    pub accounts: Vec<PoolAccount>,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct RoutingLimits {
    pub mode: String,
    pub exhausted_pct: f64,
    pub providers: Vec<ProviderPool>,
}
pub fn limits_view() -> Result<RoutingLimits, String> {
    let registry = crate::accounts::routing_registry()?;
    let mut providers = Vec::new();
    for provider in PROVIDERS {
        let (on, threshold) = settings(provider)?;
        let selected = select_account(&registry, provider, None).ok().flatten();
        let accounts = registry
            .accounts
            .iter()
            .filter(|a| a.provider == *provider)
            .map(|a| PoolAccount {
                alias: a.alias.clone(),
                plan: a.plan_label.clone().or_else(|| a.plan_slug.clone()),
                used_pct: a.used_pct,
                resets_at: a.resets_at.clone(),
                exhausted: a.used_pct.is_some_and(|pct| pct >= threshold),
                role: if selected.as_ref().is_some_and(|picked| picked.id == a.id) {
                    if on {
                        "next"
                    } else {
                        "active"
                    }
                } else {
                    "—"
                }
                .into(),
            })
            .collect();
        providers.push(ProviderPool {
            provider: (*provider).into(),
            route: match *provider {
                "grok" => "/v1",
                "nous" => "/nous/v1",
                _ => "/openai/v1",
            }
            .into(),
            autosteer: on,
            accounts,
        });
    }
    Ok(RoutingLimits {
        mode: "autosteer".into(),
        exhausted_pct: 100.0,
        providers,
    })
}
pub fn limits() -> Result<Value, String> {
    serde_json::to_value(limits_view()?).map_err(|_| "Invalid routing limits".into())
}

pub(crate) fn environment_id() -> Result<String, String> {
    let _guard = fabrials_runtime::file_set::FileSet::acquire_wait(&crate::app::data_dir())?;
    let path = crate::app::data_dir().join("environment-id");
    let id = match std::fs::read_to_string(&path) {
        Ok(id) if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) => id,
        Ok(_) => return Err("Invalid local environment identity".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let id = fabrials_runtime::accounting::new_request_id();
            fabrials_runtime::files::atomic_write_private(&path, id.as_bytes())
                .map_err(|_| "Environment identity unavailable")?;
            id
        }
        Err(_) => return Err("Environment identity unavailable".into()),
    };
    Ok(id)
}

pub fn environment(bind: &str) -> Result<Value, String> {
    let id = environment_id()?;
    Ok(
        json!({"environment_id":id,"label":"Spanreed local","version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,
        "capabilities":["proxy.openai-compat.v1","proxy.media.v1","proxy.realtime.v1","routing.autosteer.v1","usage.list-price.v1"],
        "endpoints":[{"key":"local","label":"This machine","http_url":format!("http://{bind}"),"websocket_url":format!("ws://{bind}"),"reachability":"local","available":true,"is_default":true}]}),
    )
}
