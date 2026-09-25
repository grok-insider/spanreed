use crate::catalog::CatalogRoute;
use crate::provider::{Provider, Upstream};
use crate::routes::parse_fabric_path;
use fabrials_core::hop::HopClass;
use fabrials_model::UsageRecord;

pub struct CatalogAdapter {
    pub spec: &'static CatalogRoute,
}

impl Provider for CatalogAdapter {
    fn id(&self) -> &'static str {
        self.spec.id
    }

    fn resolve(&self, raw_path: &str) -> Option<Upstream> {
        let routed = parse_fabric_path(raw_path);
        if routed.route != self.spec.id {
            return None;
        }
        Some(Upstream {
            base: self.spec.upstream.trim_end_matches('/').to_string(),
            path: routed.path,
            account_alias: routed.account_alias,
            route: routed.route,
        })
    }

    fn inject(&self, token: &str) -> Vec<(String, String)> {
        self.spec.credential_headers(token)
    }

    fn parse_usage(&self, response_body: &[u8]) -> Option<UsageRecord> {
        let text = String::from_utf8_lossy(response_body);
        fabrials_metrics::usage_from_messages_body(&text)
            .or_else(|| fabrials_metrics::usage_from_response_body(&text))
            .map(|parsed| parsed.into_record(now_ms(), None, None, Some(self.spec.id.into())))
    }

    fn classify(&self, hop: &Upstream, upgrade: bool) -> HopClass {
        HopClass::openai_compat(&hop.path, upgrade)
    }

    fn allows_request(&self, method: &str, hop: &Upstream, upgrade: bool) -> bool {
        let _ = upgrade;
        self.spec.allows(method, &hop.path)
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{by_id, CatalogSurface};

    #[test]
    fn openrouter_owns_its_prefix_and_core_surface() {
        let adapter = CatalogAdapter {
            spec: by_id("openrouter").unwrap(),
        };
        let hop = adapter.resolve("/openrouter/v1/chat/completions").unwrap();
        assert_eq!(hop.base, "https://openrouter.ai/api");
        assert_eq!(hop.path, "/v1/chat/completions");
        assert!(adapter.allows_request("POST", &hop, false));
        let files = adapter.resolve("/openrouter/v1/files").unwrap();
        assert!(!adapter.allows_request("GET", &files, false));
        assert!(adapter.resolve("/v1/chat/completions").is_none());
        let headers = adapter.inject("tok");
        assert!(headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value == "Bearer tok"
        }));
    }

    #[test]
    fn claude_entry_stays_as_data_for_its_dedicated_adapter() {
        let spec = by_id("claude").unwrap();
        assert_eq!(spec.upstream, "https://api.anthropic.com");
        assert_eq!(spec.surface, CatalogSurface::Messages);
        assert!(spec.allows("POST", "/v1/messages"));
        assert!(!spec.allows("GET", "/v1/messages"));
        assert!(!spec.allows("POST", "/v1/chat/completions"));
        assert!(spec.allows("GET", "/v1/models"));
        let headers = spec.credential_headers("sk");
        assert!(headers
            .iter()
            .any(|(name, value)| name == "x-api-key" && value == "sk"));
        assert!(headers
            .iter()
            .any(|(name, value)| name == "anthropic-version" && value == "2023-06-01"));
    }

    #[test]
    fn coding_plan_prefixes_claim_the_messages_surface() {
        for (id, upstream) in [
            ("zai-coding", "https://api.z.ai/api/anthropic"),
            ("minimax-coding", "https://api.minimax.io/anthropic"),
        ] {
            let spec = by_id(id).unwrap();
            assert_eq!(spec.upstream, upstream);
            assert_eq!(spec.surface, CatalogSurface::Messages);
            assert!(spec.allows("POST", "/v1/messages"));
            assert!(!spec.allows("POST", "/v1/chat/completions"));
            assert!(spec.allows("GET", "/v1/models"));
            let headers = spec.credential_headers("plan-key");
            assert!(headers
                .iter()
                .any(|(name, value)| name == "Authorization" && value == "Bearer plan-key"));
        }
    }

    #[test]
    fn quota_providers_claim_the_prefix_without_forwarding() {
        let adapter = CatalogAdapter {
            spec: by_id("elevenlabs").unwrap(),
        };
        assert_eq!(adapter.spec.surface, CatalogSurface::Quota);
        let hop = adapter.resolve("/elevenlabs/v1/user/subscription").unwrap();
        assert!(!adapter.allows_request("GET", &hop, false));
        let headers = adapter.inject("key");
        assert_eq!(headers, vec![("xi-api-key".into(), "key".into())]);
    }

    #[test]
    fn parse_usage_keeps_the_provider_route() {
        let adapter = CatalogAdapter {
            spec: by_id("deepseek").unwrap(),
        };
        let body = br#"{"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}}"#;
        let record = adapter.parse_usage(body).unwrap();
        assert_eq!(record.input_tokens, 3);
        assert_eq!(record.output_tokens, 4);
        assert_eq!(record.route.as_deref(), Some("deepseek"));
    }
}
