//! OpenRouter key limits and credit balance.
//!
//! `GET https://openrouter.ai/api/v1/key` and `/api/v1/credits`.

use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};
use crate::util;

const ID: &str = "openrouter";
const NAME: &str = "OpenRouter";

pub struct OpenRouter;

pub(super) fn parse_key(body: &serde_json::Value) -> Vec<MetricLine> {
    let data = body.get("data").unwrap_or(body);
    let mut lines = Vec::new();
    if let (Some(limit), Some(remaining)) = (field(data, "limit"), field(data, "limit_remaining")) {
        let used = (limit - remaining).max(0.0);
        if let Some(pct) = json_api::used_percent(used, limit) {
            let resets = data.get("limit_reset").and_then(util::to_iso);
            lines.push(MetricLine::percent("Key limit", pct, resets));
        }
    } else if let Some(usage) = field(data, "usage") {
        lines.push(json_api::text_line("Key usage", json_api::usd(usage)));
    }
    lines
}

pub(super) fn parse_credits(body: &serde_json::Value) -> Vec<MetricLine> {
    let data = body.get("data").unwrap_or(body);
    let Some(total) = field(data, "total_credits") else {
        return Vec::new();
    };
    let used = field(data, "total_usage").unwrap_or(0.0);
    let mut lines = Vec::new();
    if total > 0.0 {
        lines.push(MetricLine::dollars(
            MetricKind::Quota,
            "Credits",
            used,
            total,
            None,
        ));
    } else {
        lines.push(json_api::text_line(
            "Balance",
            json_api::usd((total - used).max(0.0)),
        ));
    }
    lines
}

impl Provider for OpenRouter {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["OPENROUTER_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["OPENROUTER_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No OpenRouter API key found. Set OPENROUTER_API_KEY.",
            );
        };
        let base = match env_any(&["OPENROUTER_API_BASE"]) {
            Some(raw) => match json_api::allowed_base(&raw, false) {
                Ok(base) => base,
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            },
            None => "https://openrouter.ai/api/v1".into(),
        };
        let mut lines = Vec::new();
        match json_api::get_bearer(&json_api::join_url(&base, "key"), &key) {
            Ok(data) => lines.extend(parse_key(&data)),
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        }
        if let Ok(data) = json_api::get_bearer(&json_api::join_url(&base, "credits"), &key) {
            lines.extend(parse_credits(&data));
        }
        if lines.is_empty() {
            ProviderOutput::error(ID, NAME, "OpenRouter response had no key usage.")
        } else {
            ProviderOutput::new(ID, NAME, lines)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_limit_and_credits() {
        let key = parse_key(&serde_json::json!({
            "data": {"limit": 10.0, "limit_remaining": 4.0, "usage": 6.0, "limit_reset": "monthly"}
        }));
        let credits = parse_credits(&serde_json::json!({
            "data": {"total_credits": 20.0, "total_usage": 5.0}
        }));
        assert_eq!(key.len(), 1);
        assert_eq!(credits.len(), 1);
    }
}
