//! ZenMux management subscription windows and PAYG balance.
//!
//! Requires a Management API key, not an inference key.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;
use crate::util;

const ID: &str = "zenmux";
const NAME: &str = "ZenMux";

pub struct ZenMux;

fn window(label: &str, quota: &serde_json::Value) -> Option<MetricLine> {
    let pct = field(quota, "usage_percentage")? * 100.0;
    let resets = quota.get("resets_at").and_then(util::to_iso);
    Some(MetricLine::percent(label, pct, resets))
}

pub(super) fn parse_detail(body: &serde_json::Value) -> (Vec<MetricLine>, Option<String>) {
    let data = body.get("data").unwrap_or(body);
    let plan = data
        .pointer("/plan/tier")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let mut lines = Vec::new();
    if let Some(quota) = data.get("quota_5_hour") {
        if let Some(line) = window("5-hour", quota) {
            lines.push(line);
        }
    }
    if let Some(quota) = data.get("quota_7_day") {
        if let Some(line) = window("7-day", quota) {
            lines.push(line);
        }
    }
    (lines, plan)
}

pub(super) fn parse_payg(body: &serde_json::Value) -> Option<MetricLine> {
    let data = body.get("data").unwrap_or(body);
    let currency = data.get("currency").and_then(|v| v.as_str())?;
    if !currency.eq_ignore_ascii_case("usd") {
        return None;
    }
    let credits = field(data, "total_credits")?;
    Some(json_api::text_line("PAYG", json_api::usd(credits)))
}

impl Provider for ZenMux {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["ZENMUX_MANAGEMENT_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["ZENMUX_MANAGEMENT_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No ZenMux Management API key found. Set ZENMUX_MANAGEMENT_API_KEY.",
            );
        };
        let detail = "https://zenmux.ai/api/v1/management/subscription/detail";
        match json_api::get_bearer(detail, &key) {
            Ok(data) => {
                let (mut lines, plan) = parse_detail(&data);
                if let Ok(payg) =
                    json_api::get_bearer("https://zenmux.ai/api/v1/management/payg/balance", &key)
                {
                    if let Some(line) = parse_payg(&payg) {
                        lines.push(line);
                    }
                }
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "ZenMux subscription response had no quota.")
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
    fn parses_flow_windows_and_payg() {
        let (lines, plan) = parse_detail(&serde_json::json!({
            "plan": {"tier": "pro", "expires_at": "2026-12-01T00:00:00Z"},
            "account_status": "healthy",
            "quota_5_hour": {"usage_percentage": 0.25, "max_flows": 100, "used_flows": 25, "remaining_flows": 75, "resets_at": "2026-09-24T20:00:00Z"},
            "quota_7_day": {"usage_percentage": 0.1, "max_flows": 1000, "used_flows": 100, "remaining_flows": 900, "resets_at": "2026-10-01T00:00:00Z"}
        }));
        assert_eq!(plan.as_deref(), Some("pro"));
        assert_eq!(lines.len(), 2);
        assert!(
            parse_payg(&serde_json::json!({"currency": "USD", "total_credits": 3.5})).is_some()
        );
    }
}
