//! Venice API balance.
//!
//! `GET https://api.venice.ai/api/v1/billing/balance`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field, number};

const ID: &str = "venice";
const NAME: &str = "Venice";

pub struct Venice;

pub(super) fn parse_balance(data: &serde_json::Value) -> Result<Vec<MetricLine>, String> {
    let can = data
        .get("canConsume")
        .and_then(|v| v.as_bool())
        .ok_or("Venice response missing canConsume")?;
    let balances = data
        .get("balances")
        .ok_or("Venice response missing balances")?;
    let diem = balances.get("diem").and_then(number);
    let usd = balances.get("usd").and_then(number);
    let allocation = field(data, "diemEpochAllocation");
    let currency = json_api::text_field(data, "consumptionCurrency")
        .map(|c| c.to_uppercase())
        .unwrap_or_default();
    if !can {
        return Ok(vec![MetricLine::percent("Balance", 100.0, None)]);
    }
    if currency == "USD"
        && let Some(usd) = usd
    {
        return Ok(vec![json_api::text_line(
            "USD remaining",
            json_api::usd(usd),
        )]);
    }
    if let (Some(diem), Some(allocation)) = (diem, allocation)
        && allocation > 0.0
    {
        let used = (allocation - diem).max(0.0);
        if let Some(pct) = json_api::used_percent(used, allocation) {
            return Ok(vec![MetricLine::percent("DIEM", pct, None)]);
        }
    }
    if let Some(diem) = diem.filter(|n| *n > 0.0) {
        return Ok(vec![json_api::text_line(
            "DIEM remaining",
            format!("{diem:.2}"),
        )]);
    }
    if let Some(usd) = usd.filter(|n| *n > 0.0) {
        return Ok(vec![json_api::text_line(
            "USD remaining",
            json_api::usd(usd),
        )]);
    }
    Ok(vec![MetricLine::percent("Balance", 100.0, None)])
}

impl Provider for Venice {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["VENICE_API_KEY", "VENICE_KEY"]).is_some()
    }

    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        let Some(key) = env_any(&["VENICE_API_KEY", "VENICE_KEY"]) else {
            return ProviderOutput::error(ID, NAME, "No Venice API key found. Set VENICE_API_KEY.");
        };
        match json_api::get_bearer("https://api.venice.ai/api/v1/billing/balance", &key) {
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
    fn parses_diem_epoch() {
        let lines = parse_balance(&serde_json::json!({
            "canConsume": true,
            "consumptionCurrency": "DIEM",
            "balances": {"diem": 40.0, "usd": 0},
            "diemEpochAllocation": 100.0
        }))
        .unwrap();
        assert!(
            matches!(&lines[0], MetricLine::Progress { used, .. } if (*used - 60.0).abs() < 0.01)
        );
    }
}
