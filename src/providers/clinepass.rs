//! ClinePass subscription windows.
//!
//! `GET https://api.cline.bot/api/v1/users/me/plan/usage-limits`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;
use crate::util;

const ID: &str = "clinepass";
const NAME: &str = "ClinePass";

pub struct ClinePass;

fn label(kind: &str) -> Option<&'static str> {
    match kind {
        "five_hour" => Some("5-hour"),
        "weekly" => Some("Weekly"),
        "monthly" => Some("Monthly"),
        _ => None,
    }
}

pub(super) fn parse_limits(body: &serde_json::Value) -> Vec<MetricLine> {
    let limits = body
        .get("data")
        .and_then(|v| v.as_array())
        .or_else(|| body.as_array());
    let Some(limits) = limits else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for limit in limits {
        let Some(kind) = limit.get("type").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(label) = label(kind) else {
            continue;
        };
        let Some(pct) = field(limit, "percentUsed") else {
            continue;
        };
        let resets = limit.get("resetsAt").and_then(util::to_iso);
        lines.push(MetricLine::percent(label, pct, resets));
    }
    lines
}

impl Provider for ClinePass {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["CLINE_API_KEY", "CLINEPASS_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["CLINE_API_KEY", "CLINEPASS_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No ClinePass API key found. Set CLINE_API_KEY.",
            );
        };
        match json_api::get_bearer(
            "https://api.cline.bot/api/v1/users/me/plan/usage-limits",
            &key,
        ) {
            Ok(data) => {
                let lines = parse_limits(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "ClinePass usage response had no windows.")
                } else {
                    ProviderOutput::new(ID, NAME, lines)
                }
            }
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_named_windows() {
        let lines = parse_limits(&serde_json::json!({
            "success": true,
            "data": [
                {"type": "five_hour", "percentUsed": 12.5, "resetsAt": "2026-10-01T00:00:00Z"},
                {"type": "weekly", "percentUsed": 40},
                {"type": "ignored", "percentUsed": 1}
            ]
        }));
        assert_eq!(lines.len(), 2);
    }
}
