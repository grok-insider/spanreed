//! Hop classification. Providers opt in; defaults are chat over HTTP.

/// What the caller asked for. Partial providers implement a subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HopKind {
    Chat,
    Models,
    Image,
    Video,
    Tts,
    Stt,
    Realtime,
}

impl HopKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Models => "models",
            Self::Image => "image",
            Self::Video => "video",
            Self::Tts => "tts",
            Self::Stt => "stt",
            Self::Realtime => "realtime",
        }
    }

    /// Always persist a hop row (no token `usage` object required).
    pub fn always_record(self) -> bool {
        !matches!(self, Self::Chat | Self::Models)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Http,
    WebSocket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopClass {
    pub kind: HopKind,
    pub transport: Transport,
}

impl Default for HopClass {
    fn default() -> Self {
        Self {
            kind: HopKind::Chat,
            transport: Transport::Http,
        }
    }
}

impl HopClass {
    /// OpenAI-shaped `/v1/…` media catalog. Any provider may reuse this.
    pub fn openai_compat(path: &str, upgrade: bool) -> Self {
        let p = path.split('?').next().unwrap_or(path).trim_end_matches('/');
        let kind = openai_compat_kind(path).unwrap_or_else(|| {
            if p == "/v1/models" {
                HopKind::Models
            } else {
                HopKind::Chat
            }
        });
        let transport =
            if upgrade && matches!(kind, HopKind::Stt | HopKind::Tts | HopKind::Realtime) {
                Transport::WebSocket
            } else {
                Transport::Http
            };
        Self { kind, transport }
    }
}

/// Media catalog (not chat, not `/v1/models`). `None` = leave on the provider default host.
pub fn openai_compat_kind(raw: &str) -> Option<HopKind> {
    let p = raw.split('?').next().unwrap_or(raw).trim_end_matches('/');
    match p {
        "/v1/images/generations" | "/v1/images/edits" => Some(HopKind::Image),
        "/v1/videos/generations" => Some(HopKind::Video),
        "/v1/tts" | "/v1/tts/voices" | "/v1/audio/speech" | "/v1/audio/voices" => {
            Some(HopKind::Tts)
        }
        "/v1/stt" | "/v1/audio/transcriptions" => Some(HopKind::Stt),
        "/v1/realtime" => Some(HopKind::Realtime),
        "/v1/custom-voices" => Some(HopKind::Tts),
        _ if p.starts_with("/v1/videos/") => Some(HopKind::Video),
        _ if p.starts_with("/v1/tts/") || p.starts_with("/v1/custom-voices/") => Some(HopKind::Tts),
        _ if p.starts_with("/v1/realtime/") => Some(HopKind::Realtime),
        _ => None,
    }
}

/// Intentionally small provider surface. Stored provider credentials must not
/// turn a proxy key into access to files, batches, fine-tunes, or account APIs.
pub fn core_request_allowed(method: &str, raw: &str) -> bool {
    let path = raw.split('?').next().unwrap_or(raw).trim_end_matches('/');
    (method.eq_ignore_ascii_case("POST")
        && matches!(path, "/v1/responses" | "/v1/chat/completions"))
        || (method.eq_ignore_ascii_case("GET") && path == "/v1/models")
}

fn hop_path(raw: &str) -> &str {
    raw.split('?').next().unwrap_or(raw).trim_end_matches('/')
}

/// Grok Build cli-chat-proxy auxiliaries (feedback, session signals, login).
/// Not account-management APIs (`/v1/files`, batches, fine-tunes, `/v1/me`).
/// `GET /v1/deployment/config` is excluded: grok.com can rewrite client endpoints.
pub fn cli_aux_request_allowed(method: &str, raw: &str) -> bool {
    let path = hop_path(raw);
    let get = method.eq_ignore_ascii_case("GET");
    let post = method.eq_ignore_ascii_case("POST");
    match path {
        "/v1/feedback" if post => true,
        "/v1/feedback/config" if get => true,
        "/v1/feedback/requests" if post => true,
        "/v1/user" if get => true,
        "/v1/settings" if get => true,
        "/v1/login-config" if get => true,
        "/v1/subagents/bundle" if get => true,
        "/v1/bundle/archive" if get => true,
        "/v1/consent/accept" if post => true,
        "/v1/privacy/coding-data-retention" if get || post => true,
        "/v1/traces" if get || post => true,
        "/v1/mcp/tools/call" if post => true,
        "/v1/mcp/tools/list" if get || post => true,
        _ if path.starts_with("/v1/feedback/requests/") => {
            (path.ends_with("/complete") || path.ends_with("/dismiss")) && post
        }
        _ if path.starts_with("/v1/sessions/") => {
            let rest = &path["/v1/sessions/".len()..];
            rest.split_once('/').is_some_and(|(id, action)| {
                !id.is_empty()
                    && !id.contains('/')
                    && post
                    && matches!(action, "signals" | "events" | "turn-deltas")
            })
        }
        _ => false,
    }
}

/// grok.com serves this without credentials so device-login can start.
pub fn cli_aux_public_unauthenticated(method: &str, raw: &str) -> bool {
    method.eq_ignore_ascii_case("GET") && hop_path(raw) == "/v1/login-config"
}

pub fn media_request_allowed(method: &str, raw: &str, upgrade: bool) -> bool {
    let path = raw.split('?').next().unwrap_or(raw).trim_end_matches('/');
    match path {
        "/v1/images/generations" | "/v1/images/edits" | "/v1/videos/generations" => {
            method.eq_ignore_ascii_case("POST")
        }
        "/v1/tts" | "/v1/stt" => {
            method.eq_ignore_ascii_case("POST") || (upgrade && method.eq_ignore_ascii_case("GET"))
        }
        "/v1/audio/speech" | "/v1/audio/transcriptions" => method.eq_ignore_ascii_case("POST"),
        "/v1/tts/voices" | "/v1/audio/voices" => method.eq_ignore_ascii_case("GET"),
        "/v1/realtime" => {
            method.eq_ignore_ascii_case("POST") || (upgrade && method.eq_ignore_ascii_case("GET"))
        }
        "/v1/custom-voices" => matches_ignore_ascii_case(method, &["GET", "POST"]),
        _ if path.starts_with("/v1/videos/") => {
            matches_ignore_ascii_case(method, &["GET", "DELETE"])
        }
        _ if path.starts_with("/v1/custom-voices/") => {
            matches_ignore_ascii_case(method, &["GET", "DELETE"])
        }
        _ if path.starts_with("/v1/realtime/") => upgrade && method.eq_ignore_ascii_case("GET"),
        _ => false,
    }
}

fn matches_ignore_ascii_case(value: &str, allowed: &[&str]) -> bool {
    allowed
        .iter()
        .any(|candidate| value.eq_ignore_ascii_case(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_stt_upgrades_to_websocket() {
        let c = HopClass::openai_compat("/v1/stt?encoding=pcm", true);
        assert_eq!(c.kind, HopKind::Stt);
        assert_eq!(c.transport, Transport::WebSocket);
        let http = HopClass::openai_compat("/v1/stt", false);
        assert_eq!(http.transport, Transport::Http);
    }

    #[test]
    fn chat_upgrade_stays_http() {
        let c = HopClass::openai_compat("/v1/responses", true);
        assert_eq!(c.kind, HopKind::Chat);
        assert_eq!(c.transport, Transport::Http);
    }

    #[test]
    fn openai_audio_aliases() {
        assert_eq!(openai_compat_kind("/v1/audio/speech"), Some(HopKind::Tts));
        assert_eq!(
            openai_compat_kind("/v1/audio/transcriptions"),
            Some(HopKind::Stt)
        );
        let c = HopClass::openai_compat("/v1/audio/speech", false);
        assert_eq!(c.kind, HopKind::Tts);
        assert_eq!(c.transport, Transport::Http);
    }

    #[test]
    fn tts_and_image_are_http_media() {
        assert_eq!(openai_compat_kind("/v1/tts"), Some(HopKind::Tts));
        assert_eq!(
            openai_compat_kind("/v1/images/generations"),
            Some(HopKind::Image)
        );
        assert!(HopKind::Tts.always_record());
        assert!(!HopKind::Chat.always_record());
    }

    #[test]
    fn provider_surface_default_denies_credential_management_apis() {
        assert!(core_request_allowed("POST", "/v1/responses"));
        assert!(core_request_allowed("GET", "/v1/models?limit=2"));
        for path in ["/v1/files", "/v1/batches", "/v1/fine_tuning/jobs", "/v1/me"] {
            assert!(!core_request_allowed("GET", path), "{path}");
            assert!(!core_request_allowed("DELETE", path), "{path}");
        }
        assert!(media_request_allowed(
            "POST",
            "/v1/images/generations",
            false
        ));
        assert!(!media_request_allowed(
            "DELETE",
            "/v1/images/generations",
            false
        ));
        assert!(media_request_allowed("GET", "/v1/stt", true));
        assert!(!media_request_allowed("GET", "/v1/stt", false));
    }

    #[test]
    fn cli_aux_allowlist_matches_grok_build_proxy_plane() {
        assert!(cli_aux_request_allowed("POST", "/v1/feedback"));
        assert!(cli_aux_request_allowed("GET", "/v1/feedback/config"));
        assert!(cli_aux_request_allowed(
            "POST",
            "/v1/feedback/requests/abc/complete"
        ));
        assert!(cli_aux_request_allowed(
            "POST",
            "/v1/sessions/01abc/signals"
        ));
        assert!(cli_aux_request_allowed(
            "GET",
            "/v1/user?include=subscription"
        ));
        assert!(cli_aux_request_allowed("GET", "/v1/login-config"));
        assert!(cli_aux_public_unauthenticated("GET", "/v1/login-config"));
        assert!(!cli_aux_public_unauthenticated("POST", "/v1/login-config"));
        for path in [
            "/v1/files",
            "/v1/batches",
            "/v1/fine_tuning/jobs",
            "/v1/me",
            "/v1/deployment/config",
            "/v1/responses",
            "/v1/models",
        ] {
            assert!(!cli_aux_request_allowed("GET", path), "{path}");
            assert!(!cli_aux_request_allowed("POST", path), "{path}");
        }
    }
}
