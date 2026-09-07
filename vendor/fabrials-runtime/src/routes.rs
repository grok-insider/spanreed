//! Fabric path routing (desktop capture compatibility).

pub const UPSTREAM_GROK_CLI: &str = fabrials_oauth_grok::UPSTREAM_GROK_CLI;
pub const UPSTREAM_XAI_API: &str = fabrials_oauth_grok::UPSTREAM_XAI_API;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricRoute {
    pub path: String,
    pub account_alias: Option<String>,
    pub route: &'static str,
    pub upstream: &'static str,
}

fn with_query(path: &str, query: Option<&str>) -> String {
    match query {
        Some(q) => format!("{path}?{q}"),
        None => path.to_string(),
    }
}

/// Match a route name as a complete path segment and keep the routed suffix
/// in origin-form. A plain `strip_prefix` would turn `/openai@attacker/…`
/// into `@attacker/…`, which can change the authority when concatenated with
/// an upstream base URL.
fn strip_route_segment<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if path == prefix {
        return Some("/");
    }
    path.strip_prefix(prefix)
        .filter(|rest| rest.starts_with('/'))
}

/// Reject request targets whose path can be reinterpreted by a URL parser or
/// upstream server after the credential allowlist has run. Queries may remain
/// percent-encoded; the routed path itself is deliberately strict.
pub fn is_safe_request_target(raw: &str) -> bool {
    let path = raw.split_once('?').map(|(path, _)| path).unwrap_or(raw);
    path.starts_with('/')
        && !raw.contains('#')
        && !path.contains('%')
        && !path.contains('\\')
        && !path.chars().any(|character| {
            character.is_control() || character.is_whitespace() || !character.is_ascii()
        })
        && path
            .split('/')
            .all(|segment| !matches!(segment, "." | ".."))
}

pub fn parse_fabric_path(raw: &str) -> FabricRoute {
    let (path_only, query) = match raw.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (raw, None),
    };

    if let Some(rest) = path_only.strip_prefix("/acct/") {
        let (alias, after) = match rest.split_once('/') {
            Some((a, t)) => (a, format!("/{t}")),
            None => (rest, "/".into()),
        };
        if fabrials_accounts::valid_alias(alias) {
            if let Some(p) = strip_route_segment(&after, "/xai") {
                return FabricRoute {
                    path: with_query(p, query),
                    account_alias: Some(alias.into()),
                    route: "xai",
                    upstream: UPSTREAM_XAI_API,
                };
            }
            if let Some(p) = strip_route_segment(&after, "/cursor") {
                return FabricRoute {
                    path: with_query(p, query),
                    account_alias: Some(alias.into()),
                    route: "cursor",
                    upstream: "https://api.cursor.com",
                };
            }
            if let Some(p) = strip_route_segment(&after, "/nous") {
                return FabricRoute {
                    path: with_query(p, query),
                    account_alias: Some(alias.into()),
                    route: "nous",
                    upstream: "https://inference-api.nousresearch.com",
                };
            }
            if let Some(p) = strip_route_segment(&after, "/openai") {
                return FabricRoute {
                    path: with_query(p, query),
                    account_alias: Some(alias.into()),
                    route: "openai",
                    upstream: "https://api.openai.com",
                };
            }
            if let Some(p) = strip_route_segment(&after, "/grok-bot") {
                return FabricRoute {
                    path: with_query(p, query),
                    account_alias: Some(alias.into()),
                    route: "grok-bot",
                    upstream: "https://api2.cursor.sh",
                };
            }
            return grok_fabric(with_query(&after, query), Some(alias.into()));
        }
    }

    if let Some(p) = strip_route_segment(path_only, "/xai") {
        return FabricRoute {
            path: with_query(p, query),
            account_alias: None,
            route: "xai",
            upstream: UPSTREAM_XAI_API,
        };
    }

    if let Some(p) = strip_route_segment(path_only, "/cursor") {
        return FabricRoute {
            path: with_query(p, query),
            account_alias: None,
            route: "cursor",
            upstream: "https://api.cursor.com",
        };
    }

    if let Some(p) = strip_route_segment(path_only, "/nous") {
        return FabricRoute {
            path: with_query(p, query),
            account_alias: None,
            route: "nous",
            upstream: "https://inference-api.nousresearch.com",
        };
    }

    if let Some(p) = strip_route_segment(path_only, "/openai") {
        return FabricRoute {
            path: with_query(p, query),
            account_alias: None,
            route: "openai",
            upstream: "https://api.openai.com",
        };
    }

    if let Some(p) = strip_route_segment(path_only, "/grok-bot") {
        return FabricRoute {
            path: with_query(p, query),
            account_alias: None,
            route: "grok-bot",
            upstream: "https://api2.cursor.sh",
        };
    }

    grok_fabric(raw.to_string(), None)
}

