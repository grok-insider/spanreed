use crate::routes::parse_fabric_path;
use fabrials_fabric::provider::{
    BodyShaper, CredentialInjector, Router, Translator, Upstream, UsageExtractor,
};
use fabrials_types::hop::HopClass;
use fabrials_types::HopRecord;

pub const USER_AGENT: &str = concat!(
    "fabrials-fabric/",
    env!("CARGO_PKG_VERSION"),
    " (+https://fabrials.com)"
);

/// `default()` serves an OpenCode API key through Zen. [`OpenCodeGoAdapter::signed_in`]
/// serves a console sign-in through Go inference, bound to its workspace.
#[derive(Default)]
pub struct OpenCodeGoAdapter {
    pub base: Option<String>,
    pub signed_in: bool,
}

impl OpenCodeGoAdapter {
    pub fn signed_in(base: Option<String>) -> Self {
        Self {
            base,
            signed_in: true,
        }
    }
}

impl Router for OpenCodeGoAdapter {
    fn id(&self) -> &'static str {
        "opencode-go"
    }

    fn resolve(&self, raw_path: &str) -> Option<Upstream> {
        let r = parse_fabric_path(raw_path);
        if r.route != "opencode-go" {
            return None;
        }
        let base = if self.signed_in {
            let root = self
                .base
                .clone()
                .unwrap_or_else(|| fabrials_providers::opencode::GO_INFERENCE.to_string());
            inference_base(&root, &r.path)
        } else {
            let base = self.base.clone().unwrap_or_else(|| r.upstream.to_string());
            base.trim_end_matches('/').to_string()
        };
        Some(Upstream {
            base,
            path: r.path,
            account_alias: r.account_alias,
            route: r.route,
        })
    }

    fn classify(&self, hop: &Upstream, upgrade: bool) -> HopClass {
        HopClass::openai_compat(&hop.path, upgrade)
    }

    /// Go serves frontier models such as `union-alpha` on the Anthropic
    /// Messages protocol only; chat and responses keep the core surface.
    fn allows_request(&self, method: &str, hop: &Upstream, upgrade: bool) -> bool {
        let _ = upgrade;
        fabrials_types::hop::core_request_allowed(method, &hop.path)
            || fabrials_types::hop::messages_request_allowed(method, &hop.path)
    }
}

/// Go inference splits protocols: Messages under `/anthropic`, the rest under
/// `/openai`; `/v1/usage` stays at the root.
pub fn inference_base(root: &str, path: &str) -> String {
    let root = root.trim_end_matches('/');
    let route = path.split('?').next().unwrap_or(path);
    if route == "/v1/usage" || !route.starts_with("/v1/") {
        root.to_string()
    } else if route == "/v1/messages" || route.starts_with("/v1/messages/") {
        format!("{root}/anthropic")
    } else {
        format!("{root}/openai")
    }
}

/// The signed-in workspace, required on every Go request.
fn workspace(secret: Option<&serde_json::Value>) -> Result<&str, String> {
    secret
        .and_then(fabrials_providers::opencode::org_id)
        .ok_or_else(|| "OpenCode workspace unavailable; sign in with OpenCode again".to_string())
}

impl CredentialInjector for OpenCodeGoAdapter {
    fn inject(&self, token: &str) -> Vec<(String, String)> {
        go_headers(token, "fabric")
    }

    fn inject_for(&self, token: &str, hop: &Upstream) -> Vec<(String, String)> {
        let session = hop
            .account_alias
            .as_deref()
            .map(str::trim)
            .filter(|alias| !alias.is_empty())
            .unwrap_or("fabric");
        go_headers(token, session)
    }

    fn credential_headers(
        &self,
        token: &str,
        hop: &Upstream,
        secret: Option<&serde_json::Value>,
    ) -> Result<Vec<(String, String)>, String> {
        let mut headers = self.inject_for(token, hop);
        if !self.signed_in {
            return Ok(headers);
        }
        let org = workspace(secret)?;
        headers.push((fabrials_providers::opencode::ORG_HEADER.into(), org.into()));
        Ok(headers)
    }
}

impl UsageExtractor for OpenCodeGoAdapter {
    fn parse_usage(&self, response_body: &[u8]) -> Option<HopRecord> {
        let text = String::from_utf8_lossy(response_body);
        fabrials_providers::sse::usage_from_messages_body(&text)
            .or_else(|| fabrials_providers::sse::usage_from_response_body(&text))
            .map(|p| p.into_record(now_ms(), None, None, Some("opencode-go".into())))
    }
}

impl Translator for OpenCodeGoAdapter {}

impl BodyShaper for OpenCodeGoAdapter {}

fn go_headers(token: &str, session: &str) -> Vec<(String, String)> {
    vec![
        ("Authorization".into(), format!("Bearer {token}")),
        ("User-Agent".into(), USER_AGENT.into()),
        ("x-opencode-session".into(), format!("fabrials-{session}")),
    ]
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_paths_map_to_the_inference_protocols() {
        let root = "https://opencode.ai/inference/go";
        assert_eq!(
            inference_base(root, "/v1/chat/completions"),
            format!("{root}/openai")
        );
        assert_eq!(
            inference_base(root, "/v1/responses?x=1"),
            format!("{root}/openai")
        );
        assert_eq!(
            inference_base(root, "/v1/messages"),
            format!("{root}/anthropic")
        );
        assert_eq!(inference_base(root, "/v1/usage"), root);
        let hop = OpenCodeGoAdapter::signed_in(None)
            .resolve("/acct/go/opencode-go/v1/messages")
            .unwrap();
        assert_eq!(hop.base, format!("{root}/anthropic"));
        assert_eq!(hop.path, "/v1/messages");
        let key = OpenCodeGoAdapter::default()
            .resolve("/opencode-go/v1/messages")
            .unwrap();
        assert_eq!(key.base, crate::routes::UPSTREAM_OPENCODE_GO);
    }

    #[test]
    fn requests_carry_the_signed_in_workspace() {
        let adapter = OpenCodeGoAdapter::signed_in(None);
        let hop = adapter.resolve("/opencode-go/v1/chat/completions").unwrap();
        let secret = serde_json::json!({"access_token": "t", "org_id": "wrk_a"});
        let headers = adapter
            .credential_headers("t", &hop, Some(&secret))
            .unwrap();
        assert!(headers.contains(&("x-opencode-org-id".into(), "wrk_a".into())));
        assert!(headers.contains(&("Authorization".into(), "Bearer t".into())));
        assert!(adapter
            .credential_headers("t", &hop, Some(&serde_json::json!({"api_key": "k"})))
            .is_err());
        let key = OpenCodeGoAdapter::default();
        let hop = key.resolve("/opencode-go/v1/chat/completions").unwrap();
        let headers = key.credential_headers("k", &hop, None).unwrap();
        assert!(!headers.iter().any(|(name, _)| name == "x-opencode-org-id"));
    }
}
