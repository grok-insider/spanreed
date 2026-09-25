//! Kimi Code prefix. The coding plan is Anthropic-compatible, so the messages
//! surface travels with `Authorization: Bearer` and the Anthropic version header.
//! Header values mirror the `fabrials-providers` Kimi credential headers, and
//! ai-relay asserts both agree.

use crate::catalog::by_id;
use crate::provider::{Provider, Upstream};
use crate::routes::parse_fabric_path;
use fabrials_core::hop::HopClass;
use fabrials_model::UsageRecord;

pub const ID: &str = "kimi";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Default)]
pub struct KimiAdapter;

impl Provider for KimiAdapter {
    fn id(&self) -> &'static str {
        ID
    }

    fn resolve(&self, raw_path: &str) -> Option<Upstream> {
        let routed = parse_fabric_path(raw_path);
        if routed.route != ID {
            return None;
        }
        let spec = by_id(ID)?;
        Some(Upstream {
            base: spec.upstream.trim_end_matches('/').to_string(),
            path: routed.path,
            account_alias: routed.account_alias,
            route: spec.id,
        })
    }

    fn inject(&self, token: &str) -> Vec<(String, String)> {
        inject_headers(token)
    }

    fn parse_usage(&self, response_body: &[u8]) -> Option<UsageRecord> {
        let text = String::from_utf8_lossy(response_body);
        fabrials_metrics::usage_from_messages_body(&text)
            .or_else(|| fabrials_metrics::usage_from_response_body(&text))
            .map(|parsed| parsed.into_record(now_ms(), None, None, Some(ID.into())))
    }

    fn classify(&self, hop: &Upstream, upgrade: bool) -> HopClass {
        HopClass::openai_compat(&hop.path, upgrade)
    }

    fn allows_request(&self, method: &str, hop: &Upstream, upgrade: bool) -> bool {
        let _ = upgrade;
        by_id(ID).is_some_and(|spec| spec.allows(method, &hop.path))
    }
}

pub fn inject_headers(token: &str) -> Vec<(String, String)> {
    vec![
        ("Authorization".into(), format!("Bearer {}", token.trim())),
        ("anthropic-version".into(), ANTHROPIC_VERSION.into()),
    ]
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;

    #[test]
    fn catalogue_entry_pins_the_anthropic_surface() {
        let adapter = KimiAdapter;
        let hop = adapter.resolve("/kimi/v1/messages").unwrap();
        assert_eq!(hop.base, "https://api.kimi.com/coding");
        assert_eq!(hop.path, "/v1/messages");
        assert!(adapter.allows_request("POST", &hop, false));
        assert!(!adapter.allows_request("GET", &hop, false));
        let pinned = adapter.resolve("/acct/work/kimi/v1/messages").unwrap();
        assert_eq!(pinned.account_alias.as_deref(), Some("work"));
        let models = adapter.resolve("/kimi/v1/models").unwrap();
        assert!(adapter.allows_request("GET", &models, false));
        let chat = adapter.resolve("/kimi/v1/chat/completions").unwrap();
        assert!(!adapter.allows_request("POST", &chat, false));
        assert!(adapter.resolve("/other/v1/messages").is_none());
    }

    #[test]
    fn credentials_travel_as_bearer_with_the_anthropic_version() {
        let adapter = KimiAdapter;
        let hop = adapter.resolve("/kimi/v1/messages").unwrap();
        let headers = adapter
            .credential_headers("kimi-token", &hop, None)
            .unwrap();
        assert!(headers
            .iter()
            .any(|(name, value)| name == "Authorization" && value == "Bearer kimi-token"));
        assert!(headers
            .iter()
            .any(|(name, value)| name == "anthropic-version" && value == ANTHROPIC_VERSION));
        assert!(!headers.iter().any(|(name, _)| name == "x-api-key"));
    }
}
