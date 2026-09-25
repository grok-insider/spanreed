//! Vercel AI Gateway credits.
//!
//! `GET https://ai-gateway.vercel.sh/v1/credits` with `AI_GATEWAY_API_KEY`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any};

const ID: &str = "vercel";
const NAME: &str = "Vercel AI Gateway";

pub struct Vercel;

fn money(value: Option<&serde_json::Value>) -> Result<f64, String> {
    let raw = value
        .and_then(|v| v.as_str())
        .ok_or("Vercel credit amount missing")?;
    raw.parse()
        .map_err(|_| "Vercel credit amount was not numeric".into())
}

pub(super) fn parse_credits(data: &serde_json::Value) -> Result<Vec<MetricLine>, String> {
    let balance = money(data.get("balance"))?;
    let spent = money(data.get("total_used"))?;
    if spent < 0.0 {
        return Err("Vercel lifetime spend was negative.".into());
    }
    Ok(vec![
        json_api::text_line("Available balance", json_api::usd(balance)),
        json_api::text_line("Lifetime spend", json_api::usd(spent)),
    ])
}

impl Provider for Vercel {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["AI_GATEWAY_API_KEY"]).is_some()
    }

    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        let Some(key) = env_any(&["AI_GATEWAY_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Vercel AI Gateway key found. Set AI_GATEWAY_API_KEY.",
            );
        };
        match json_api::get_bearer("https://ai-gateway.vercel.sh/v1/credits", &key) {
            Ok(data) => match parse_credits(&data) {
                Ok(lines) => ProviderOutput::new(ID, NAME, lines),
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
    fn parses_balance_and_spend() {
        let lines =
            parse_credits(&serde_json::json!({"balance": "4.00", "total_used": "12.25"})).unwrap();
        assert_eq!(lines.len(), 2);
    }
}
