//! Poe point balance.
//!
//! `GET https://api.poe.com/usage/current_balance`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, number};

const ID: &str = "poe";
const NAME: &str = "Poe";

pub struct Poe;

pub(super) fn parse_balance(data: &serde_json::Value) -> Result<Vec<MetricLine>, String> {
    let points = data
        .get("current_point_balance")
        .and_then(number)
        .ok_or("Poe balance response missing current_point_balance")?;
    Ok(vec![json_api::text_line("Points", format!("{points:.0}"))])
}

impl Provider for Poe {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["POE_API_KEY"]).is_some()
    }

    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        let Some(key) = env_any(&["POE_API_KEY"]) else {
            return ProviderOutput::error(ID, NAME, "No Poe API key found. Set POE_API_KEY.");
        };
        match json_api::get_bearer("https://api.poe.com/usage/current_balance", &key) {
            Ok(data) => match parse_balance(&data) {
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
    fn parses_points() {
        let lines = parse_balance(&serde_json::json!({"current_point_balance": 1250})).unwrap();
        assert!(matches!(&lines[0], MetricLine::Text { value, .. } if value == "1250"));
    }
}
