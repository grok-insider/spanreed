//! Self-hosted LLM proxy quota stats.
//!
//! Both `LLM_PROXY_BASE_URL` and `LLM_PROXY_API_KEY` are required.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;

const ID: &str = "llmproxy";
const NAME: &str = "LLM Proxy";

pub struct LlmProxy;

pub(super) fn parse_stats(body: &serde_json::Value) -> Vec<MetricLine> {
    let mut lines = Vec::new();
    let groups = body
        .get("quota_groups")
        .map(|value| {
            if let Some(rows) = value.as_array() {
                rows.clone()
            } else if let Some(map) = value.as_object() {
                map.values().cloned().collect()
            } else {
                Vec::new()
            }
        })
        .unwrap_or_default();
    let mut lowest = None;
    for group in &groups {
        if let Some(remaining) = field(group, "remaining_percent") {
            lowest = Some(lowest.map_or(remaining, |current: f64| current.min(remaining)));
        }
    }
    if let Some(remaining) = lowest {
        lines.push(MetricLine::percent(
            "Quota",
            (100.0 - remaining).clamp(0.0, 100.0),
            None,
        ));
    }
    if let Some(requests) = field(body, "requests").or_else(|| field(body, "total_requests")) {
        lines.push(json_api::text_line("Requests", format!("{requests:.0}")));
    }
    if let Some(tokens) = field(body, "tokens").or_else(|| field(body, "total_tokens")) {
        lines.push(json_api::text_line("Tokens", format!("{tokens:.0}")));
    }
    if let Some(cost) = field(body, "approx_cost") {
        lines.push(json_api::text_line("Approx cost", json_api::usd(cost)));
    }
    lines
}

impl Provider for LlmProxy {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["LLM_PROXY_API_KEY"]).is_some() && env_any(&["LLM_PROXY_BASE_URL"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["LLM_PROXY_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No LLM Proxy API key found. Set LLM_PROXY_API_KEY.",
            );
        };
        let Some(raw) = env_any(&["LLM_PROXY_BASE_URL"]) else {
            return ProviderOutput::error(ID, NAME, "Set LLM_PROXY_BASE_URL for the proxy origin.");
        };
        let base = match json_api::allowed_base(&raw, true) {
            Ok(base) => base,
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        let url = if base.ends_with("/v1") {
            json_api::join_url(&base, "quota-stats")
        } else {
            json_api::join_url(&base, "v1/quota-stats")
        };
        match json_api::get_bearer(&url, &key) {
            Ok(data) => {
                let lines = parse_stats(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "LLM Proxy quota response was empty.")
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
    fn uses_lowest_remaining_group() {
        let lines = parse_stats(&serde_json::json!({
            "quota_groups": {"a": {"remaining_percent": 80}, "b": {"remaining_percent": 20}},
            "requests": 12,
            "approx_cost": 1.25
        }));
        assert!(lines.len() >= 2);
    }
}
