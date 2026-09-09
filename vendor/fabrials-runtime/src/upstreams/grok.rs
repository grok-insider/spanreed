use crate::provider::{Provider, Upstream};
use crate::routes::parse_fabric_path;
use crate::routes::UPSTREAM_XAI_API;
use fabrials_core::hop::HopClass;
use fabrials_model::UsageRecord;
#[derive(Default)]
pub struct GrokAdapter {
    /// Override CLI upstream (tests / fake).
    pub cli_base: Option<String>,
    /// Override `api.x.ai` (tests / fake).
    pub xai_base: Option<String>,
}

impl Provider for GrokAdapter {
    fn id(&self) -> &'static str {
        "grok"
    }

    fn resolve(&self, raw_path: &str) -> Option<Upstream> {
        let r = parse_fabric_path(raw_path);
        if r.route != "grok" && r.route != "xai" {
            return None;
        }
        let base = if r.upstream == UPSTREAM_XAI_API {
            self.xai_base
                .clone()
                .unwrap_or_else(|| r.upstream.to_string())
        } else {
            self.cli_base
                .clone()
                .unwrap_or_else(|| r.upstream.to_string())
        };
        Some(Upstream {
            base: base.trim_end_matches('/').to_string(),
            path: map_openai_audio_alias(&r.path),
            account_alias: r.account_alias,
            route: r.route,
        })
    }

    fn inject(&self, token: &str) -> Vec<(String, String)> {
        inject_cli_headers(token)
    }

    fn inject_for(&self, token: &str, hop: &Upstream) -> Vec<(String, String)> {
        // If it's a standard API key (e.g. xai-...) or calling xai API directly, send standard bearer
        if token.starts_with("xai-") || self.uses_xai_api(hop) {
            inject_xai_headers(token)
        } else {
            inject_cli_headers(token)
        }
    }

    fn parse_usage(&self, response_body: &[u8]) -> Option<UsageRecord> {
        let text = String::from_utf8_lossy(response_body);
        fabrials_metrics::usage_from_response_body(&text)
            .map(|p| p.into_record(now_ms(), None, None, Some("grok".into())))
    }

    fn classify(&self, hop: &Upstream, upgrade: bool) -> HopClass {
        HopClass::openai_compat(&hop.path, upgrade)
    }

    fn allows_request(&self, method: &str, hop: &Upstream, upgrade: bool) -> bool {
        fabrials_core::hop::core_request_allowed(method, &hop.path)
            || fabrials_core::hop::media_request_allowed(method, &hop.path, upgrade)
    }
}

impl GrokAdapter {
    fn uses_xai_api(&self, hop: &Upstream) -> bool {
        hop.route == "xai"
            || hop.base == UPSTREAM_XAI_API
            || self
                .xai_base
                .as_deref()
                .is_some_and(|b| hop.base == b.trim_end_matches('/'))
    }
}

pub fn inject_cli_headers(token: &str) -> Vec<(String, String)> {
    let mut h = fabrials_oauth_grok::inject_headers(token);
    h.extend(client_identity_headers());
    h
}

pub fn inject_xai_headers(token: &str) -> Vec<(String, String)> {
    let mut h = vec![("Authorization".into(), format!("Bearer {token}"))];
    h.extend(client_identity_headers());
    h
}

fn client_identity_headers() -> Vec<(String, String)> {
    vec![
        (
            "x-grok-client-version".into(),
            std::env::var("AI_RELAY_GROK_CLIENT_VERSION").unwrap_or_else(|_| "0.2.84".into()),
        ),
        (
            "x-grok-client-identifier".into(),
            std::env::var("AI_RELAY_GROK_CLIENT_IDENTIFIER").unwrap_or_else(|_| "grok-cli".into()),
        ),
    ]
}

fn map_openai_audio_alias(path: &str) -> String {
    let (p, q) = match path.split_once('?') {
        Some((a, b)) => (a, Some(b)),
        None => (path, None),
    };
    let mapped = match p.trim_end_matches('/') {
        "/v1/audio/speech" => "/v1/tts",
        "/v1/audio/transcriptions" => "/v1/stt",
        "/v1/audio/voices" => "/v1/tts/voices",
        _ => return path.to_string(),
    };
    match q {
        Some(q) => format!("{mapped}?{q}"),
        None => mapped.to_string(),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
