use fabrials_fabric::provider::{
    BodyShaper, CredentialInjector, Router, Translator, Upstream, UsageExtractor,
};
use fabrials_types::hop::{HopClass, HopKind, Transport};
use fabrials_types::HopRecord;
use serde_json::Value;
pub mod translation;
pub mod websocket;

#[derive(Default)]
pub struct CodexAdapter {
    pub base: Option<String>,
}
impl Router for CodexAdapter {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn resolve(&self, raw: &str) -> Option<Upstream> {
        let route = crate::routes::parse_fabric_path(raw);
        if route.route != "codex" {
            return None;
        }
        let (path_only, query) = match route.path.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (route.path.as_str(), None),
        };
        let path = match path_only {
            "/v1/responses" => "/backend-api/codex/responses".to_string(),
            "/v1/chat/completions" => "/backend-api/codex/chat/completions".to_string(),
            "/v1/models" => {
                let mut path = "/backend-api/codex/models".to_string();
                if let Some(version) = query.and_then(translation::client_version_from_query) {
                    path.push_str("?client_version=");
                    path.push_str(version);
                }
                path
            }
            _ => return None,
        };
        Some(Upstream {
            base: self.base.clone().unwrap_or("https://chatgpt.com".into()),
            path,
            account_alias: route.account_alias,
            route: "codex",
        })
    }

    fn classify(&self, hop: &Upstream, upgrade: bool) -> HopClass {
        let path = codex_api_path(&hop.path);
        HopClass {
            kind: if path.ends_with("/models") {
                HopKind::Models
            } else {
                HopKind::Chat
            },
            transport: if upgrade && path == "/backend-api/codex/responses" {
                Transport::WebSocket
            } else {
                Transport::Http
            },
        }
    }

    fn allows_request(&self, method: &str, hop: &Upstream, upgrade: bool) -> bool {
        match codex_api_path(&hop.path) {
            "/backend-api/codex/models" => method == "GET" && !upgrade,
            "/backend-api/codex/responses" => {
                (method == "POST" && !upgrade) || (method == "GET" && upgrade)
            }
            "/backend-api/codex/chat/completions" => method == "POST" && !upgrade,
            _ => false,
        }
    }
}

impl CredentialInjector for CodexAdapter {
    fn inject(&self, token: &str) -> Vec<(String, String)> {
        vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("originator".into(), "codex_cli_rs".into()),
            ("User-Agent".into(), "codex_cli_rs".into()),
        ]
    }

    fn credential_headers(
        &self,
        token: &str,
        _hop: &Upstream,
        secret: Option<&Value>,
    ) -> Result<Vec<(String, String)>, String> {
        let id = secret
            .and_then(|s| s["account_id"].as_str())
            .filter(|s| !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| b.is_ascii_graphic()))
            .ok_or("Codex account identity unavailable")?;
        let mut headers = self.inject(token);
        headers.push(("ChatGPT-Account-Id".into(), id.into()));
        Ok(headers)
    }
}

impl UsageExtractor for CodexAdapter {
    fn parse_usage(&self, body: &[u8]) -> Option<HopRecord> {
        fabrials_providers::sse::usage_from_response_body(&String::from_utf8_lossy(body))
            .map(|p| p.into_record(translation::now_ms(), None, None, Some("codex".into())))
    }
}

impl Translator for CodexAdapter {
    fn websocket_hop(
        &self,
        hop: fabrials_fabric::forward::WebSocketHop<'_>,
    ) -> Option<Result<(), String>> {
        Some(websocket::forward(self, hop))
    }

    fn translated_hop(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        token: &str,
        secret: Option<&Value>,
        client: &mut dyn std::io::Write,
    ) -> Option<Result<Option<HopRecord>, String>> {
        Some(translation::forward(
            self, method, path, body, token, secret, client,
        ))
    }
}

impl BodyShaper for CodexAdapter {}

const CLIENT_ROUTING_HEADERS: &[&str] = &[
    "session-id",
    "session_id",
    "thread-id",
    "x-client-request-id",
    "x-codex-window-id",
    "x-codex-turn-state",
    "x-codex-turn-metadata",
    "x-codex-beta-features",
    "x-codex-parent-thread-id",
    "x-openai-subagent",
];

