//! Claude (Anthropic) prefix with two credential kinds.
//!
//! An `sk-ant-oat…` subscription session travels as `Authorization: Bearer` with the
//! OAuth beta header and `x-app: cli`; an API key keeps `x-api-key`. Header values
//! mirror the `fabrials-providers` Claude credential headers, and ai-relay asserts
//! both agree.

use crate::catalog::by_id;
use crate::provider::{Provider, Upstream};
use crate::routes::parse_fabric_path;
use fabrials_core::hop::HopClass;
use fabrials_model::UsageRecord;
use serde_json::Value;

pub const ID: &str = "claude";
pub const OAUTH_TOKEN_PREFIX: &str = "sk-ant-oat";
pub const OAUTH_BETA: &str = "oauth-2025-04-20";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Anthropic serves a subscription session (`sk-ant-oat…`) only when the
/// request's `system` opens with the official client's identity text; anything
/// else answers an opaque `429 rate_limit_error`. Measured on 2026-09-25:
/// same body, same session, 200 with this text first and 429 without it.
pub const IDENTITY_SYSTEM_TEXT: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

#[derive(Default)]
pub struct ClaudeAdapter {
    pub base: Option<String>,
}

impl Provider for ClaudeAdapter {
    fn id(&self) -> &'static str {
        ID
    }

    fn resolve(&self, raw_path: &str) -> Option<Upstream> {
        let routed = parse_fabric_path(raw_path);
        if routed.route != ID {
            return None;
        }
        let spec = by_id(ID)?;
        let base = self
            .base
            .clone()
            .unwrap_or_else(|| spec.upstream.to_string());
        Some(Upstream {
            base: base.trim_end_matches('/').to_string(),
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

pub fn is_oauth(token: &str) -> bool {
    token.trim().starts_with(OAUTH_TOKEN_PREFIX)
}

pub fn inject_headers(token: &str) -> Vec<(String, String)> {
    let token = token.trim();
    if is_oauth(token) {
        vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("anthropic-version".into(), ANTHROPIC_VERSION.into()),
            ("anthropic-beta".into(), OAUTH_BETA.into()),
            ("x-app".into(), "cli".into()),
        ]
    } else {
        vec![
            ("x-api-key".into(), token.to_string()),
            ("anthropic-version".into(), ANTHROPIC_VERSION.into()),
        ]
    }
}

/// Ensure `system` opens with the identity block. Returns false when the
/// request needs no change: it already opens with the identity text, or it is
/// not a Messages request (`messages` array absent) or not a JSON object.
pub fn ensure_identity_system(request: &mut Value) -> bool {
    let Some(object) = request.as_object_mut() else {
        return false;
    };
    if !object.get("messages").is_some_and(Value::is_array) {
        return false;
    }
    match object.get_mut("system") {
        None => {
            object.insert("system".into(), Value::Array(vec![identity_block()]));
            true
        }
        Some(Value::Array(blocks)) => {
            if blocks.first().is_some_and(is_identity_block) {
                return false;
            }
            blocks.insert(0, identity_block());
            true
        }
        Some(Value::String(text)) => {
            if text.trim().starts_with(IDENTITY_SYSTEM_TEXT) {
                return false;
            }
            let existing = Value::String(std::mem::take(text));
            object.insert(
                "system".into(),
                Value::Array(vec![identity_block(), existing]),
            );
            true
        }
        // An unexpected `system` shape is left alone: Anthropic rejects the
        // body on its own terms, and rewriting it would be a guess.
        Some(_) => false,
    }
}

/// Parse, shape, serialize. `None` when the body is unchanged or is not a JSON
/// object, so callers forward the original bytes untouched.
pub fn shape_identity_system(body: &[u8]) -> Option<Vec<u8>> {
    let mut request: Value = serde_json::from_slice(body).ok()?;
    if !ensure_identity_system(&mut request) {
        return None;
    }
    serde_json::to_vec(&request).ok()
}

fn identity_block() -> Value {
    serde_json::json!({ "type": "text", "text": IDENTITY_SYSTEM_TEXT })
}

fn is_identity_block(block: &Value) -> bool {
    match block {
        Value::String(text) => text.trim().starts_with(IDENTITY_SYSTEM_TEXT),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.trim().starts_with(IDENTITY_SYSTEM_TEXT)),
        _ => false,
    }
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

    fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn catalogue_entry_pins_the_origin_and_messages_surface() {
        let adapter = ClaudeAdapter::default();
        let hop = adapter.resolve("/claude/v1/messages").unwrap();
        assert_eq!(hop.base, "https://api.anthropic.com");
        assert_eq!(hop.path, "/v1/messages");
        assert!(adapter.allows_request("POST", &hop, false));
        assert!(!adapter.allows_request("GET", &hop, false));
        let pinned = adapter.resolve("/acct/work/claude/v1/messages").unwrap();
        assert_eq!(pinned.account_alias.as_deref(), Some("work"));
        let models = adapter.resolve("/claude/v1/models").unwrap();
        assert!(adapter.allows_request("GET", &models, false));
        let chat = adapter.resolve("/claude/v1/chat/completions").unwrap();
        assert!(!adapter.allows_request("POST", &chat, false));
        assert!(adapter.resolve("/other/v1/messages").is_none());
    }

    #[test]
    fn a_fixture_base_replaces_the_pinned_origin() {
        let adapter = ClaudeAdapter {
            base: Some("http://127.0.0.1:9".into()),
        };
        let hop = adapter.resolve("/claude/v1/messages").unwrap();
        assert_eq!(hop.base, "http://127.0.0.1:9");
        assert_eq!(hop.path, "/v1/messages");
    }

    #[test]
    fn generic_system_gets_the_identity_block_in_front() {
        let body = br#"{"model":"claude-opus-5-5","system":[{"type":"text","text":"Be terse."}],"messages":[{"role":"user","content":"hi"}]}"#;
        let shaped = shape_identity_system(body).expect("shaped");
        let request: Value = serde_json::from_slice(&shaped).unwrap();
        let system = request["system"].as_array().unwrap();
        assert_eq!(system.len(), 2);
        assert_eq!(system[0]["text"].as_str(), Some(IDENTITY_SYSTEM_TEXT));
        assert_eq!(system[1]["text"].as_str(), Some("Be terse."));
        assert_eq!(request["model"].as_str(), Some("claude-opus-5-5"));
        assert_eq!(request["messages"][0]["content"].as_str(), Some("hi"));
    }

    #[test]
    fn a_missing_system_gains_the_identity_block() {
        let body = br#"{"model":"claude-opus-5-5","messages":[{"role":"user","content":"hi"}]}"#;
        let shaped = shape_identity_system(body).expect("shaped");
        let request: Value = serde_json::from_slice(&shaped).unwrap();
        let system = request["system"].as_array().unwrap();
        assert_eq!(system.len(), 1);
        assert_eq!(system[0]["text"].as_str(), Some(IDENTITY_SYSTEM_TEXT));
    }

    #[test]
    fn a_plain_string_system_keeps_its_text_after_the_identity() {
        let body = br#"{"system":"Be terse.","messages":[]}"#;
        let shaped = shape_identity_system(body).expect("shaped");
        let request: Value = serde_json::from_slice(&shaped).unwrap();
        let system = request["system"].as_array().unwrap();
        assert_eq!(system[0]["text"].as_str(), Some(IDENTITY_SYSTEM_TEXT));
        assert_eq!(system[1].as_str(), Some("Be terse."));
    }

    #[test]
    fn an_identity_that_already_leads_is_left_alone() {
        let leading_block = format!(
            r#"{{"system":[{{"type":"text","text":"{IDENTITY_SYSTEM_TEXT}"}},{{"type":"text","text":"Be terse."}}],"messages":[]}}"#
        );
        assert!(shape_identity_system(leading_block.as_bytes()).is_none());
        let long_block = format!(
            r#"{{"system":[{{"type":"text","text":"{IDENTITY_SYSTEM_TEXT}\nBe terse."}}],"messages":[]}}"#
        );
        assert!(shape_identity_system(long_block.as_bytes()).is_none());
        let plain = format!(r#"{{"system":"{IDENTITY_SYSTEM_TEXT}","messages":[]}}"#);
        assert!(shape_identity_system(plain.as_bytes()).is_none());
        let bare = format!(r#"{{"system":["{IDENTITY_SYSTEM_TEXT}"],"messages":[]}}"#);
        assert!(shape_identity_system(bare.as_bytes()).is_none());
    }

    #[test]
    fn malformed_and_non_messages_bodies_pass_through() {
        for body in [
            &b"not json"[..],
            br#"["claude"]"#,
            br#"{"model":"claude-opus-5-5"}"#,
            br#"{"messages":"claude"}"#,
            br#"{"messages":[],"system":42}"#,
            b"",
        ] {
            assert!(
                shape_identity_system(body).is_none(),
                "body {body:?} was rewritten"
            );
        }
    }

    #[test]
    fn an_unidentified_first_block_keeps_the_identity_first_of_all() {
        let body = br#"{"messages":[],"system":["Be terse."]}"#;
        let shaped = shape_identity_system(body).expect("shaped");
        let request: Value = serde_json::from_slice(&shaped).unwrap();
        let system = request["system"].as_array().unwrap();
        assert_eq!(system.len(), 2);
        assert_eq!(system[0]["text"].as_str(), Some(IDENTITY_SYSTEM_TEXT));
        assert_eq!(system[1].as_str(), Some("Be terse."));
    }

    #[test]
    fn subscription_sessions_use_bearer_and_api_keys_keep_the_key_header() {
        let oauth = adapter_headers("sk-ant-oat01-session");
        assert_eq!(
            header(&oauth, "Authorization"),
            Some("Bearer sk-ant-oat01-session")
        );
        assert_eq!(header(&oauth, "anthropic-beta"), Some(OAUTH_BETA));
        assert_eq!(header(&oauth, "x-app"), Some("cli"));
        assert!(header(&oauth, "x-api-key").is_none());

        let key = adapter_headers("sk-ant-api03-key");
        assert_eq!(header(&key, "x-api-key"), Some("sk-ant-api03-key"));
        assert_eq!(header(&key, "anthropic-version"), Some(ANTHROPIC_VERSION));
        assert!(header(&key, "Authorization").is_none());
    }

    fn adapter_headers(token: &str) -> Vec<(String, String)> {
        let adapter = ClaudeAdapter::default();
        let hop = adapter.resolve("/claude/v1/messages").unwrap();
        let mut resolved = hop;
        adapter.bind_credential(&mut resolved, token);
        adapter.credential_headers(token, &resolved, None).unwrap()
    }
}
