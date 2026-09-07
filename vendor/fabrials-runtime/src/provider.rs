//! Provider port. New LLMs = new adapter implementing this trait.

use std::io::Write;

use fabrials_model::UsageRecord;
use serde_json::Value;

use fabrials_core::hop::HopClass;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    pub base: String,
    pub path: String,
    pub account_alias: Option<String>,
    pub route: &'static str,
}

/// Outbound LLM/CLI backend. The HTTP use-case never names Grok/Codex/Claude.
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    /// `None` = this provider does not own the path (try the next).
    fn resolve(&self, raw_path: &str) -> Option<Upstream>;
    fn inject(&self, token: &str) -> Vec<(String, String)>;
    /// Header injection for this hop. Default = [`Self::inject`].
    fn inject_for(&self, token: &str, hop: &Upstream) -> Vec<(String, String)> {
        let _ = hop;
        self.inject(token)
    }
    fn parse_usage(&self, response_body: &[u8]) -> Option<UsageRecord>;
    /// What this hop is. Default: chat over HTTP (no voice tunnel).
    fn classify(&self, hop: &Upstream, upgrade: bool) -> HopClass {
        let _ = (hop, upgrade);
        HopClass::default()
    }
    /// Explicit credential boundary. Defaults to chat/responses/model listing;
    /// adapters must opt into every additional media family.
    fn allows_request(&self, method: &str, hop: &Upstream, upgrade: bool) -> bool {
        let _ = upgrade;
        fabrials_core::hop::core_request_allowed(method, &hop.path)
    }
    /// `Some` = this provider wrote the HTTP response (no reverse-proxy hop).
    fn translated_hop(
        &self,
        method: &str,
        routed_path: &str,
        body: &[u8],
        token: &str,
        secret: Option<&Value>,
        client: &mut dyn Write,
    ) -> Option<Result<Option<UsageRecord>, String>> {
        let _ = (method, routed_path, body, token, secret, client);
        None
    }
}
