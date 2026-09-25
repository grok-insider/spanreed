//! sub2api group usage.
//!
//! `GET {base}/v1/usage` with the group API key. HTTPS, or loopback/private HTTP.

use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};

const ID: &str = "sub2api";
const NAME: &str = "sub2api";

pub struct Sub2Api;

pub(super) fn parse_usage(body: &serde_json::Value) -> (Vec<MetricLine>, Option<String>) {
    let data = body.get("data").unwrap_or(body);
    let plan = json_api::text_field(data, "planName");
    let mut lines = Vec::new();
    if let Some(quota) = data.get("quota")
        && let (Some(used), Some(limit)) = (field(quota, "used"), field(quota, "limit"))
        && let Some(pct) = json_api::used_percent(used, limit)
    {
        lines.push(MetricLine::percent("Quota", pct, None));
    }
    if let Some(sub) = data.get("subscription") {
        for (label, key) in [
            ("Daily", "daily_usage_usd"),
            ("Weekly", "weekly_usage_usd"),
            ("Monthly", "monthly_usage_usd"),
        ] {
            let limit_key = key.replace("_usage_", "_limit_");
            if let (Some(used), Some(limit)) = (field(sub, key), field(sub, &limit_key))
                && limit > 0.0
            {
                lines.push(MetricLine::dollars(
                    MetricKind::Quota,
                    label,
                    used,
                    limit,
                    None,
                ));
            }
        }
    }
    if let Some(balance) = field(data, "balance") {
        lines.push(json_api::text_line("Wallet", json_api::usd(balance)));
    }
    (lines, plan)
}

impl Provider for Sub2Api {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["SUB2API_API_KEY"]).is_some() && env_any(&["SUB2API_BASE_URL"]).is_some()
    }

    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        let Some(key) = env_any(&["SUB2API_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No sub2api API key found. Set SUB2API_API_KEY.",
            );
        };
        let Some(raw) = env_any(&["SUB2API_BASE_URL"]) else {
            return ProviderOutput::error(ID, NAME, "Set SUB2API_BASE_URL for your deployment.");
        };
        let base = match json_api::allowed_base(&raw, true) {
            Ok(base) => base,
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        let url = if base.ends_with("/v1") {
            json_api::join_url(&base, "usage")
        } else {
            json_api::join_url(&base, "v1/usage")
        };
        match json_api::get_bearer(&url, &key) {
            Ok(data) => {
                let (lines, plan) = parse_usage(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "sub2api usage response was empty.")
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
    fn parses_quota_and_wallet() {
        let (lines, plan) = parse_usage(&serde_json::json!({
            "planName": "Claude",
            "quota": {"used": 2, "limit": 10, "remaining": 8, "unit": "USD"},
            "balance": 4.5
        }));
        assert_eq!(plan.as_deref(), Some("Claude"));
        assert_eq!(lines.len(), 2);
    }
}
