//! v0 Platform API billing and rate limits.
//!
//! Balances keep the API's own units.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};
use crate::util;

const ID: &str = "v0";
const NAME: &str = "v0";

pub struct V0;

fn quota_line(label: &str, node: &serde_json::Value, reset_key: &str) -> Option<MetricLine> {
    let limit = field(node, "limit")?;
    let remaining = field(node, "remaining")?;
    let used = (limit - remaining).max(0.0);
    let pct = json_api::used_percent(used, limit)?;
    let resets = node.get(reset_key).and_then(util::to_iso);
    Some(MetricLine::percent(label, pct, resets))
}

pub(super) fn parse_billing(body: &serde_json::Value) -> Vec<MetricLine> {
    let kind = body
        .get("billingType")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let data = body.get("data").unwrap_or(body);
    let mut lines = Vec::new();
    if kind == "token" {
        if let Some(balance) = data.get("balance")
            && let (Some(total), Some(remaining)) =
                (field(balance, "total"), field(balance, "remaining"))
        {
            let used = (total - remaining).max(0.0);
            if let Some(pct) = json_api::used_percent(used, total) {
                let resets = data.pointer("/billingCycle/end").and_then(util::to_iso);
                lines.push(MetricLine::percent("Billing", pct, resets));
            }
        }
        if let Some(on_demand) = data.pointer("/onDemand/balance").and_then(json_api::number) {
            lines.push(json_api::text_line(
                "On-demand balance",
                format!("{on_demand:.0}"),
            ));
        }
    } else if let Some(line) = quota_line("Billing", data, "reset") {
        lines.push(line);
    }
    lines
}

pub(super) fn parse_rate_limit(body: &serde_json::Value) -> Option<MetricLine> {
    quota_line("Rate limit", body, "reset")
}

impl Provider for V0 {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["V0_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["V0_API_KEY"]) else {
            return ProviderOutput::error(ID, NAME, "No v0 API key found. Set V0_API_KEY.");
        };
        let query = match env_any(&["V0_SCOPE"]) {
            Some(scope)
                if !scope.is_empty() && scope.len() <= 256 && !scope.contains(['\n', '\r']) =>
            {
                format!("?scope={}", json_api::query_escape(&scope))
            }
            _ => String::new(),
        };
        let billing_url = format!("https://api.v0.dev/v1/user/billing{query}");
        let rates_url = format!("https://api.v0.dev/v1/rate-limits{query}");
        match json_api::get_bearer(&billing_url, &key) {
            Ok(billing) => {
                let mut lines = parse_billing(&billing);
                if let Ok(rates) = json_api::get_bearer(&rates_url, &key)
                    && let Some(line) = parse_rate_limit(&rates)
                {
                    lines.push(line);
                }
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "v0 billing response was empty.")
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
    fn parses_token_billing_and_rate_limit() {
        let lines = parse_billing(&serde_json::json!({
            "billingType": "token",
            "data": {
                "balance": {"total": 100, "remaining": 25},
                "billingCycle": {"end": "2026-10-01T00:00:00Z"},
                "onDemand": {"balance": 0}
            }
        }));
        assert!(!lines.is_empty());
        assert!(
            parse_rate_limit(
                &serde_json::json!({"limit": 50, "remaining": 10, "reset": "2026-09-25T00:00:00Z"})
            )
            .is_some()
        );
    }
}
