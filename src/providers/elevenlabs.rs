//! ElevenLabs subscription usage.
//!
//! Auth: `xi-api-key` from `ELEVENLABS_API_KEY` or `XI_API_KEY`.
//! Usage: `GET https://api.elevenlabs.io/v1/user/subscription`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};
use crate::util;

const ID: &str = "elevenlabs";
const NAME: &str = "ElevenLabs";

pub struct ElevenLabs;

pub(super) fn parse_subscription(data: &serde_json::Value) -> (Vec<MetricLine>, Option<String>) {
    let used = field(data, "character_count");
    let limit = field(data, "character_limit");
    let mut lines = Vec::new();
    if let (Some(used), Some(limit)) = (used, limit)
        && let Some(pct) = json_api::used_percent(used, limit)
    {
        let resets = data
            .get("next_character_count_reset_unix")
            .and_then(util::to_iso);
        lines.push(MetricLine::percent("Credits", pct, resets));
    }
    if let (Some(used), Some(limit)) = (field(data, "voice_slots_used"), field(data, "voice_limit"))
        && limit > 0.0
    {
        lines.push(json_api::count_line(
            "Voice slots",
            used,
            limit,
            "voices",
            None,
        ));
    }
    let plan = json_api::text_field(data, "tier").map(|tier| {
        tier.replace('_', " ")
            .split_whitespace()
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    });
    (lines, plan)
}

impl Provider for ElevenLabs {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["ELEVENLABS_API_KEY", "XI_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["ELEVENLABS_API_KEY", "XI_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No ElevenLabs API key found. Set ELEVENLABS_API_KEY.",
            );
        };
        let base = env_any(&["ELEVENLABS_API_URL"])
            .unwrap_or_else(|| "https://api.elevenlabs.io/v1".into());
        let base = match json_api::allowed_base(&base, false) {
            Ok(b) => b,
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        let url = if base.ends_with("/v1") {
            json_api::join_url(&base, "user/subscription")
        } else {
            json_api::join_url(&base, "v1/user/subscription")
        };
        match json_api::get_headers(&url, &[("xi-api-key", &key)]) {
            Ok(data) => {
                let (lines, plan) = parse_subscription(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "ElevenLabs subscription response was empty.")
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
    fn parses_characters_and_voices() {
        let (lines, plan) = parse_subscription(&serde_json::json!({
            "tier": "creator",
            "character_count": 2500,
            "character_limit": 10000,
            "voice_slots_used": 2,
            "voice_limit": 10,
            "next_character_count_reset_unix": 1700000000
        }));
        assert_eq!(plan.as_deref(), Some("Creator"));
        assert!(lines.len() >= 2);
    }
}