fn grok_fabric(path: String, account_alias: Option<String>) -> FabricRoute {
    let media = fabrials_core::hop::openai_compat_kind(&path).is_some();
    FabricRoute {
        path,
        account_alias,
        route: "grok",
        upstream: if media {
            UPSTREAM_XAI_API
        } else {
            UPSTREAM_GROK_CLI
        },
    }
}

/// Imagine / voice HTTP on `api.x.ai` (Grok Build uses `xai_api_base_url`, not CCP).
pub fn is_xai_media_path(raw: &str) -> bool {
    fabrials_core::hop::openai_compat_kind(raw).is_some()
}

pub fn is_health_path(path: &str) -> bool {
    let p = path.split('?').next().unwrap_or(path);
    p == "/health"
        || p == "/__spanreed/health"
        || p == "/__spanreed/health/"
        || p.ends_with("/__spanreed/health")
}

pub fn is_environment_path(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    matches!(path, "/__spanreed/environment" | "/__spanreed/environment/")
}

pub fn is_autosteer_path(path: &str) -> bool {
    let p = path.split('?').next().unwrap_or(path);
    p == "/__spanreed/autosteer" || p == "/__spanreed/autosteer/"
}

pub fn is_limits_path(path: &str) -> bool {
    let p = path.split('?').next().unwrap_or(path);
    p == "/__spanreed/limits" || p == "/__spanreed/limits/"
}

pub fn is_ingest_path(path: &str) -> bool {
    let p = path.split('?').next().unwrap_or(path);
    p == "/__spanreed/usage" || p == "/__spanreed/usage/"
}

