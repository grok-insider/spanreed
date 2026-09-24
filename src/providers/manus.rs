//! Manus credit balances from a session token.
//!
//! `MANUS_SESSION_TOKEN` or `session_id` inside `MANUS_COOKIE`.
//! Browser cookie import is not used.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;
use crate::util;

const ID: &str = "manus";
const NAME: &str = "Manus";

pub struct Manus;

fn session_token() -> Option<String> {
    if let Some(token) = env_any(&["MANUS_SESSION_TOKEN"]) {
        return Some(token);
    }
    let cookie = env_any(&["MANUS_COOKIE"])?;
    for part in cookie.split([';', ' ']) {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("session_id=") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn credits(body: &serde_json::Value) -> &serde_json::Value {
    body.pointer("/availableCredits")
        .or_else(|| body.get("data"))
        .or_else(|| body.get("result"))
        .or_else(|| body.get("response"))
        .unwrap_or(body)
}

pub(super) fn parse_credits(body: &serde_json::Value) -> Vec<MetricLine> {
    let data = credits(body);
    let mut lines = Vec::new();
    if let (Some(total), Some(remaining)) = (
        field(data, "proMonthlyCredits"),
        field(data, "periodicCredits"),
    ) {
        let used = (total - remaining).max(0.0);
        if let Some(pct) = json_api::used_percent(used, total) {
            lines.push(MetricLine::percent("Monthly", pct, None));
        }
    }
    if let (Some(remaining), Some(max)) = (
        field(data, "refreshCredits"),
        field(data, "maxRefreshCredits"),
    ) {
        let used = (max - remaining).max(0.0);
        if let Some(pct) = json_api::used_percent(used, max) {
            let resets = data.get("nextRefreshTime").and_then(util::to_iso);
            lines.push(MetricLine::percent("Daily refresh", pct, resets));
        }
    }
    if let Some(total) = field(data, "totalCredits") {
        lines.push(json_api::text_line("Balance", format!("{total:.0}")));
    }
    lines
}

impl Provider for Manus {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        session_token().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(token) = session_token() else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Manus session token found. Set MANUS_SESSION_TOKEN.",
            );
        };
        match json_api::post_bearer(
            "https://api.manus.im/user.v1.UserService/GetAvailableCredits",
            &token,
            "{}",
        ) {
            Ok(data) => {
                let lines = parse_credits(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "Manus credit response was empty.")
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
    fn parses_monthly_and_daily() {
        let lines = parse_credits(&serde_json::json!({
            "data": {
                "proMonthlyCredits": 1000,
                "periodicCredits": 250,
                "refreshCredits": 20,
                "maxRefreshCredits": 100,
                "totalCredits": 270,
                "nextRefreshTime": "2026-09-25T00:00:00Z"
            }
        }));
        assert!(lines.len() >= 2);
    }
}
