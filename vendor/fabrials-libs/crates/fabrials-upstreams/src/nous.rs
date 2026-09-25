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
pub struct NousAdapter {
    pub base: Option<String>,
}

impl Router for NousAdapter {
    fn id(&self) -> &'static str {
        "nous"
    }

    fn resolve(&self, raw_path: &str) -> Option<Upstream> {
        let r = parse_fabric_path(raw_path);
        if r.route != "nous" {
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
}

impl CredentialInjector for NousAdapter {
    fn inject(&self, token: &str) -> Vec<(String, String)> {
        vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("User-Agent".into(), USER_AGENT.into()),
        ]
    }
}

impl UsageExtractor for NousAdapter {
    fn parse_usage(&self, response_body: &[u8]) -> Option<HopRecord> {
        let text = String::from_utf8_lossy(response_body);
        fabrials_providers::sse::usage_from_response_body(&text)
            .map(|p| p.into_record(now_ms(), None, None, Some("nous".into())))
    }
}

impl Translator for NousAdapter {}

impl BodyShaper for NousAdapter {}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
