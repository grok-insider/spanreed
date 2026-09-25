//! Pinned CodexBar providers that fit AI-Relay's credential boundary.
//!
//! Each origin is a fixed HTTPS host from that provider's API-key usage or
//! inference surface. Operator-chosen base URLs are not entries: injecting a
//! stored key into an arbitrary upstream is outside this proxy.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogAuth {
    Bearer,
    /// `x-api-key` plus the Anthropic version header.
    Anthropic,
    Header(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogSurface {
    /// `POST /v1/responses`, `POST /v1/chat/completions`, `GET /v1/models`.
    OpenAi,
    /// `POST /v1/messages` and `GET /v1/models`.
    Messages,
    /// The prefix is claimed so it cannot fall through to Grok. Client hops
    /// stay closed; the host probes a pinned usage URL with the stored key.
    Quota,
}

#[derive(Clone, Copy, Debug)]
pub struct CatalogRoute {
    pub id: &'static str,
    pub name: &'static str,
    pub upstream: &'static str,
    pub client_prefix: &'static str,
    pub auth: CatalogAuth,
    pub surface: CatalogSurface,
}

impl CatalogRoute {
    pub fn credential_headers(self, token: &str) -> Vec<(String, String)> {
        match self.auth {
            CatalogAuth::Bearer => vec![("Authorization".into(), format!("Bearer {token}"))],
            CatalogAuth::Anthropic => vec![
                ("x-api-key".into(), token.to_string()),
                ("anthropic-version".into(), "2023-06-01".into()),
            ],
            CatalogAuth::Header(name) => vec![(name.into(), token.to_string())],
        }
    }

    pub fn allows(self, method: &str, path: &str) -> bool {
        match self.surface {
            CatalogSurface::OpenAi => fabrials_types::hop::core_request_allowed(method, path),
            CatalogSurface::Messages => {
                fabrials_types::hop::messages_request_allowed(method, path)
                    || (method.eq_ignore_ascii_case("GET")
                        && path.split('?').next().unwrap_or(path).trim_end_matches('/')
                            == "/v1/models")
            }
            CatalogSurface::Quota => false,
        }
    }
}

pub const ROUTES: &[CatalogRoute] = &[
    CatalogRoute {
        id: "atlascloud",
        name: "Atlas Cloud",
        upstream: "https://api.atlascloud.ai",
        client_prefix: "/atlascloud/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Quota,
    },
    CatalogRoute {
        id: "claude",
        name: "Claude",
        upstream: "https://api.anthropic.com",
        client_prefix: "/claude/v1",
        auth: CatalogAuth::Anthropic,
        surface: CatalogSurface::Messages,
    },
    CatalogRoute {
        id: "clawrouter",
        name: "ClawRouter",
        upstream: "https://clawrouter.openclaw.ai",
        client_prefix: "/clawrouter/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::OpenAi,
    },
    CatalogRoute {
        id: "clinepass",
        name: "ClinePass",
        upstream: "https://api.cline.bot",
        client_prefix: "/clinepass/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Quota,
    },
    CatalogRoute {
        id: "codebuff",
        name: "Codebuff",
        upstream: "https://www.codebuff.com",
        client_prefix: "/codebuff/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Quota,
    },
    CatalogRoute {
        id: "deepinfra",
        name: "DeepInfra",
        upstream: "https://api.deepinfra.com",
        client_prefix: "/deepinfra/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Quota,
    },
    CatalogRoute {
        id: "deepseek",
        name: "DeepSeek",
        upstream: "https://api.deepseek.com",
        client_prefix: "/deepseek/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::OpenAi,
    },
    CatalogRoute {
        id: "devpass",
        name: "DevPass",
        upstream: "https://api.llmgateway.io",
        client_prefix: "/devpass/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::OpenAi,
    },
    CatalogRoute {
        id: "elevenlabs",
        name: "ElevenLabs",
        upstream: "https://api.elevenlabs.io",
        client_prefix: "/elevenlabs/v1",
        auth: CatalogAuth::Header("xi-api-key"),
        surface: CatalogSurface::Quota,
    },
    CatalogRoute {
        id: "factory",
        name: "Factory",
        upstream: "https://api.factory.ai",
        client_prefix: "/factory/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Quota,
    },
    CatalogRoute {
        id: "fireworks",
        name: "Fireworks",
        upstream: "https://api.fireworks.ai/inference",
        client_prefix: "/fireworks/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::OpenAi,
    },
    CatalogRoute {
        id: "groq",
        name: "Groq",
        upstream: "https://api.groq.com/openai",
        client_prefix: "/groq/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::OpenAi,
    },
    CatalogRoute {
        id: "huggingface",
        name: "Hugging Face",
        upstream: "https://router.huggingface.co",
        client_prefix: "/huggingface/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::OpenAi,
    },
    CatalogRoute {
        id: "kimi",
        name: "Kimi Code",
        upstream: "https://api.kimi.com/coding",
        client_prefix: "/kimi/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Messages,
    },
    CatalogRoute {
        id: "minimax-coding",
        name: "MiniMax coding plan",
        upstream: "https://api.minimax.io/anthropic",
        client_prefix: "/minimax-coding/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Messages,
    },
    CatalogRoute {
        id: "moonshot",
        name: "Moonshot",
        upstream: "https://api.moonshot.ai",
        client_prefix: "/moonshot/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::OpenAi,
    },
    CatalogRoute {
        id: "openrouter",
        name: "OpenRouter",
        upstream: "https://openrouter.ai/api",
        client_prefix: "/openrouter/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::OpenAi,
    },
    CatalogRoute {
        id: "poe",
        name: "Poe",
        upstream: "https://api.poe.com",
        client_prefix: "/poe/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::OpenAi,
    },
    CatalogRoute {
        id: "synthetic",
        name: "Synthetic",
        upstream: "https://api.synthetic.new",
        client_prefix: "/synthetic/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Quota,
    },
    CatalogRoute {
        id: "v0",
        name: "v0",
        upstream: "https://api.v0.dev",
        client_prefix: "/v0/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Quota,
    },
    CatalogRoute {
        id: "venice",
        name: "Venice",
        upstream: "https://api.venice.ai/api",
        client_prefix: "/venice/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::OpenAi,
    },
    CatalogRoute {
        id: "vercel",
        name: "Vercel AI Gateway",
        upstream: "https://ai-gateway.vercel.sh",
        client_prefix: "/vercel/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::OpenAi,
    },
    CatalogRoute {
        id: "zai",
        name: "z.ai",
        upstream: "https://api.z.ai",
        client_prefix: "/zai/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Quota,
    },
    CatalogRoute {
        id: "zai-coding",
        name: "Z.ai coding plan",
        upstream: "https://api.z.ai/api/anthropic",
        client_prefix: "/zai-coding/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Messages,
    },
    CatalogRoute {
        id: "hyper",
        name: "Charm Hyper",
        upstream: "https://hyper.charm.land",
        client_prefix: "/hyper/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Quota,
    },
    CatalogRoute {
        id: "zenmux",
        name: "ZenMux",
        upstream: "https://zenmux.ai",
        client_prefix: "/zenmux/v1",
        auth: CatalogAuth::Bearer,
        surface: CatalogSurface::Quota,
    },
];

pub fn by_id(id: &str) -> Option<&'static CatalogRoute> {
    ROUTES.iter().find(|route| route.id == id)
}

