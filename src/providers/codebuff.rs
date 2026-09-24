//! Codebuff credit balance and optional weekly limit.
//!
//! `POST https://www.codebuff.com/api/v1/usage`. A CLI session token also
//! reads `GET /api/user/subscription`.

use crate::creds;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;
use crate::util;

const ID: &str = "codebuff";
const NAME: &str = "Codebuff";

pub struct Codebuff;

enum Token {
    ApiKey(String),
    Session(String),
}

fn session_token() -> Option<String> {
    let path = creds::config_home().join("manicode/credentials.json");
    let data = creds::read_json(&path)?;
    data.pointer("/default/authToken")
        .and_then(|v| v.as_str())
        .or_else(|| data.get("authToken").and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn token() -> Option<Token> {
    if let Some(key) = env_any(&["CODEBUFF_API_KEY"]) {
        return Some(Token::ApiKey(key));
    }
    session_token().map(Token::Session)
}

pub(super) fn parse_usage(body: &serde_json::Value) -> Vec<MetricLine> {
    let used = field(body, "usage").or_else(|| field(body, "used"));
    let limit = field(body, "quota").or_else(|| field(body, "limit"));
    let mut lines = Vec::new();
    if let (Some(used), Some(limit)) = (used, limit) {
        if let Some(pct) = json_api::used_percent(used, limit) {
            let resets = body.get("next_quota_reset").and_then(util::to_iso);
            lines.push(MetricLine::percent("Credits", pct, resets));
        }
    }
    if let Some(remaining) = field(body, "remainingBalance").or_else(|| field(body, "remaining")) {
        lines.push(json_api::text_line("Remaining", format!("{remaining:.0}")));
    }
    lines
}

pub(super) fn parse_subscription(body: &serde_json::Value) -> (Vec<MetricLine>, Option<String>) {
    let plan = body
        .pointer("/subscription/displayName")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let rate = body.get("rateLimit").unwrap_or(body);
    let mut lines = Vec::new();
    if let (Some(used), Some(limit)) = (field(rate, "weeklyUsed"), field(rate, "weeklyLimit")) {
        if let Some(pct) = json_api::used_percent(used, limit) {
            lines.push(MetricLine::percent("Weekly", pct, None));
        }
    }
    (lines, plan)
}

impl Provider for Codebuff {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        token().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(token) = token() else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Codebuff token found. Set CODEBUFF_API_KEY or run `codebuff login`.",
            );
        };
        let (key, session) = match token {
            Token::ApiKey(key) => (key, false),
            Token::Session(key) => (key, true),
        };
        let base = match env_any(&["CODEBUFF_API_URL"]) {
            Some(raw) => match json_api::allowed_base(&raw, false) {
                Ok(base) => base,
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            },
            None => "https://www.codebuff.com".into(),
        };
        match json_api::post_bearer(&json_api::join_url(&base, "api/v1/usage"), &key, "{}") {
            Ok(data) => {
                let mut lines = parse_usage(&data);
                let mut plan = None;
                if session {
                    if let Ok(sub) = json_api::get_bearer(
                        &json_api::join_url(&base, "api/user/subscription"),
                        &key,
                    ) {
                        let (extra, sub_plan) = parse_subscription(&sub);
                        lines.extend(extra);
                        plan = sub_plan;
                    }
                }
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "Codebuff usage response was empty.")
                } else {
                    ProviderOutput::new(ID, NAME, lines).with_plan(plan)
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
    fn parses_credits_and_weekly() {
        let usage = parse_usage(&serde_json::json!({
            "usage": 12,
            "quota": 100,
            "remainingBalance": 88,
            "next_quota_reset": "2026-10-01T00:00:00Z"
        }));
        let (weekly, plan) = parse_subscription(&serde_json::json!({
            "subscription": {"displayName": "Pro"},
            "rateLimit": {"weeklyUsed": 3, "weeklyLimit": 20}
        }));
        assert_eq!(usage.len(), 2);
        assert_eq!(plan.as_deref(), Some("Pro"));
        assert_eq!(weekly.len(), 1);
    }
}
