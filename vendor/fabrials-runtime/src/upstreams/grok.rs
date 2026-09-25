use crate::provider::{Provider, Upstream};
use crate::routes::parse_fabric_path;
use crate::routes::{UPSTREAM_GROK_CLI, UPSTREAM_XAI_API};
use fabrials_core::hop::HopClass;
use fabrials_model::UsageRecord;
/// Client identity headers sent to Grok upstreams. The host chooses the values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrokClientIdentity {
    pub version: String,
    pub identifier: String,
}

impl Default for GrokClientIdentity {
    fn default() -> Self {
        Self {
            version: "0.2.84".into(),
            identifier: "grok-cli".into(),
        }
    }
}

impl GrokClientIdentity {
    pub fn headers(&self) -> Vec<(String, String)> {
        vec![
            ("x-grok-client-version".into(), self.version.clone()),
            ("x-grok-client-identifier".into(), self.identifier.clone()),
        ]
    }

    pub fn cli_headers(&self, token: &str) -> Vec<(String, String)> {
        let mut h = fabrials_oauth_grok::inject_headers(token);
        h.extend(self.headers());
        h
    }

    pub fn xai_headers(&self, token: &str) -> Vec<(String, String)> {
        let mut h = vec![("Authorization".into(), format!("Bearer {token}"))];
        h.extend(self.headers());
        h
    }
}

#[derive(Default)]
pub struct GrokAdapter {
    /// Override CLI upstream (tests / fake).
    pub cli_base: Option<String>,
    /// Override `api.x.ai` (tests / fake).
    pub xai_base: Option<String>,
    pub identity: GrokClientIdentity,
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
        self.identity.cli_headers(token)
    }

    fn inject_for(&self, token: &str, hop: &Upstream) -> Vec<(String, String)> {
        // If it's a standard API key (e.g. xai-...) or calling xai API directly, send standard bearer
        if token.starts_with("xai-") || self.uses_xai_api(hop) {
            self.identity.xai_headers(token)
        } else {
            self.identity.cli_headers(token)
        }
    }

    fn bind_credential(&self, hop: &mut Upstream, token: &str) {
        if hop.route != "xai" || token.starts_with("xai-") || !is_cli_chat_path(&hop.path) {
            return;
        }
        hop.base = self.cli_origin();
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
            || fabrials_core::hop::cli_aux_request_allowed(method, &hop.path)
    }
}

impl GrokAdapter {
    fn cli_origin(&self) -> String {
        self.cli_base
            .as_deref()
            .unwrap_or(UPSTREAM_GROK_CLI)
            .trim_end_matches('/')
            .to_string()
    }

    fn uses_xai_api(&self, hop: &Upstream) -> bool {
        hop.base == UPSTREAM_XAI_API
            || self
                .xai_base
                .as_deref()
                .is_some_and(|b| hop.base == b.trim_end_matches('/'))
    }
}

fn is_cli_chat_path(path: &str) -> bool {
    matches!(
        path.split('?').next().unwrap_or(path).trim_end_matches('/'),
        "/v1/responses" | "/v1/chat/completions"
    )
}

/// Headers with the default client identity.
pub fn inject_cli_headers(token: &str) -> Vec<(String, String)> {
    GrokClientIdentity::default().cli_headers(token)
}

pub fn inject_xai_headers(token: &str) -> Vec<(String, String)> {
    GrokClientIdentity::default().xai_headers(token)
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

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn grok_adapter_allows_cli_aux_and_denies_account_apis() {
        let g = GrokAdapter::default();
        let feedback = g.resolve("/v1/feedback").expect("feedback route");
        assert!(g.allows_request("POST", &feedback, false));
        let user = g.resolve("/v1/user").expect("user route");
        assert!(g.allows_request("GET", &user, false));
        let files = g.resolve("/v1/files").expect("files still grok-routed");
        assert!(!g.allows_request("GET", &files, false));
        let deploy = g.resolve("/v1/deployment/config").expect("deploy");
        assert!(!g.allows_request("GET", &deploy, false));
    }

    fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn assert_cli_inject(headers: &[(String, String)], token: &str) {
        let expected = format!("Bearer {token}");
        assert_eq!(header(headers, "Authorization"), Some(expected.as_str()));
        assert_eq!(
            header(headers, "x-xai-token-auth"),
            Some(fabrials_oauth_grok::TOKEN_AUTH)
        );
    }

    fn assert_xai_inject(headers: &[(String, String)], token: &str) {
        let expected = format!("Bearer {token}");
        assert_eq!(header(headers, "Authorization"), Some(expected.as_str()));
        assert_eq!(header(headers, "x-xai-token-auth"), None);
    }

    fn bind_inject(g: &GrokAdapter, path: &str, token: &str) -> (String, Vec<(String, String)>) {
        let mut hop = g.resolve(path).unwrap_or_else(|| panic!("{path}"));
        g.bind_credential(&mut hop, token);
        let headers = g.inject_for(token, &hop);
        (hop.base, headers)
    }

    #[test]
    fn xai_chat_cli_token_binds_to_cli_proxy() {
        let g = GrokAdapter::default();
        let cli = UPSTREAM_GROK_CLI.trim_end_matches('/');
        for path in [
            "/xai/v1/responses",
            "/xai/v1/chat/completions",
            "/xai/v1/responses/",
            "/xai/v1/responses?stream=true",
        ] {
            let (base, headers) = bind_inject(&g, path, "cli-oauth-token");
            assert_eq!(base, cli, "{path}");
            assert_cli_inject(&headers, "cli-oauth-token");
        }
        for token in ["", "eyJhbGciOiJIUzI1NiJ9.e30.d"] {
            let (base, headers) = bind_inject(&g, "/xai/v1/responses", token);
            assert_eq!(base, cli, "{token:?}");
            assert_cli_inject(&headers, token);
        }
        for path in ["/xai/v1/responses", "/xai/v1/chat/completions"] {
            let (base, headers) = bind_inject(&g, path, "xai-consolekey");
            assert_eq!(base, UPSTREAM_XAI_API, "{path}");
            assert_xai_inject(&headers, "xai-consolekey");
        }
        for path in [
            "/xai/v1/models",
            "/xai/v1/videos/generations",
            "/xai/v1/images/generations",
        ] {
            let (base, headers) = bind_inject(&g, path, "cli-oauth-token");
            assert_eq!(base, UPSTREAM_XAI_API, "{path}");
            assert_xai_inject(&headers, "cli-oauth-token");
        }
        let overridden = GrokAdapter {
            cli_base: Some("http://127.0.0.1:9".into()),
            ..Default::default()
        };
        let (base, headers) = bind_inject(&overridden, "/xai/v1/responses", "cli-oauth-token");
        assert_eq!(base, "http://127.0.0.1:9");
        assert_cli_inject(&headers, "cli-oauth-token");
    }
}
