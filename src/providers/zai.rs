//! Z.ai (Zhipu / GLM coding plans) provider.
//!
//! Auth: API key from `ZAI_API_KEY` (fallback `GLM_API_KEY`).
//! Usage: `GET https://api.z.ai/api/monitor/usage/quota/limit`
//! Plan:  `GET https://api.z.ai/api/biz/subscription/list`

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProgressFormat, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "zai";
const NAME: &str = "Z.ai";
const QUOTA_URL: &str = "https://api.z.ai/api/monitor/usage/quota/limit";
const SUB_URL: &str = "https://api.z.ai/api/biz/subscription/list";

pub struct Zai;

fn api_key() -> Option<String> {
    creds::env("ZAI_API_KEY").or_else(|| creds::env("GLM_API_KEY"))
}

fn get(url: &str, key: &str) -> Result<serde_json::Value, String> {
    let resp = Request::get(url)
        .bearer(key)
        .header("Accept", "application/json")
        .send()?;
    if resp.is_auth_error() {
        return Err("API key invalid. Check your Z.ai API key.".into());
    }
    if !(200..300).contains(&resp.status) {
        return Err(format!("Usage request failed (HTTP {}). Try again later.", resp.status));
    }
    resp.json().ok_or_else(|| "Usage response invalid. Try again later.".into())
}

impl Provider for Zai {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        api_key().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let key = match api_key() {
            Some(k) => k,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "No ZAI_API_KEY found. Set up environment variable first.",
                )
            }
        };

        let data = match get(QUOTA_URL, &key) {
            Ok(d) => d,
            Err(e) => return ProviderOutput::error(ID, NAME, e),
        };

        let limits = data
            .get("data")
            .and_then(|d| d.get("limits"))
            .and_then(|l| l.as_array());
        let limits = match limits {
            Some(l) => l,
            None => return ProviderOutput::error(ID, NAME, "Usage response invalid. Try again later."),
        };

        let mut lines = Vec::new();
        for limit in limits {
            let kind = limit.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match kind {
                "TOKENS_LIMIT" => {
                    let pct = limit.get("percentage").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let number = limit.get("number").and_then(|v| v.as_i64()).unwrap_or(0);
                    let unit = limit.get("unit").and_then(|v| v.as_i64()).unwrap_or(0);
                    // unit 3/number 5 = 5-hour session; unit 6/number 7 = 7-day weekly.
                    let label = if unit == 6 || number == 7 { "Weekly" } else { "Session" };
                    let resets = limit.get("nextResetTime").and_then(util::to_iso);
                    lines.push(MetricLine::percent(label, pct, resets));
                }
                "TIME_LIMIT" => {
                    let usage = limit.get("usage").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let current = limit.get("currentValue").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    if usage > 0.0 {
                        lines.push(MetricLine::Progress {
                            label: "Web Searches".into(),
                            used: current,
                            limit: usage,
                            format: ProgressFormat::Count {
                                suffix: "searches".into(),
                            },
                            resets_at: None,
                            color: None,
                        });
                    }
                }
                _ => {}
            }
        }

        if lines.is_empty() {
            return ProviderOutput::error(ID, NAME, "Usage response invalid. Try again later.");
        }

        // Plan name (best-effort).
        let plan = get(SUB_URL, &key).ok().and_then(|sub| {
            sub.get("data")
                .and_then(|d| d.as_array())
                .and_then(|arr| arr.first())
                .and_then(|s| s.get("productName"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        });

        ProviderOutput::new(ID, NAME, lines).with_plan(plan)
    }
}
