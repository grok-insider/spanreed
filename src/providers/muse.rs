//! Muse Code subscription windows.
//!
//! Uses the `dca:` device-code token from `~/.config/muse/auth.json`.
//! Keychain prompts are not used.

use crate::creds;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, field};
use crate::util;

const ID: &str = "muse";
const NAME: &str = "Muse";

pub struct Muse;

fn auth_path() -> std::path::PathBuf {
    json_api::env_any(&["MUSE_AUTH_PATH"])
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| creds::config_home().join("muse/auth.json"))
}

fn access_token() -> Option<String> {
    let data = creds::read_json(&auth_path())?;
    let token = data
        .pointer("/providers/meta/access_token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| s.starts_with("dca:"))?;
    Some(token.to_string())
}

pub(super) fn parse_key(body: &serde_json::Value) -> (Vec<MetricLine>, Option<String>) {
    let plan = json_api::text_field(body, "subs_tier_name");
    if body.get("require_payment").and_then(|v| v.as_bool()) == Some(true) {
        return (vec![json_api::text_line("Billing", "Incomplete")], plan);
    }
    if body.get("is_subs_active").and_then(|v| v.as_bool()) == Some(false) {
        return (vec![json_api::text_line("Subscription", "Inactive")], plan);
    }
    let mut lines = Vec::new();
    if let Some(window) = body.pointer("/subs_usage/window")
        && let Some(pct) = field(window, "used_percent")
    {
        let resets = window.get("resets_at").and_then(util::to_iso);
        lines.push(MetricLine::percent("5-hour", pct, resets));
    }
    if let Some(weekly) = body.pointer("/subs_usage/weekly")
        && let Some(pct) = field(weekly, "used_percent")
    {
        let resets = weekly.get("resets_at").and_then(util::to_iso);
        lines.push(MetricLine::percent("Weekly", pct, resets));
    }
    (lines, plan)
}

impl Provider for Muse {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        access_token().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(token) = access_token() else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Muse device-code token found. Run `muse login`. Keychain-only sessions are not read.",
            );
        };
        match json_api::post_headers(
            "https://api.meta.ai/muse-code/key",
            &[
                ("Authorization", &format!("Bearer {token}")),
                ("x-api-version", "1.0.0"),
            ],
            "{}",
        ) {
            Ok(data) => {
                let (lines, plan) = parse_key(&data);
                if lines.is_empty() {
                    ProviderOutput::new(
                        ID,
                        NAME,
                        vec![json_api::text_line(
                            "Quota",
                            "Not included in this login response",
                        )],
                    )
                    .with_plan(plan)
                } else {
                    ProviderOutput::new(ID, NAME, lines).with_plan(plan)
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
    fn parses_subscription_windows() {
        let (lines, plan) = parse_key(&serde_json::json!({
            "is_subs_active": true,
            "require_payment": false,
            "subs_tier_name": "Muse Code Power Usage",
            "subs_usage": {
                "window": {"used_percent": 12.0, "window_duration_mins": 300, "resets_at": 1700000000},
                "weekly": {"used_percent": 40.0, "resets_at": 1700000000}
            }
        }));
        assert_eq!(plan.as_deref(), Some("Muse Code Power Usage"));
        assert_eq!(lines.len(), 2);
    }
}
