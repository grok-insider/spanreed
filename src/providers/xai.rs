//! xAI developer-platform prepaid balance.
//!
//! Separate from the Grok subscription provider.
//! `GET https://management-api.x.ai/v1/billing/teams/{team}/prepaid/balance`.

use crate::model::ProviderOutput;
use crate::providers::json_api::{self, env_any};
use crate::providers::Provider;

const ID: &str = "xai";
const NAME: &str = "xAI";

pub struct Xai;

/// `total.val` is an inverted ledger in string USD cents.
pub(super) fn parse_balance(body: &serde_json::Value) -> Option<f64> {
    let raw = body.pointer("/total/val")?.as_str()?.trim();
    let cents: f64 = raw.parse().ok()?;
    Some(-cents / 100.0)
}

impl Provider for Xai {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["XAI_MANAGEMENT_API_KEY"]).is_some() && env_any(&["XAI_TEAM_ID"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["XAI_MANAGEMENT_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No xAI Management API key found. Set XAI_MANAGEMENT_API_KEY. Inference keys are not accepted.",
            );
        };
        let Some(team) = env_any(&["XAI_TEAM_ID"]) else {
            return ProviderOutput::error(ID, NAME, "Set XAI_TEAM_ID for the xAI team to read.");
        };
        let Some(team) = json_api::safe_segment(&team, 128) else {
            return ProviderOutput::error(ID, NAME, "XAI_TEAM_ID is not a valid team id.");
        };
        let url = format!("https://management-api.x.ai/v1/billing/teams/{team}/prepaid/balance");
        match json_api::get_bearer(&url, &key) {
            Ok(data) => match parse_balance(&data) {
                Some(balance) => ProviderOutput::new(
                    ID,
                    NAME,
                    vec![json_api::text_line("Prepaid", json_api::usd(balance))],
                ),
                None => ProviderOutput::error(
                    ID,
                    NAME,
                    "xAI balance response did not include total.val.",
                ),
            },
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negates_cent_ledger() {
        assert_eq!(
            parse_balance(&serde_json::json!({"total": {"val": "-1000"}})),
            Some(10.0)
        );
        assert!(parse_balance(&serde_json::json!({"total": {"val": "nope"}})).is_none());
    }
}
