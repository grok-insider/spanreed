//! ai& organization spend from request logs.
//!
//! Sums `cost` for the newest pages of `GET https://api.aiand.com/logs`.

use crate::model::ProviderOutput;
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, number};

const ID: &str = "aiand";
const NAME: &str = "ai&";

pub struct AiAnd;

pub(super) fn parse_page(body: &serde_json::Value) -> (f64, Option<String>, bool) {
    let rows = body
        .get("logs")
        .or_else(|| body.get("data"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let currency = rows
        .first()
        .and_then(|row| json_api::text_field(row, "currency"));
    let mut total = 0.0;
    for row in &rows {
        let row_currency = json_api::text_field(row, "currency");
        if currency.is_some() && row_currency != currency {
            continue;
        }
        if let Some(cost) = row.get("cost").and_then(number) {
            total += cost;
        }
    }
    let more = json_api::text_field(body, "next_after").is_some()
        && json_api::text_field(body, "next_after_id").is_some();
    (total, currency, more)
}

impl Provider for AiAnd {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["AIAND_API_KEY"]).is_some()
    }

    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        let Some(key) = env_any(&["AIAND_API_KEY"]) else {
            return ProviderOutput::error(ID, NAME, "No ai& API key found. Set AIAND_API_KEY.");
        };
        let mut total = 0.0;
        let mut currency = None;
        let mut after: Option<String> = None;
        let mut after_id: Option<String> = None;
        let mut partial = false;
        for page in 0..5 {
            let mut url = "https://api.aiand.com/logs?range=30days&limit=100".to_string();
            if let (Some(after), Some(after_id)) = (after.as_deref(), after_id.as_deref()) {
                url.push_str(&format!(
                    "&next_after={}&next_after_id={}",
                    json_api::query_escape(after),
                    json_api::query_escape(after_id)
                ));
            }
            let body = match json_api::get_bearer(&url, &key) {
                Ok(body) => body,
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            };
            let (page_total, page_currency, more) = parse_page(&body);
            if currency.is_none() {
                currency = page_currency;
            }
            total += page_total;
            if !more {
                partial = false;
                break;
            }
            after = json_api::text_field(&body, "next_after");
            after_id = json_api::text_field(&body, "next_after_id");
            partial = page == 4;
            if after.is_none() || after_id.is_none() {
                partial = false;
                break;
            }
        }
        let Some(currency) = currency else {
            return ProviderOutput::error(
                ID,
                NAME,
                "ai& returned no request costs for the last 30 days.",
            );
        };
        let label = if partial {
            "Last 30 days (partial)"
        } else {
            "Last 30 days"
        };
        ProviderOutput::new(
            ID,
            NAME,
            vec![json_api::text_line(label, format!("{total:.4} {currency}"))],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_matching_currency() {
        let (total, currency, more) = parse_page(&serde_json::json!({
            "logs": [
                {"cost": "1.25", "currency": "USD"},
                {"cost": "0.50", "currency": "USD"},
                {"cost": "9", "currency": "JPY"}
            ],
            "next_after": "1",
            "next_after_id": "2"
        }));
        assert_eq!(total, 1.75);
        assert_eq!(currency.as_deref(), Some("USD"));
        assert!(more);
    }
}
