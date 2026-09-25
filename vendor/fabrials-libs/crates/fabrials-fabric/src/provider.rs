//! Provider ports. A provider is the composition of five small roles; the
//! engine never names Grok, Codex or Claude. New LLMs = new adapter
//! implementing the roles (or a [`ProviderParts`] bundle of them).

use std::io::Write;
use std::sync::Arc;

use fabrials_types::hop::HopClass;
use fabrials_types::HopRecord;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    pub base: String,
    pub path: String,
    pub account_alias: Option<String>,
    pub route: &'static str,
}

/// Which upstream owns a client path, what the hop is, and whether it may carry a credential.
pub trait Router: Send + Sync {
    fn id(&self) -> &'static str;
    /// `None` = this provider does not own the path (try the next).
    fn resolve(&self, raw_path: &str) -> Option<Upstream>;
    /// What this hop is. Default: chat over HTTP (no voice tunnel).
    fn classify(&self, hop: &Upstream, upgrade: bool) -> HopClass {
        let _ = (hop, upgrade);
        HopClass::default()
    }
    /// Explicit credential boundary. Defaults to chat/responses/model listing;
    /// adapters must opt into every additional media family.
    fn allows_request(&self, method: &str, hop: &Upstream, upgrade: bool) -> bool {
        let _ = upgrade;
        fabrials_types::hop::core_request_allowed(method, &hop.path)
    }
}

/// How a stored credential becomes upstream headers.
pub trait CredentialInjector: Send + Sync {
    fn inject(&self, token: &str) -> Vec<(String, String)>;
    /// Header injection for this hop. Default = [`Self::inject`].
    fn inject_for(&self, token: &str, hop: &Upstream) -> Vec<(String, String)> {
        let _ = hop;
        self.inject(token)
    }
    /// Adjust hop origin after the injected credential is known. Default no-op.
    fn bind_credential(&self, hop: &mut Upstream, token: &str) {
        let _ = (hop, token);
    }
    fn credential_headers(
        &self,
        token: &str,
        hop: &Upstream,
        secret: Option<&Value>,
    ) -> Result<Vec<(String, String)>, String> {
        let _ = secret;
        Ok(self.inject_for(token, hop))
    }
}

/// Official usage out of a captured response body.
pub trait UsageExtractor: Send + Sync {
    fn parse_usage(&self, response_body: &[u8]) -> Option<HopRecord>;
}

/// Hops the provider answers itself instead of a plain reverse proxy.
pub trait Translator: Send + Sync {
    /// `Some` = this provider wrote the HTTP response (no reverse-proxy hop).
    fn translated_hop(
        &self,
        method: &str,
        routed_path: &str,
        body: &[u8],
        token: &str,
        secret: Option<&Value>,
        client: &mut dyn Write,
    ) -> Option<Result<Option<HopRecord>, String>> {
        let _ = (method, routed_path, body, token, secret, client);
        None
    }
    /// `Some` = this provider ran the WebSocket hop itself.
    fn websocket_hop(&self, hop: crate::forward::WebSocketHop<'_>) -> Option<Result<(), String>> {
        let _ = hop;
        None
    }
    /// `true` when a successful model listing on this hop goes through the
    /// host's [`crate::forward::HopObserver::models_response`] rewrite.
    fn rewrites_models_list(&self, method: &str, hop: &Upstream) -> bool {
        let _ = (method, hop);
        false
    }
}

/// Request body changes an upstream requires before forwarding.
pub trait BodyShaper: Send + Sync {
    /// `None` = forward the body unchanged.
    fn shape_request(&self, hop: &Upstream, token: Option<&str>, body: &[u8]) -> Option<Vec<u8>> {
        let _ = (hop, token, body);
        None
    }
}

/// Outbound LLM/CLI backend: every role at once. Implemented for any type that
/// implements the five roles.
pub trait Provider: Router + CredentialInjector + UsageExtractor + Translator + BodyShaper {}

impl<T> Provider for T where
    T: Router + CredentialInjector + UsageExtractor + Translator + BodyShaper
{
}

/// The default for roles a provider does not play: no translation, no shaping.
#[derive(Debug, Clone, Copy, Default)]
pub struct Passthrough;

impl Translator for Passthrough {}
impl BodyShaper for Passthrough {}

/// A provider assembled from independent role implementations.
#[derive(Clone)]
pub struct ProviderParts {
    pub router: Arc<dyn Router>,
    pub credentials: Arc<dyn CredentialInjector>,
    pub usage: Arc<dyn UsageExtractor>,
    pub translator: Arc<dyn Translator>,
    pub shaper: Arc<dyn BodyShaper>,
}

