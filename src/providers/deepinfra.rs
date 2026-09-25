//! DeepInfra billing checklist and current-month usage.
//!
//! Amounts on the checklist are USD. `usage.months[].total_cost` is cents.

use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};

const ID: &str = "deepinfra";
const NAME: &str = "DeepInfra";

pub struct DeepInfra;

pub(super) fn parse_billing(
    checklist: &serde_json::Value,
    usage: &serde_json::Value,
) -> Result<Vec<MetricLine>, String> {
    let recent = field(checklist, "recent").unwrap_or(0.0).max(0.0);
    let stripe =
        field(checklist, "stripe_balance").ok_or("DeepInfra checklist missing stripe_balance")?;
    let balance = stripe + recent;
    let months = usage
        .get("months")
        .and_then(|v| v.as_array())
        .ok_or("DeepInfra usage missing months")?;
    let month_cost = months
        .last()
        .and_then(|m| field(m, "total_cost"))
        .map(|cents| (cents / 100.0).max(0.0))
        .unwrap_or(recent);
    let balance_text = if balance > 0.0 {
        format!("{} owed", json_api::usd(balance))
    } else {
        format!("{} available", json_api::usd((-balance).max(0.0)))
    };
    let mut lines = vec![
        json_api::text_line("Balance", balance_text),
        json_api::text_line("This month", json_api::usd(month_cost)),
    ];
    if checklist.get("suspended").and_then(|v| v.as_bool()) == Some(true) {
        let reason =
            json_api::text_field(checklist, "suspend_reason").unwrap_or_else(|| "account".into());
        lines.push(MetricLine::badge(MetricKind::Quota, "Suspended", reason));
    }
    if let Some(limit) = field(checklist, "limit").filter(|n| *n > 0.0) {
        lines.push(MetricLine::dollars(
            MetricKind::Cost,
            "Billing cycle",
            recent,
            limit,
            None,
        ));
    }
    Ok(lines)
}

impl Provider for DeepInfra {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["DEEPINFRA_API_KEY", "DEEPINFRA_TOKEN"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["DEEPINFRA_API_KEY", "DEEPINFRA_TOKEN"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No DeepInfra API key found. Set DEEPINFRA_API_KEY.",
            );
        };
        let checklist = match json_api::get_bearer(
            "https://api.deepinfra.com/payment/checklist?compute_owed=true",
            &key,
        ) {
            Ok(v) => v,
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        let usage = match json_api::get_bearer(
            "https://api.deepinfra.com/payment/usage?from=current",
            &key,
        ) {
            Ok(v) => v,
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        match parse_billing(&checklist, &usage) {
            Ok(lines) => ProviderOutput::new(ID, NAME, lines),
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_month_cost_from_cents() {
        let lines = parse_billing(
            &serde_json::json!({"recent": 1.0, "stripe_balance": -4.0, "limit": 20}),
            &serde_json::json!({"months": [{"period": "2026-09", "total_cost": 250}]}),
        )
        .unwrap();
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Text { label, value, .. } if label == "This month" && value == "$2.50")));
    }
}
