//! Atlas Cloud account balance.
//!
//! `GET https://api.atlascloud.ai/public/v1/balance`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any};

const ID: &str = "atlascloud";
const NAME: &str = "Atlas Cloud";

pub struct AtlasCloud;

pub(super) fn parse_balance(data: &serde_json::Value) -> Result<Vec<MetricLine>, String> {
    if data.get("object").and_then(|v| v.as_str()) != Some("balance")
        || data.get("scope").and_then(|v| v.as_str()) != Some("account")
        || data.pointer("/available/currency").and_then(|v| v.as_str()) != Some("usd")
    {
        return Err("Atlas Cloud returned an unrecognized USD account balance.".into());
    }
    let raw = data
        .pointer("/available/value")
        .and_then(|v| v.as_str())
        .ok_or("Atlas Cloud balance value missing")?;
    let amount: f64 = raw
        .parse()
        .map_err(|_| "Atlas Cloud balance value was not numeric")?;
    Ok(vec![json_api::text_line(
        "Available balance",
        json_api::usd(amount),
    )])
}

impl Provider for AtlasCloud {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["ATLASCLOUD_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["ATLASCLOUD_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Atlas Cloud API key found. Set ATLASCLOUD_API_KEY.",
            );
        };
        match json_api::get_bearer("https://api.atlascloud.ai/public/v1/balance", &key) {
            Ok(data) => match parse_balance(&data) {
                Ok(lines) => ProviderOutput::new(ID, NAME, lines).with_plan(Some("API".into())),
                Err(err) => ProviderOutput::error(ID, NAME, err),
            },
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_available_usd() {
        let lines = parse_balance(&serde_json::json!({
            "object": "balance",
            "scope": "account",
            "available": {"currency": "usd", "value": "18.50"}
        }))
        .unwrap();
        assert!(matches!(&lines[0], MetricLine::Text { value, .. } if value == "$18.50"));
    }
}
