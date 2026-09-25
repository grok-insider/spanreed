//! Neuralwatt subscription kWh and prepaid credits.
//!
//! `GET https://api.neuralwatt.com/v1/quota`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};
use crate::util;

const ID: &str = "neuralwatt";
const NAME: &str = "Neuralwatt";

pub struct Neuralwatt;

pub(super) fn parse_quota(body: &serde_json::Value) -> (Vec<MetricLine>, Option<String>) {
    let mut lines = Vec::new();
    let plan = body
        .get("subscription")
        .and_then(|v| json_api::text_field(v, "plan"));
    if let Some(sub) = body.get("subscription")
        && let (Some(used), Some(limit)) = (field(sub, "kwh_used"), field(sub, "kwh_included"))
    {
        if let Some(pct) = json_api::used_percent(used, limit) {
            let resets = sub.get("current_period_end").and_then(util::to_iso);
            lines.push(MetricLine::percent("Subscription", pct, resets));
        }
        lines.push(json_api::count_line("Energy", used, limit, "kWh", None));
    }
    if let Some(balance) = body.get("balance")
        && let Some(remaining) = field(balance, "credits_remaining_usd")
    {
        lines.push(json_api::text_line("Prepaid", json_api::usd(remaining)));
    }
    if let Some(allowance) = body.pointer("/key/allowance")
        && let (Some(spent), Some(limit)) =
            (field(allowance, "spent_usd"), field(allowance, "limit_usd"))
        && let Some(pct) = json_api::used_percent(spent, limit)
    {
        lines.push(MetricLine::percent("Key allowance", pct, None));
    }
    (lines, plan)
}

impl Provider for Neuralwatt {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["NEURALWATT_API_KEY"]).is_some()
    }

    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        let Some(key) = env_any(&["NEURALWATT_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Neuralwatt API key found. Set NEURALWATT_API_KEY.",
            );
        };
        let base = match env_any(&["NEURALWATT_API_URL"]) {
            Some(raw) => match json_api::allowed_base(&raw, false) {
                Ok(base) => base,
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            },
            None => "https://api.neuralwatt.com".into(),
        };
        match json_api::get_bearer(&json_api::join_url(&base, "v1/quota"), &key) {
            Ok(data) => {
                let (lines, plan) = parse_quota(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "Neuralwatt quota response was empty.")
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
    fn parses_kwh_and_prepaid() {
        let (lines, plan) = parse_quota(&serde_json::json!({
            "balance": {"credits_remaining_usd": 4.5, "total_credits_usd": 10.0},
            "subscription": {
                "plan": "Pro",
                "kwh_included": 100,
                "kwh_used": 25,
                "current_period_end": "2026-10-01T00:00:00Z"
            },
            "key": {"allowance": {"limit_usd": 20, "spent_usd": 5}}
        }));
        assert_eq!(plan.as_deref(), Some("Pro"));
        assert!(lines.len() >= 3);
    }
}
