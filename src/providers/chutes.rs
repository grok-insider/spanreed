//! Chutes subscription and quota windows.
//!
//! `GET https://api.chutes.ai/users/me/subscription_usage`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};
use crate::util;

const ID: &str = "chutes";
const NAME: &str = "Chutes";

pub struct Chutes;

fn window(
    label: &str,
    node: &serde_json::Value,
    used_keys: &[&str],
    reset_keys: &[&str],
) -> Option<MetricLine> {
    let used = used_keys.iter().find_map(|key| field(node, key))?;
    let limit = field(node, "limit")?;
    let resets = reset_keys
        .iter()
        .find_map(|key| node.get(*key).and_then(util::to_iso));
    json_api::used_percent(used, limit).map(|pct| MetricLine::percent(label, pct, resets))
}

pub(super) fn parse_subscription(body: &serde_json::Value) -> (Vec<MetricLine>, Option<String>) {
    let root = body.get("data").unwrap_or(body);
    let plan = root
        .pointer("/subscription/plan_name")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let mut lines = Vec::new();
    if let Some(rolling) = root.get("rolling_window").or_else(|| root.get("rolling"))
        && let Some(line) = window(
            "Rolling",
            rolling,
            &["requests", "used"],
            &["reset_at", "resets_at"],
        )
    {
        lines.push(line);
    }
    if let Some(monthly) = root.get("monthly")
        && let Some(line) = window(
            "Monthly",
            monthly,
            &["used", "requests"],
            &["resets_at", "reset_at"],
        )
    {
        lines.push(line);
    }
    (lines, plan)
}

pub(super) fn parse_quotas(body: &serde_json::Value) -> Vec<MetricLine> {
    let rows = body
        .get("quotas")
        .and_then(|v| v.as_array())
        .or_else(|| body.as_array());
    let Some(rows) = rows else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let used = field(row, "used")?;
            let limit = field(row, "quota").or_else(|| field(row, "limit"))?;
            let label = json_api::text_field(row, "chute_id").unwrap_or_else(|| "Quota".into());
            json_api::used_percent(used, limit).map(|pct| MetricLine::percent(label, pct, None))
        })
        .collect()
}

impl Provider for Chutes {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["CHUTES_API_KEY"]).is_some()
    }

    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        let Some(key) = env_any(&["CHUTES_API_KEY"]) else {
            return ProviderOutput::error(ID, NAME, "No Chutes API key found. Set CHUTES_API_KEY.");
        };
        let base = match env_any(&["CHUTES_API_URL"]) {
            Some(raw) => match json_api::allowed_base(&raw, false) {
                Ok(base) => base,
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            },
            None => "https://api.chutes.ai".into(),
        };
        match json_api::get_bearer(
            &json_api::join_url(&base, "users/me/subscription_usage"),
            &key,
        ) {
            Ok(data) => {
                let (mut lines, plan) = parse_subscription(&data);
                if lines.is_empty()
                    && let Ok(quotas) =
                        json_api::get_bearer(&json_api::join_url(&base, "users/me/quotas"), &key)
                {
                    lines = parse_quotas(&quotas);
                }
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "Chutes response had no usage windows.")
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
        let (lines, plan) = parse_subscription(&serde_json::json!({
            "subscription": {"active": true, "plan_name": "Pro", "current_period_end": "2026-10-01T00:00:00Z"},
            "monthly": {"used": 10, "limit": 100, "resets_at": "2026-10-01T00:00:00Z", "unit": "requests"},
            "rolling_window": {"requests": 2, "limit": 20, "window_minutes": 240, "reset_at": "2026-09-24T20:00:00Z"}
        }));
        assert_eq!(plan.as_deref(), Some("Pro"));
        assert_eq!(lines.len(), 2);
    }
}
