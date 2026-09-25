//! Fireworks 30-day rated spend.
//!
//! Discovers the account slug, then reads the billing summary.

use crate::model::ProviderOutput;
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};

const ID: &str = "fireworks";
const NAME: &str = "Fireworks";

pub struct Fireworks;

pub(super) fn parse_accounts(body: &serde_json::Value) -> Vec<String> {
    let Some(accounts) = body.get("accounts").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut slugs = Vec::new();
    for account in accounts {
        let name = ["accountId", "id", "name"]
            .iter()
            .find_map(|key| json_api::text_field(account, key));
        let Some(name) = name else {
            continue;
        };
        let slug = name.split('/').rfind(|part| !part.is_empty());
        if let Some(slug) = slug.and_then(|slug| json_api::safe_segment(slug, 256))
            && !slugs.iter().any(|existing| existing == slug)
        {
            slugs.push(slug.to_string());
        }
    }
    slugs.sort();
    slugs
}

pub(super) fn parse_spend(body: &serde_json::Value) -> Option<(String, f64)> {
    let items = body.get("lineItems").and_then(|v| v.as_array())?;
    let mut currency = None;
    let mut total = 0.0;
    for item in items {
        let Some(cost) = item.get("totalCost") else {
            continue;
        };
        let Some(units) = cost.get("units").and_then(|v| v.as_str()) else {
            continue;
        };
        let Ok(units) = units.trim().parse::<f64>() else {
            continue;
        };
        let Some(nanos) = field(cost, "nanos") else {
            continue;
        };
        let Some(code) = json_api::text_field(cost, "currencyCode") else {
            continue;
        };
        if currency.is_none() {
            currency = Some(code.clone());
        }
        if currency.as_deref() == Some(code.as_str()) {
            total += units + nanos / 1e9;
        }
    }
    currency.map(|code| (code, total))
}

impl Provider for Fireworks {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["FIREWORKS_API_KEY", "FIREWORKS_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["FIREWORKS_API_KEY", "FIREWORKS_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Fireworks API key found. Set FIREWORKS_API_KEY.",
            );
        };
        let configured = env_any(&["FIREWORKS_ACCOUNT_SLUG"]);
        if let Some(slug) = &configured
            && json_api::safe_segment(slug, 256).is_none()
        {
            return ProviderOutput::error(ID, NAME, "FIREWORKS_ACCOUNT_SLUG is invalid.");
        }
        let mut slugs = Vec::new();
        let mut token = String::new();
        for _ in 0..5 {
            let url = if token.is_empty() {
                "https://api.fireworks.ai/v1/accounts".to_string()
            } else {
                format!(
                    "https://api.fireworks.ai/v1/accounts?pageToken={}",
                    json_api::query_escape(&token)
                )
            };
            let body = match json_api::get_bearer(&url, &key) {
                Ok(body) => body,
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            };
            slugs.extend(parse_accounts(&body));
            token = json_api::text_field(&body, "nextPageToken").unwrap_or_default();
            if token.is_empty() {
                break;
            }
        }
        slugs.sort();
        slugs.dedup();
        let slug = if let Some(slug) = configured {
            if !slugs.iter().any(|found| found == &slug) {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "FIREWORKS_ACCOUNT_SLUG is not in the accounts visible to this key.",
                );
            }
            slug
        } else if slugs.len() == 1 {
            slugs.remove(0)
        } else {
            return ProviderOutput::error(
                ID,
                NAME,
                "Set FIREWORKS_ACCOUNT_SLUG. This key can see more than one Fireworks account.",
            );
        };
        let start = json_api::utc_ymd(30);
        let end = json_api::utc_ymd(0);
        let url = format!(
            "https://api.fireworks.ai/v1/accounts/{slug}/billing/summary?startTime={start}T00:00:00Z&endTime={end}T23:59:59Z"
        );
        match json_api::get_bearer(&url, &key) {
            Ok(data) => match parse_spend(&data) {
                Some((currency, total)) => ProviderOutput::new(
                    ID,
                    NAME,
                    vec![json_api::text_line(
                        "Last 30 days",
                        format!("{total:.2} {currency}"),
                    )],
                ),
                None => {
                    ProviderOutput::error(ID, NAME, "Fireworks billing summary had no rated spend.")
                }
            },
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_one_currency() {
        let slugs = parse_accounts(&serde_json::json!({
            "accounts": [{"name": "accounts/demo"}, {"accountId": "other"}]
        }));
        assert_eq!(slugs, vec!["demo".to_string(), "other".to_string()]);
        let spend = parse_spend(&serde_json::json!({
            "lineItems": [
                {"totalCost": {"units": "1", "nanos": 500_000_000, "currencyCode": "USD"}},
                {"totalCost": {"units": "2", "nanos": 0, "currencyCode": "EUR"}}
            ]
        }));
        assert_eq!(spend, Some(("USD".into(), 1.5)));
    }
}
