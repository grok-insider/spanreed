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

#[derive(Default)]
pub struct OpenCodeGoAdapter {
    pub base: Option<String>,
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
        let base = self.base.clone().unwrap_or_else(|| r.upstream.to_string());
        Some(Upstream {
            base: base.trim_end_matches('/').to_string(),
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