pub(crate) const UPSTREAM_REPLY_HEADERS: &[&str] =
    &["x-codex-turn-state", "x-reasoning-included", "openai-model"];

const MAX_ROUTING_HEADER_BYTES: usize = 4096;

pub(crate) fn routing_header_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ROUTING_HEADER_BYTES
        && value.bytes().all(|b| (0x20..0x7f).contains(&b))
}

pub(crate) fn client_routing_headers(
    headers: &std::collections::HashMap<String, String>,
) -> Vec<(&'static str, &str)> {
    CLIENT_ROUTING_HEADERS
        .iter()
        .filter_map(|name| {
            headers
                .get(*name)
                .map(String::as_str)
                .filter(|value| routing_header_value(value))
                .map(|value| (*name, value))
        })
        .collect()
}

fn codex_api_path(path: &str) -> &str {
    path.split_once('?').map(|(path, _)| path).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routes_are_scoped_and_credentials_require_a_host_account_identity() {
        let adapter = CodexAdapter::default();
        let hop = adapter.resolve("/acct/work/codex/v1/responses").unwrap();
        assert_eq!(hop.account_alias.as_deref(), Some("work"));
        assert_eq!(hop.path, "/backend-api/codex/responses");
        assert!(adapter.allows_request("GET", &hop, true));
        assert_eq!(adapter.classify(&hop, true).transport, Transport::WebSocket);
        assert_eq!(adapter.classify(&hop, false).transport, Transport::Http);
        let catalog = adapter.resolve("/codex/v1/models").unwrap();
        assert_eq!(catalog.path, "/backend-api/codex/models");
        assert!(adapter.allows_request("GET", &catalog, false));
        assert_eq!(adapter.classify(&catalog, false).kind, HopKind::Models);
        assert_eq!(adapter.classify(&catalog, true).transport, Transport::Http);
        let native = adapter
            .resolve("/acct/personal/codex/v1/models?client_version=0.155.1")
            .unwrap();
        assert_eq!(
            native.path,
            "/backend-api/codex/models?client_version=0.155.1"
        );
        assert!(adapter.allows_request("GET", &native, false));
        assert!(!adapter.allows_request("POST", &native, false));
        assert_eq!(adapter.classify(&native, false).kind, HopKind::Models);
        let ignored = adapter
            .resolve("/codex/v1/models?client_version=0.155.1%0aX")
            .unwrap();
        assert_eq!(ignored.path, "/backend-api/codex/models");
        for path in [
            "/codex@evil/v1/responses",
            "/codex/v1/files",
            "/openai/v1/responses",
        ] {
            assert!(adapter.resolve(path).is_none());
        }
        assert!(adapter.credential_headers("token", &hop, None).is_err());
        assert!(adapter
            .credential_headers(
                "token",
                &hop,
                Some(&serde_json::json!({"account_id":"bad\r\nheader"}))
            )
            .is_err());
        let headers = adapter
            .credential_headers(
                "token",
                &hop,
                Some(&serde_json::json!({"account_id":"host-account"})),
            )
            .unwrap();
        assert!(headers.contains(&("ChatGPT-Account-Id".into(), "host-account".into())));
    }

    #[test]
    fn only_bounded_codex_routing_headers_are_forwarded() {
        let client: std::collections::HashMap<String, String> = [
            ("session-id", "s-1"),
            ("x-codex-turn-state", "t-1"),
            ("x-client-request-id", ""),
            ("x-codex-window-id", "bad\u{7f}value"),
            ("authorization", "Bearer client"),
            ("cookie", "c=1"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let mut forwarded = client_routing_headers(&client);
        forwarded.sort();
        assert_eq!(
            forwarded,
            vec![("session-id", "s-1"), ("x-codex-turn-state", "t-1")]
        );
        let long = "a".repeat(MAX_ROUTING_HEADER_BYTES + 1);
        assert!(!routing_header_value(&long));
    }
}
