//! DevPass (LLM Gateway) key and plan credits.
//!
//! `GET https://api.llmgateway.io/v1/key`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any};
use crate::providers::Provider;

const ID: &str = "devpass";
const NAME: &str = "DevPass";

pub struct DevPass;

fn money(data: &serde_json::Value, key: &str) -> Result<f64, String> {
    let raw = data
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("DevPass response missing {key}"))?;
    raw.parse()
        .map_err(|_| format!("DevPass {key} was not numeric"))
}

pub(super) fn parse_key(
    body: &serde_json::Value,
) -> Result<(Vec<MetricLine>, Option<String>), String> {
    let data = body.get("data").ok_or("DevPass response missing data")?;
    let plan = json_api::text_field(data, "devPlan").ok_or("DevPass response missing devPlan")?;
    if !matches!(plan.as_str(), "none" | "lite" | "pro" | "max") {
        return Err("DevPass returned an unrecognized plan.".into());
    }
    let mut lines = Vec::new();
    let key_used = money(data, "usage")?;
    lines.push(json_api::text_line(
        "All-time key usage",
        json_api::usd(key_used),
    ));
    if data.get("limit").is_some_and(|value| !value.is_null()) {
        lines.push(json_api::text_line(
            "Key spending limit",
            json_api::usd(money(data, "limit")?),
        ));
    }
    let plan_label = if plan == "none" {
        "Pay as you go".into()
    } else {
        let mut chars = plan.chars();
        let titled = match chars.next() {
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        };
        format!("DevPass {titled}")
    };
    if plan != "none" {
        let used = money(data, "devPlanCreditsUsed")?;
        let limit = money(data, "devPlanCreditsLimit")?;
        if let Some(pct) = json_api::used_percent(used, limit) {
            lines.insert(0, MetricLine::percent("Cycle", pct, None));
        }
        let weekly_used = money(data, "devPlanPremiumCreditsUsed")?;
        let weekly_limit = money(data, "devPlanPremiumWeeklyLimit")?;
        if let Some(pct) = json_api::used_percent(weekly_used, weekly_limit) {
            let resets = data
                .get("devPlanPremiumWeekResetsAt")
                .and_then(crate::util::to_iso);
            lines.insert(1, MetricLine::percent("Premium weekly", pct, resets));
        }
    }
    Ok((lines, Some(plan_label)))
}

impl Provider for DevPass {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["DEVPASS_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["DEVPASS_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No DevPass API key found. Set DEVPASS_API_KEY.",
            );
        };
        match json_api::get_bearer("https://api.llmgateway.io/v1/key", &key) {
            Ok(data) => match parse_key(&data) {
                Ok((lines, plan)) => ProviderOutput::new(ID, NAME, lines).with_plan(plan),
                Err(err) => ProviderOutput::error(ID, NAME, err),
            },
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pro_cycle() {
        let (lines, plan) = parse_key(&serde_json::json!({
            "data": {
                "devPlan": "pro",
                "usage": "3.00",
                "limit": null,
                "devPlanCreditsUsed": "10.00",
                "devPlanCreditsLimit": "40.00",
                "devPlanCreditsRemaining": "30.00",
                "devPlanPremiumCreditsUsed": "1.00",
                "devPlanPremiumWeeklyLimit": "5.00",
                "devPlanPremiumWeekResetsAt": "2026-09-28T00:00:00Z"
            }
        }))
        .unwrap();
        assert_eq!(plan.as_deref(), Some("DevPass Pro"));
        assert!(lines.len() >= 3);
    }
}
