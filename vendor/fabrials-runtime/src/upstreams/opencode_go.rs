use crate::provider::{Provider, Upstream};
use crate::routes::parse_fabric_path;
use fabrials_core::hop::HopClass;
use fabrials_model::UsageRecord;

pub const USER_AGENT: &str = concat!(
    "fabrials-runtime/",
    env!("CARGO_PKG_VERSION"),
    " (+https://fabrials.com)"
);

#[derive(Default)]
pub struct OpenCodeGoAdapter {
    pub base: Option<String>,
}

impl Provider for OpenCodeGoAdapter {
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

    fn inject(&self, token: &str) -> Vec<(String, String)> {
        vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("User-Agent".into(), USER_AGENT.into()),
        ]
    }

    fn parse_usage(&self, response_body: &[u8]) -> Option<UsageRecord> {
        let text = String::from_utf8_lossy(response_body);
        fabrials_metrics::usage_from_response_body(&text)
            .map(|p| p.into_record(now_ms(), None, None, Some("opencode-go".into())))
    }

    fn classify(&self, hop: &Upstream, upgrade: bool) -> HopClass {
        HopClass::openai_compat(&hop.path, upgrade)
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
