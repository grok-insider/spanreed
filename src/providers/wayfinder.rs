//! Wayfinder local gateway savings.
//!
//! Detects only when `WAYFINDER_GATEWAY_URL` is set. A force probe uses
//! `http://127.0.0.1:8088`. HTTP is limited to loopback.

use crate::model::ProviderOutput;
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};

const ID: &str = "wayfinder";
const NAME: &str = "Wayfinder";

pub struct Wayfinder;

pub(super) fn parse_health(body: &serde_json::Value) -> Vec<crate::model::MetricLine> {
    let status = json_api::text_field(body, "status").unwrap_or_else(|| "unknown".into());
    vec![json_api::text_line("Gateway", status)]
}

pub(super) fn parse_savings(body: &serde_json::Value) -> Vec<crate::model::MetricLine> {
    let mut lines = Vec::new();
    if let Some(requests) = field(body, "requests") {
        lines.push(json_api::text_line("Requests", format!("{requests:.0}")));
    }
    if let Some(saved) = field(body, "savings").or_else(|| field(body, "saved_usd")) {
        lines.push(json_api::text_line("Savings", json_api::usd(saved)));
    } else if let Some(pct) = field(body, "savings_percent") {
        lines.push(json_api::text_line("Savings", format!("{pct:.1}%")));
    }
    lines
}

impl Provider for Wayfinder {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["WAYFINDER_GATEWAY_URL"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let raw =
            env_any(&["WAYFINDER_GATEWAY_URL"]).unwrap_or_else(|| "http://127.0.0.1:8088".into());
        let base = match json_api::allowed_loopback(&raw) {
            Ok(base) => base,
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        let health = match json_api::get_headers(&json_api::join_url(&base, "healthz"), &[]) {
            Ok(data) => data,
            Err(err) => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    format!("{err} Start the gateway with `wayfinder-router serve`."),
                );
            }
        };
        let mut lines = parse_health(&health);
        if let Ok(savings) =
            json_api::get_headers(&json_api::join_url(&base, "v1/savings?period=30d"), &[])
        {
            lines.extend(parse_savings(&savings));
        }
        if lines.is_empty() {
            ProviderOutput::error(ID, NAME, "Wayfinder health response was empty.")
        } else {
            ProviderOutput::new(ID, NAME, lines)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_health_and_savings() {
        let mut lines = parse_health(&serde_json::json!({"status": "ok"}));
        lines.extend(parse_savings(
            &serde_json::json!({"requests": 12, "savings": 1.5}),
        ));
        assert_eq!(lines.len(), 3);
    }
}
