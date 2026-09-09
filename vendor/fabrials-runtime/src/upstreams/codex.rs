use crate::provider::{Provider, Upstream};
use fabrials_core::hop::HopClass;
use fabrials_model::UsageRecord;
use serde_json::Value;
pub mod translation;
pub mod websocket;

#[derive(Default)]
pub struct CodexAdapter {
    pub base: Option<String>,
}
impl Provider for CodexAdapter {
    fn id(&self) -> &'static str {
        "codex"
    }
    fn resolve(&self, raw: &str) -> Option<Upstream> {
        let route = crate::routes::parse_fabric_path(raw);
        if route.route != "codex" {
            return None;
        }
        let path = match route.path.split('?').next()? {
            "/v1/responses" => "/backend-api/codex/responses",
            "/v1/chat/completions" => "/backend-api/codex/chat/completions",
            "/v1/models" => "/backend-api/codex/models",
            _ => return None,
        };
        Some(Upstream {
            base: self.base.clone().unwrap_or("https://chatgpt.com".into()),
            path: path.into(),
            account_alias: route.account_alias,
            route: "codex",
        })
    }
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
    fn parse_usage(&self, body: &[u8]) -> Option<UsageRecord> {
        fabrials_metrics::usage_from_response_body(&String::from_utf8_lossy(body))
            .map(|p| p.into_record(translation::now_ms(), None, None, Some("codex".into())))
    }
    fn classify(&self, hop: &Upstream, upgrade: bool) -> HopClass {
        HopClass::openai_compat(
            if hop.path.ends_with("/models") {
                "/v1/models"
            } else {
                "/v1/responses"
            },
            upgrade,
        )
    }
    fn allows_request(&self, method: &str, hop: &Upstream, upgrade: bool) -> bool {
        match hop.path.as_str() {
            "/backend-api/codex/models" => method == "GET" && !upgrade,
            "/backend-api/codex/responses" => {
                (method == "POST" && !upgrade) || (method == "GET" && upgrade)
            }
            "/backend-api/codex/chat/completions" => method == "POST" && !upgrade,
            _ => false,
        }
    }
    fn websocket_hop(&self, hop: crate::forward::WebSocketHop<'_>) -> Option<Result<(), String>> {
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
    ) -> Option<Result<Option<UsageRecord>, String>> {
        Some(translation::forward(
            self, method, path, body, token, secret, client,
        ))
    }
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
}
