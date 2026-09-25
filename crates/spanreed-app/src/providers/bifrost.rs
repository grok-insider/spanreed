//! Bifrost virtual-key governance quota.
//!
//! `GET {base}/api/governance/virtual-keys/quota` with `x-bf-vk`.

use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};

const ID: &str = "bifrost";
const NAME: &str = "Bifrost";

pub struct Bifrost;

pub(super) fn parse_quota(body: &serde_json::Value) -> Vec<MetricLine> {
    let mut lines = Vec::new();
    if body.get("is_active").and_then(|v| v.as_bool()) == Some(false) {
        lines.push(MetricLine::badge(MetricKind::Quota, "Key", "Inactive"));
    }
    let budgets = body
        .get("budgets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for budget in budgets.iter().take(4) {
        let used = field(budget, "current_usage").unwrap_or(0.0);
        let limit = field(budget, "max_limit").unwrap_or(0.0);
        let label = json_api::text_field(budget, "name").unwrap_or_else(|| "Budget".into());
        if limit > 0.0 {
            lines.push(MetricLine::dollars(
                MetricKind::Quota,
                label,
                used,
                limit,
                None,
            ));
        } else if used > 0.0 {
            lines.push(json_api::text_line(&label, json_api::usd(used)));
        }
    }
    if let Some(rates) = body.get("rate_limits").and_then(|v| v.as_array()) {
        for rate in rates.iter().take(4) {
            let used = field(rate, "current_usage").or_else(|| field(rate, "usage"));
            let limit = field(rate, "limit").or_else(|| field(rate, "max_limit"));
            if let (Some(used), Some(limit)) = (used, limit)
                && let Some(pct) = json_api::used_percent(used, limit)
            {
                let label =
                    json_api::text_field(rate, "name").unwrap_or_else(|| "Rate limit".into());
                lines.push(MetricLine::percent(label, pct, None));
            }
        }
    }
    lines
}

impl Provider for Bifrost {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["BIFROST_API_KEY"]).is_some() && env_any(&["BIFROST_BASE_URL"]).is_some()
    }

    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        let Some(key) = env_any(&["BIFROST_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Bifrost virtual key found. Set BIFROST_API_KEY.",
            );
        };
        let Some(raw) = env_any(&["BIFROST_BASE_URL"]) else {
            return ProviderOutput::error(ID, NAME, "Set BIFROST_BASE_URL for the gateway origin.");
        };
        let base = match json_api::allowed_base(&raw, true) {
            Ok(base) => base,
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        match json_api::get_headers(
            &json_api::join_url(&base, "api/governance/virtual-keys/quota"),
            &[("x-bf-vk", &key)],
        ) {
            Ok(data) => {
                let lines = parse_quota(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "Bifrost quota response had no budgets.")
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
    fn parses_budget_and_rate() {
        let lines = parse_quota(&serde_json::json!({
            "is_active": true,
            "budgets": [{"name": "Daily", "current_usage": 1.5, "max_limit": 10.0}],
            "rate_limits": [{"name": "Requests", "current_usage": 4, "limit": 20}]
        }));
        assert_eq!(lines.len(), 2);
    }
}
