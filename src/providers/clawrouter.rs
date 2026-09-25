//! ClawRouter policy budget and routed usage.
//!
//! Default origin `https://clawrouter.openclaw.ai`. Overrides must be HTTPS.

use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};

const ID: &str = "clawrouter";
const NAME: &str = "ClawRouter";

pub struct ClawRouter;

fn micros(node: &serde_json::Value, key: &str) -> Option<f64> {
    field(node, key).map(|value| value / 1_000_000.0)
}

pub(super) fn parse_usage(body: &serde_json::Value) -> Vec<MetricLine> {
    let Some(budget) = body.get("budget") else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if let (Some(spent), Some(limit)) =
        (micros(budget, "spentMicros"), micros(budget, "limitMicros"))
    {
        if limit > 0.0 {
            lines.push(MetricLine::dollars(
                MetricKind::Quota,
                "Monthly",
                spent,
                limit,
                None,
            ));
        } else {
            lines.push(json_api::text_line("Spend", json_api::usd(spent)));
        }
    }
    if let Some(requests) = body
        .pointer("/usage/summary/requestCount")
        .and_then(field_value)
    {
        lines.push(json_api::text_line("Requests", format!("{requests:.0}")));
    }
    lines
}

fn field_value(value: &serde_json::Value) -> Option<f64> {
    json_api::number(value)
}

impl Provider for ClawRouter {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["CLAWROUTER_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["CLAWROUTER_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No ClawRouter API key found. Set CLAWROUTER_API_KEY.",
            );
        };
        let base = match env_any(&["CLAWROUTER_BASE_URL"]) {
            Some(raw) => match json_api::allowed_base(&raw, false) {
                Ok(base) => base,
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            },
            None => "https://clawrouter.openclaw.ai".into(),
        };
        let url = if base.ends_with("/v1") {
            json_api::join_url(&base, "usage")
        } else {
            json_api::join_url(&base, "v1/usage")
        };
        match json_api::get_bearer(&url, &key) {
            Ok(data) => {
                let lines = parse_usage(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "ClawRouter usage response was empty.")
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
    fn converts_micros() {
        let lines = parse_usage(&serde_json::json!({
            "budget": {"configured": true, "ledger": "monthly", "limitMicros": 10_000_000, "spentMicros": 2_500_000},
            "usage": {"summary": {"requestCount": 8, "successCount": 8, "errorCount": 0}, "providers": []}
        }));
        assert_eq!(lines.len(), 2);
    }
}