pub fn is_sync_path(path: &str) -> bool {
    let p = path.split('?').next().unwrap_or(path);
    p == "/__spanreed/sync/account"
        || p == "/__spanreed/sync/account/"
        || p == "/__spanreed/sync/usage"
        || p == "/__spanreed/sync/usage/"
        || p == "/__spanreed/sync/pull"
        || p == "/__spanreed/sync/pull/"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_is_grok_cli() {
        let r = parse_fabric_path("/v1/responses");
        assert_eq!(r.route, "grok");
        assert_eq!(r.upstream, UPSTREAM_GROK_CLI);
        assert!(r.account_alias.is_none());
    }

    #[test]
    fn unsafe_url_normalization_targets_are_rejected() {
        assert!(is_safe_request_target("/openai/v1/models?after=a%2Fb"));
        for path in [
            "/openai/v1/videos/../files",
            "/openai/v1/videos/%2e%2e/files",
            "/openai/v1/videos/%2E./files",
            "/openai/v1/videos/..\\files",
            "/openai/v1/videos/../../files",
            "/openai/v1/videos/item#fragment",
            "https://attacker.invalid/v1/models",
        ] {
            assert!(!is_safe_request_target(path), "{path}");
        }
    }

    #[test]
    fn v1_media_is_grok_route_on_xai_api() {
        for p in [
            "/v1/images/generations",
            "/v1/images/edits",
            "/v1/videos/generations",
            "/v1/videos/abc-123",
            "/v1/tts",
            "/v1/tts/voices",
            "/v1/stt",
            "/v1/custom-voices",
            "/v1/realtime",
            "/v1/realtime?model=grok-voice-latest",
            "/v1/images/generations?n=1",
        ] {
            let r = parse_fabric_path(p);
            assert_eq!(r.route, "grok", "{p}");
            assert_eq!(r.upstream, UPSTREAM_XAI_API, "{p}");
            assert!(r.account_alias.is_none(), "{p}");
            assert!(r.path.starts_with("/v1/"), "{p} path {}", r.path);
        }
        let r = parse_fabric_path("/acct/work/v1/images/generations");
        assert_eq!(r.account_alias.as_deref(), Some("work"));
        assert_eq!(r.route, "grok");
        assert_eq!(r.path, "/v1/images/generations");
        assert_eq!(r.upstream, UPSTREAM_XAI_API);
        let r = parse_fabric_path("/v1/chat/completions");
        assert_eq!(r.upstream, UPSTREAM_GROK_CLI);
        assert!(is_xai_media_path("/v1/stt"));
        assert!(!is_xai_media_path("/v1/responses"));
    }

    #[test]
    fn acct_alias_v1() {
        let r = parse_fabric_path("/acct/work/v1/chat/completions");
        assert_eq!(r.account_alias.as_deref(), Some("work"));
        assert_eq!(r.route, "grok");
        assert_eq!(r.path, "/v1/chat/completions");
    }

    #[test]
    fn xai_and_acct_xai() {
        let r = parse_fabric_path("/xai/v1/chat/completions");
        assert_eq!(r.route, "xai");
        assert_eq!(r.upstream, UPSTREAM_XAI_API);
        let r = parse_fabric_path("/acct/heavy/xai/v1/models");
        assert_eq!(r.account_alias.as_deref(), Some("heavy"));
        assert_eq!(r.route, "xai");
        assert_eq!(r.path, "/v1/models");
    }

    #[test]
    fn cursor_and_acct_cursor() {
        let r = parse_fabric_path("/cursor/v1/me");
        assert_eq!(r.route, "cursor");
        assert_eq!(r.path, "/v1/me");
        assert!(r.account_alias.is_none());
        let r = parse_fabric_path("/acct/work/cursor/v1/agents");
        assert_eq!(r.account_alias.as_deref(), Some("work"));
        assert_eq!(r.route, "cursor");
        assert_eq!(r.path, "/v1/agents");
    }

    #[test]
    fn grok_bot_and_acct() {
        let r = parse_fabric_path("/grok-bot/v1/chat/completions");
        assert_eq!(r.route, "grok-bot");
        assert_eq!(r.path, "/v1/chat/completions");
        assert_eq!(r.upstream, "https://api2.cursor.sh");
        assert!(r.account_alias.is_none());
        let r = parse_fabric_path("/acct/house/grok-bot/v1/models");
        assert_eq!(r.account_alias.as_deref(), Some("house"));
        assert_eq!(r.route, "grok-bot");
        assert_eq!(r.path, "/v1/models");
    }

    #[test]
    fn nous_and_acct_nous() {
        let r = parse_fabric_path("/nous/v1/chat/completions");
        assert_eq!(r.route, "nous");
        assert_eq!(r.path, "/v1/chat/completions");
        assert_eq!(r.upstream, "https://inference-api.nousresearch.com");
        assert!(r.account_alias.is_none());
        let r = parse_fabric_path("/acct/work/nous/v1/models");
        assert_eq!(r.account_alias.as_deref(), Some("work"));
        assert_eq!(r.route, "nous");
        assert_eq!(r.path, "/v1/models");
    }

    #[test]
    fn openai_and_acct_openai() {
        let r = parse_fabric_path("/openai/v1/chat/completions");
        assert_eq!(r.route, "openai");
        assert_eq!(r.path, "/v1/chat/completions");
        assert_eq!(r.upstream, "https://api.openai.com");
        assert!(r.account_alias.is_none());
        let r = parse_fabric_path("/acct/work/openai/v1/models");
        assert_eq!(r.account_alias.as_deref(), Some("work"));
        assert_eq!(r.route, "openai");
        assert_eq!(r.path, "/v1/models");
    }

    #[test]
    fn provider_prefixes_require_a_segment_boundary() {
        for prefix in ["xai", "cursor", "nous", "openai", "grok-bot"] {
            for suffix in [
                "@attacker.invalid/v1/models",
                ".attacker.invalid/v1/models",
                ":443@attacker.invalid/v1/models",
                "%40attacker.invalid/v1/models",
                "%2eattacker.invalid/v1/models",
                "%3a443%40attacker.invalid/v1/models",
            ] {
                let direct = format!("/{prefix}{suffix}");
                let route = parse_fabric_path(&direct);
                assert_eq!(route.route, "grok", "{direct}");
                assert_eq!(route.path, direct, "{direct}");
                assert!(route.path.starts_with('/'), "{direct}");

                let pinned = format!("/acct/work/{prefix}{suffix}");
                let route = parse_fabric_path(&pinned);
                assert_eq!(route.route, "grok", "{pinned}");
                assert_eq!(route.account_alias.as_deref(), Some("work"), "{pinned}");
                assert_eq!(route.path, format!("/{prefix}{suffix}"), "{pinned}");
                assert!(route.path.starts_with('/'), "{pinned}");
            }
        }
    }

    #[test]
    fn account_aliases_use_the_persisted_alias_grammar() {
        for alias in [
            "work@attacker.invalid",
            "work.attacker.invalid",
            "work:443@attacker.invalid",
            "work%2fopenai%40attacker.invalid",
        ] {
            let raw = format!("/acct/{alias}/openai/v1/models");
            let route = parse_fabric_path(&raw);
            assert_eq!(route.route, "grok", "{raw}");
            assert!(route.account_alias.is_none(), "{raw}");
            assert_eq!(route.path, raw, "{raw}");
        }
    }

    #[test]
    fn ingest_path() {
        assert!(is_ingest_path("/__spanreed/usage"));
        assert!(is_ingest_path("/__spanreed/usage/"));
        assert!(!is_ingest_path("/v1/responses"));
        assert!(is_sync_path("/__spanreed/sync/pull"));
        assert!(is_sync_path("/__spanreed/sync/account"));
        assert!(!is_sync_path("/__spanreed/usage"));
    }

    #[test]
    fn environment_descriptor_path_is_exact() {
        assert!(is_environment_path("/__spanreed/environment"));
        assert!(is_environment_path("/__spanreed/environment/?probe=1"));
        assert!(!is_environment_path("/__spanreed/environment-evil"));
    }

    #[test]
    fn limits_path() {
        assert!(is_limits_path("/__spanreed/limits"));
        assert!(is_limits_path("/__spanreed/limits/"));
        assert!(is_limits_path("/__spanreed/limits?provider=nous"));
        assert!(!is_limits_path("/__spanreed/autosteer"));
        assert!(!is_limits_path("/v1/models"));
    }
}