impl ProviderParts {
    /// Bundle one adapter's router, credentials and usage parser with no
    /// translation and no body shaping.
    pub fn new<P>(adapter: Arc<P>) -> Self
    where
        P: Router + CredentialInjector + UsageExtractor + 'static,
    {
        Self {
            router: adapter.clone(),
            credentials: adapter.clone(),
            usage: adapter,
            translator: Arc::new(Passthrough),
            shaper: Arc::new(Passthrough),
        }
    }
}

impl Router for ProviderParts {
    fn id(&self) -> &'static str {
        self.router.id()
    }
    fn resolve(&self, raw_path: &str) -> Option<Upstream> {
        self.router.resolve(raw_path)
    }
    fn classify(&self, hop: &Upstream, upgrade: bool) -> HopClass {
        self.router.classify(hop, upgrade)
    }
    fn allows_request(&self, method: &str, hop: &Upstream, upgrade: bool) -> bool {
        self.router.allows_request(method, hop, upgrade)
    }
}

impl CredentialInjector for ProviderParts {
    fn inject(&self, token: &str) -> Vec<(String, String)> {
        self.credentials.inject(token)
    }
    fn inject_for(&self, token: &str, hop: &Upstream) -> Vec<(String, String)> {
        self.credentials.inject_for(token, hop)
    }
    fn bind_credential(&self, hop: &mut Upstream, token: &str) {
        self.credentials.bind_credential(hop, token)
    }
    fn credential_headers(
        &self,
        token: &str,
        hop: &Upstream,
        secret: Option<&Value>,
    ) -> Result<Vec<(String, String)>, String> {
        self.credentials.credential_headers(token, hop, secret)
    }
}

impl UsageExtractor for ProviderParts {
    fn parse_usage(&self, response_body: &[u8]) -> Option<HopRecord> {
        self.usage.parse_usage(response_body)
    }
}

impl Translator for ProviderParts {
    fn translated_hop(
        &self,
        method: &str,
        routed_path: &str,
        body: &[u8],
        token: &str,
        secret: Option<&Value>,
        client: &mut dyn Write,
    ) -> Option<Result<Option<HopRecord>, String>> {
        self.translator
            .translated_hop(method, routed_path, body, token, secret, client)
    }
    fn websocket_hop(&self, hop: crate::forward::WebSocketHop<'_>) -> Option<Result<(), String>> {
        self.translator.websocket_hop(hop)
    }
    fn rewrites_models_list(&self, method: &str, hop: &Upstream) -> bool {
        self.translator.rewrites_models_list(method, hop)
    }
}

impl BodyShaper for ProviderParts {
    fn shape_request(&self, hop: &Upstream, token: Option<&str>, body: &[u8]) -> Option<Vec<u8>> {
        self.shaper.shape_request(hop, token, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed;
    impl Router for Fixed {
        fn id(&self) -> &'static str {
            "fixed"
        }
        fn resolve(&self, raw_path: &str) -> Option<Upstream> {
            Some(Upstream {
                base: "https://example.invalid".into(),
                path: raw_path.into(),
                account_alias: None,
                route: "fixed",
            })
        }
    }
    impl CredentialInjector for Fixed {
        fn inject(&self, token: &str) -> Vec<(String, String)> {
            vec![("Authorization".into(), format!("Bearer {token}"))]
        }
    }
    impl UsageExtractor for Fixed {
        fn parse_usage(&self, _response_body: &[u8]) -> Option<HopRecord> {
            None
        }
    }

    struct Uppercase;
    impl BodyShaper for Uppercase {
        fn shape_request(
            &self,
            _hop: &Upstream,
            _token: Option<&str>,
            body: &[u8],
        ) -> Option<Vec<u8>> {
            Some(body.to_ascii_uppercase())
        }
    }

    #[test]
    fn parts_compose_into_one_provider() {
        let mut parts = ProviderParts::new(Arc::new(Fixed));
        parts.shaper = Arc::new(Uppercase);
        let provider: &dyn Provider = &parts;
        let hop = provider.resolve("/v1/models").unwrap();
        assert_eq!(provider.id(), "fixed");
        assert_eq!(provider.inject_for("t", &hop)[0].1, "Bearer t");
        assert!(!provider.rewrites_models_list("GET", &hop));
        assert_eq!(
            provider.shape_request(&hop, None, b"abc").unwrap(),
            b"ABC".to_vec()
        );
        assert!(provider
            .translated_hop("POST", "/", b"", "", None, &mut Vec::new())
            .is_none());
    }
}
