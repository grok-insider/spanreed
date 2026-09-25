//! OpenAI API platform usage, separate from the Codex subscription provider.
//!
//! Admin keys read organization cost buckets. A normal API key falls back to
//! the legacy credit-grants balance.

use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};

const ID: &str = "openai";
const NAME: &str = "OpenAI";

pub struct OpenAI;

fn admin_key() -> Option<String> {
    env_any(&["OPENAI_ADMIN_KEY"])
}

fn api_key() -> Option<String> {
    admin_key().or_else(|| env_any(&["OPENAI_API_KEY"]))
}

/// Sum `data[].results[].amount.value` and keep the newest bucket as today.
pub(super) fn parse_costs(data: &serde_json::Value) -> Option<(f64, f64)> {
    let buckets = data.get("data")?.as_array()?;
    let mut total = 0.0;
    let mut today = 0.0;
    let mut saw = false;
    for bucket in buckets {
        let mut bucket_total = 0.0;
        let results = bucket.get("results").and_then(|v| v.as_array());
        if let Some(results) = results {
            for item in results {
                if let Some(value) = item.get("amount").and_then(|a| field(a, "value")) {
                    bucket_total += value;
                    saw = true;
                }
            }
        }
        total += bucket_total;
        today = bucket_total;
    }
    saw.then_some((today, total))
}

pub(super) fn parse_grants(data: &serde_json::Value) -> Vec<MetricLine> {
    let granted = field(data, "total_granted");
    let used = field(data, "total_used");
    let available = field(data, "total_available");
    let mut lines = Vec::new();
    if let (Some(used), Some(granted)) = (used, granted) {
        lines.push(MetricLine::dollars(
            MetricKind::Quota,
            "Credits",
            used,
            granted,
            None,
        ));
    }
    if let Some(available) = available {
        lines.push(json_api::text_line("Available", json_api::usd(available)));
    }
    lines
}

impl Provider for OpenAI {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        api_key().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = api_key() else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No OpenAI key found. Set OPENAI_ADMIN_KEY or OPENAI_API_KEY.",
            );
        };
        let admin = admin_key().is_some();
        if admin {
            let start = crate::util::now_ms() / 1000 - 7 * 86_400;
            let url = format!(
                "https://api.openai.com/v1/organization/costs?start_time={start}&bucket_width=1d&limit=31"
            );
            match json_api::get_bearer(&url, &key) {
                Ok(data) => {
                    if let Some((today, total)) = parse_costs(&data) {
                        let lines = vec![
                            json_api::text_line("Today", json_api::usd(today)),
                            json_api::text_line("Last 7 days", json_api::usd(total)),
                        ];
                        return ProviderOutput::new(ID, NAME, lines)
                            .with_plan(Some("Admin API".into()));
                    }
                    return ProviderOutput::error(ID, NAME, "OpenAI cost response had no amounts.");
                }
                Err(err) => {
                    return ProviderOutput::error(ID, NAME, err);
                }
            }
        }
        match json_api::get_bearer(
            "https://api.openai.com/v1/dashboard/billing/credit_grants",
            &key,
        ) {
            Ok(data) => {
                let lines = parse_grants(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "OpenAI credit balance response was empty.")
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
    fn sums_cost_buckets() {
        let data = serde_json::json!({
            "data": [
                {"results": [{"amount": {"value": 1.5, "currency": "usd"}}]},
                {"results": [{"amount": {"value": 2.25}}]}
            ]
        });
        assert_eq!(parse_costs(&data), Some((2.25, 3.75)));
    }

    #[test]
    fn parses_credit_grants() {
        let lines = parse_grants(&serde_json::json!({
            "total_granted": 20.0,
            "total_used": 5.0,
            "total_available": 15.0
        }));
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, MetricLine::Progress { label, .. } if label == "Credits"))
        );
    }
}
