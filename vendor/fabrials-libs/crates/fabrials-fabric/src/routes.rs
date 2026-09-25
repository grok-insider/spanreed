//! Engine path policy: request-target safety and the host's control-plane
//! prefix. Provider routing lives with the adapters.

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

/// The engine's default control-plane prefix.
pub const DEFAULT_CONTROL_PREFIX: &str = "/__spanreed";

/// Where a host serves its control plane (`<prefix>/health`, `<prefix>/usage`, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPrefix(String);

impl Default for ControlPrefix {
    fn default() -> Self {
        Self(DEFAULT_CONTROL_PREFIX.into())
    }
}

impl ControlPrefix {
    /// A prefix is one or more `/segment`s of ASCII letters, digits, `_` or `-`.
    pub fn new(prefix: &str) -> Result<Self, String> {
        let valid = prefix.len() > 1
            && prefix.len() <= 64
            && prefix.starts_with('/')
            && prefix[1..].split('/').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            });
        if valid {
            Ok(Self(prefix.into()))
        } else {
            Err("Invalid control prefix".into())
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `true` when `path` (query ignored) is `<prefix><suffix>` with or without a trailing slash.
    pub fn matches(&self, path: &str, suffix: &str) -> bool {
        let path = path.split('?').next().unwrap_or(path);
        path.strip_prefix(self.0.as_str())
            .and_then(|rest| rest.strip_prefix(suffix))
            .is_some_and(|rest| rest.is_empty() || rest == "/")
    }

    /// `/health`, `<prefix>/health`, or any path ending in `<prefix>/health`.
    pub fn is_health(&self, path: &str) -> bool {
        let p = path.split('?').next().unwrap_or(path);
        p == "/health" || self.matches(p, "/health") || p.ends_with(&format!("{}/health", self.0))
    }

    pub fn is_environment(&self, path: &str) -> bool {
        self.matches(path, "/environment")
    }

    pub fn is_autosteer(&self, path: &str) -> bool {
        self.matches(path, "/autosteer")
    }

    pub fn is_limits(&self, path: &str) -> bool {
        self.matches(path, "/limits")
    }

    pub fn is_ingest(&self, path: &str) -> bool {
        self.matches(path, "/usage")
    }

    pub fn is_sync(&self, path: &str) -> bool {
        ["/sync/account", "/sync/usage", "/sync/pull"]
            .iter()
            .any(|suffix| self.matches(path, suffix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ingest_path() {
        let control = ControlPrefix::default();
        assert!(control.is_ingest("/__spanreed/usage"));
        assert!(control.is_ingest("/__spanreed/usage/"));
        assert!(!control.is_ingest("/v1/responses"));
        assert!(control.is_sync("/__spanreed/sync/pull"));
        assert!(control.is_sync("/__spanreed/sync/account"));
        assert!(!control.is_sync("/__spanreed/usage"));
    }

    #[test]
    fn environment_descriptor_path_is_exact() {
        let control = ControlPrefix::default();
        assert!(control.is_environment("/__spanreed/environment"));
        assert!(control.is_environment("/__spanreed/environment/?probe=1"));
        assert!(!control.is_environment("/__spanreed/environment-evil"));
    }

    #[test]
    fn limits_path() {
        let control = ControlPrefix::default();
        assert!(control.is_limits("/__spanreed/limits"));
        assert!(control.is_limits("/__spanreed/limits/"));
        assert!(control.is_limits("/__spanreed/limits?provider=nous"));
        assert!(!control.is_limits("/__spanreed/autosteer"));
        assert!(!control.is_limits("/v1/models"));
    }

    #[test]
    fn a_host_can_move_the_control_plane() {
        let control = ControlPrefix::new("/__relay").unwrap();
        assert!(control.is_health("/__relay/health"));
        assert!(control.is_health("/health"));
        assert!(control.is_ingest("/__relay/usage/"));
        assert!(!control.is_ingest("/__spanreed/usage"));
        assert!(!control.is_health("/__spanreed/health"));
        for invalid in ["", "/", "relay", "/a//b", "/a?b", "/a/"] {
            assert!(ControlPrefix::new(invalid).is_err(), "{invalid}");
        }
    }
}