/// Prefixes owned by a dedicated adapter rather than
/// `fabrials_upstreams::catalog::CatalogAdapter`. The entry stays as pinned data
/// (origin, surface, credential headers) for hosts that probe it, and the dedicated
/// adapter must resolve the same origin.
pub fn dedicated_adapter(id: &str) -> bool {
    matches!(id, "claude" | "kimi")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedicated_adapters_keep_their_catalog_entry() {
        let dedicated = ROUTES
            .iter()
            .filter(|route| dedicated_adapter(route.id))
            .map(|route| route.id)
            .collect::<Vec<_>>();
        assert_eq!(dedicated, vec!["claude", "kimi"]);
        for id in dedicated {
            assert!(by_id(id).is_some(), "{id}");
        }
    }

    #[test]
    fn catalog_ids_are_unique_pinned_https_origins() {
        let mut seen = std::collections::BTreeSet::new();
        for route in ROUTES {
            assert!(seen.insert(route.id), "duplicate {}", route.id);
            assert!(fabrials_accounts::valid_alias(route.id), "{}", route.id);
            assert_eq!(route.client_prefix, format!("/{}/v1", route.id));
            let url = url::Url::parse(route.upstream).unwrap_or_else(|_| panic!("{}", route.id));
            assert_eq!(url.scheme(), "https", "{}", route.id);
            assert!(url.username().is_empty(), "{}", route.id);
            assert!(url.password().is_none(), "{}", route.id);
            assert!(url.query().is_none(), "{}", route.id);
            assert!(url.fragment().is_none(), "{}", route.id);
            assert!(!matches!(
                route.id,
                "grok"
                    | "nous"
                    | "openai"
                    | "codex"
                    | "opencode-go"
                    | "xai"
                    | "cursor"
                    | "grok-bot"
            ));
        }
    }
}
