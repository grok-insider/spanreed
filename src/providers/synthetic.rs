//! Synthetic (synthetic.new) provider.
//!
//! API key discovered from (in order): Pi auth.json, Pi models.json,
//! Factory/Droid settings.json, OpenCode auth.json, or `SYNTHETIC_API_KEY`.
//! Usage: `GET https://api.synthetic.new/v2/quotas`.

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProgressFormat, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "synthetic";
const NAME: &str = "Synthetic";
const QUOTAS_URL: &str = "https://api.synthetic.new/v2/quotas";
const KEY_NAMES: &[&str] = &["synthetic", "synthetic.new", "syn"];

pub struct Synthetic;

fn pi_dir() -> std::path::PathBuf {
    if let Some(dir) = creds::env("PI_CODING_AGENT_DIR") {
        return creds::expand(&dir);
    }
    creds::expand("~/.pi/agent")
}

/// Look up a key under any of the synthetic provider aliases in a JSON object
/// that maps providerName -> { key | apiKey }.
fn key_from_provider_map(value: &serde_json::Value, field: &str) -> Option<String> {
    for name in KEY_NAMES {
        if let Some(k) = value
            .get(name)
            .and_then(|e| e.get(field))
            .and_then(|v| v.as_str())
        {
            if !k.is_empty() {
                return Some(k.to_string());
            }
        }
    }
    None
}

fn discover_key() -> Option<String> {
    // 1) Pi auth.json: { synthetic: { type, key } }
    if let Some(v) = creds::read_json(&pi_dir().join("auth.json")) {
        if let Some(k) = key_from_provider_map(&v, "key") {
            return Some(k);
        }
    }
    // 2) Pi models.json: { providers: { synthetic: { apiKey } } }
    if let Some(v) = creds::read_json(&pi_dir().join("models.json")) {
        if let Some(providers) = v.get("providers") {
            if let Some(k) = key_from_provider_map(providers, "apiKey") {
                return Some(k);
            }
        }
    }
    // 3) Factory/Droid settings.json: customModels[].baseUrl contains synthetic.new
    if let Some(v) = creds::read_json(&creds::expand("~/.factory/settings.json")) {
        if let Some(models) = v.get("customModels").and_then(|m| m.as_array()) {
            for m in models {
                let base = m.get("baseUrl").and_then(|b| b.as_str()).unwrap_or("");
                if base.contains("synthetic.new") {
                    if let Some(k) = m.get("apiKey").and_then(|v| v.as_str()) {
                        if !k.is_empty() {
                            return Some(k.to_string());
                        }
                    }
                }
            }
        }
    }
    // 4) OpenCode auth.json: { synthetic: { key } }
    if let Some(v) = creds::read_json(&creds::data_home().join("opencode").join("auth.json")) {
        if let Some(k) = key_from_provider_map(&v, "key") {
            return Some(k);
        }
    }
    // 5) env
    creds::env("SYNTHETIC_API_KEY")
}

impl Provider for Synthetic {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        discover_key().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let key = match discover_key() {
            Some(k) => k,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "Synthetic API key not found. Set SYNTHETIC_API_KEY or add key to ~/.pi/agent/auth.json",
                )
            }
        };

        let resp = match Request::get(QUOTAS_URL)
            .bearer(&key)
            .header("Accept", "application/json")
            .send()
        {
            Ok(r) => r,
            Err(_) => return ProviderOutput::error(ID, NAME, "Request failed. Check your connection."),
        };
        if resp.is_auth_error() {
            return ProviderOutput::error(ID, NAME, "API key invalid or expired. Check your Synthetic API key.");
        }
        if !(200..300).contains(&resp.status) {
            return ProviderOutput::error(ID, NAME, format!("Request failed (HTTP {})", resp.status));
        }
        let data = match resp.json() {
            Some(d) => d,
            None => return ProviderOutput::error(ID, NAME, "Could not parse usage data."),
        };

        let mut lines = Vec::new();

        // 5h rolling limit (primary).
        if let Some(roll) = data.get("rollingFiveHourLimit") {
            let max = roll.get("max").and_then(|v| v.as_f64());
            let remaining = roll.get("remaining").and_then(|v| v.as_f64());
            if let (Some(max), Some(remaining)) = (max, remaining) {
                if max > 0.0 {
                    lines.push(MetricLine::Progress {
                        label: "5h Rate Limit".into(),
                        used: (max - remaining).max(0.0),
                        limit: max,
                        format: ProgressFormat::Count {
                            suffix: "reqs".into(),
                        },
                        resets_at: None,
                        color: None,
                    });
                }
            }
            if roll.get("limited").and_then(|v| v.as_bool()).unwrap_or(false) {
                lines.push(MetricLine::Badge {
                    label: "Rate Limited".into(),
                    text: "Active".into(),
                    color: Some("#ef4444".into()),
                    subtitle: None,
                });
            }
        }

        // Weekly mana bar.
        if let Some(pct_remaining) = data
            .get("weeklyTokenLimit")
            .and_then(|w| w.get("percentRemaining"))
            .and_then(|v| v.as_f64())
        {
            lines.push(MetricLine::percent("Mana Bar", 100.0 - pct_remaining, None));
        }

        // Search hourly quota.
        if let Some(hourly) = data.get("search").and_then(|s| s.get("hourly")) {
            let limit = hourly.get("limit").and_then(|v| v.as_f64());
            let requests = hourly.get("requests").and_then(|v| v.as_f64());
            if let (Some(limit), Some(requests)) = (limit, requests) {
                if limit > 0.0 {
                    let resets = hourly.get("renewsAt").and_then(util::to_iso);
                    lines.push(MetricLine::Progress {
                        label: "Search".into(),
                        used: requests,
                        limit,
                        format: ProgressFormat::Count {
                            suffix: "reqs".into(),
                        },
                        resets_at: resets,
                        color: None,
                    });
                }
            }
        }

        if lines.is_empty() {
            return ProviderOutput::error(ID, NAME, "Could not parse usage data.");
        }
        ProviderOutput::new(ID, NAME, lines)
    }
}
