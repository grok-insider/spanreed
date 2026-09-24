//! DeepSeek credit balance.
//!
//! `GET https://api.deepseek.com/user/balance`.

use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, number};
use crate::providers::Provider;

const ID: &str = "deepseek";
const NAME: &str = "DeepSeek";

pub struct DeepSeek;

pub(super) fn parse_balance(data: &serde_json::Value) -> Vec<MetricLine> {
    let mut lines = Vec::new();
    if data.get("is_available").and_then(|v| v.as_bool()) == Some(false) {
        lines.push(MetricLine::badge(
            MetricKind::Quota,
            "Balance",
            "Unavailable",
        ));
    }
    let Some(infos) = data.get("balance_infos").and_then(|v| v.as_array()) else {
        return lines;
    };
    for info in infos {
        let currency = info
            .get("currency")
            .and_then(|v| v.as_str())
            .unwrap_or("USD");
        if let Some(total) = info.get("total_balance").and_then(number) {
            lines.push(json_api::text_line(
                "Balance",
                format!("{total:.4} {currency}"),
            ));
        }
        if let Some(granted) = info.get("granted_balance").and_then(number) {
            lines.push(json_api::text_line(
                "Granted",
                format!("{granted:.4} {currency}"),
            ));
        }
    }
    lines
}

impl Provider for DeepSeek {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["DEEPSEEK_API_KEY", "DEEPSEEK_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["DEEPSEEK_API_KEY", "DEEPSEEK_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No DeepSeek API key found. Set DEEPSEEK_API_KEY.",
            );
        };
        match json_api::get_bearer("https://api.deepseek.com/user/balance", &key) {
            Ok(data) => {
                let lines = parse_balance(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "DeepSeek balance response was empty.")
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
    fn parses_currency_rows() {
        let lines = parse_balance(&serde_json::json!({
            "is_available": true,
            "balance_infos": [{
                "currency": "USD",
                "total_balance": "10.5",
                "granted_balance": "1.0",
                "topped_up_balance": "9.5"
            }]
        }));
        assert_eq!(lines.len(), 2);
    }
}
