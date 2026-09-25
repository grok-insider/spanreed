//! Charm Hyper credits.
//!
//! API-key path: `GET https://hyper.charm.land/v1/credits`.
//! Browser-cookie sessions are not imported.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};

const ID: &str = "hyper";
const NAME: &str = "Charm Hyper";

pub struct Hyper;

pub(super) fn parse_credits(data: &serde_json::Value) -> Result<Vec<MetricLine>, String> {
    let balance = field(data, "balance").ok_or("Charm Hyper balance missing")?;
    if balance < 0.0 {
        return Err("Charm Hyper balance must be non-negative.".into());
    }
    Ok(vec![json_api::text_line(
        "Balance",
        format!("{balance:.2} HC"),
    )])
}

impl Provider for Hyper {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["HYPER_API_KEY"]).is_some()
    }

    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        let Some(key) = env_any(&["HYPER_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Charm Hyper API key found. Set HYPER_API_KEY. Browser sessions are not imported.",
            );
        };
        match json_api::get_bearer("https://hyper.charm.land/v1/credits", &key) {
            Ok(data) => match parse_credits(&data) {
                Ok(lines) => ProviderOutput::new(ID, NAME, lines).with_plan(Some("API key".into())),
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
    fn parses_hypercredits() {
        let lines = parse_credits(&serde_json::json!({"balance": 42.5})).unwrap();
        assert!(matches!(&lines[0], MetricLine::Text { value, .. } if value == "42.50 HC"));
    }
}
