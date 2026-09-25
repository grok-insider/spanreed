//! Kimi Code (Moonshot coding plan) credentials, headers and plan usage.
//!
//! `kimi login` stores a subscription session in `~/.kimi/credentials/kimi-code.json`:
//! an opaque access token plus a rotating refresh token (`expires_at` in unix seconds).
//! The coding plan is Anthropic-compatible at `https://api.kimi.com/coding`, so the
//! messages surface travels with `Authorization: Bearer` and the Anthropic version
//! header. Probed 2026-09-25: `POST /coding/v1/messages` and `GET /coding/v1/usages`
//! answer `401 invalid_authentication_error` without a token, which pins the paths.

use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub use crate::oauth::{ensure_access, valid_access};

pub const ID: &str = "kimi";
pub const NAME: &str = "Kimi Code";
pub const API_BASE: &str = "https://api.kimi.com/coding";
pub const AUTH_BASE: &str = "https://auth.kimi.com";
pub const TOKEN_URL: &str = "https://auth.kimi.com/api/oauth/token";
pub const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
pub const USAGE_PATH: &str = "/v1/usages";
pub const MODELS_PATH: &str = "/v1/models";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const REFRESH_BUFFER_MS: i64 = 300_000;

const MAX_TOKEN: usize = 16 * 1024;
const MAX_LABEL: usize = 120;

/// Endpoints the client talks to, chosen by the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origins {
    pub api_base: String,
    pub token_url: String,
}

impl Default for Origins {
    fn default() -> Self {
        Self {
            api_base: API_BASE.to_string(),
            token_url: TOKEN_URL.to_string(),
        }
    }
}

impl Origins {
    pub fn with_overrides(api_base: Option<&str>, token_url: Option<&str>) -> Self {
        let pinned = Self::default();
        Self {
            api_base: crate::origin_or(api_base, &pinned.api_base),
            token_url: crate::origin_or(token_url, &pinned.token_url),
        }
    }
}

fn graphic(value: &Value, max: usize) -> Option<String> {
    let text = value.as_str()?.trim();
    (!text.is_empty() && text.len() <= max && text.bytes().all(|b| b.is_ascii_graphic()))
        .then(|| text.to_string())
}

pub fn refresh_token(document: &Value) -> Option<String> {
    document
        .get("refresh_token")
        .and_then(|value| graphic(value, MAX_TOKEN))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_ms: i64,
}

/// Accept the `kimi-code.json` store, or an already normalized document.
pub fn document_from_credentials(value: &Value, now: i64) -> Result<Value, String> {
    let credential = credential_from_credentials(value, now)?;
    let mut document = json!({
        "type": "oauth",
        "access_token": credential.access_token,
        "refresh_token": credential.refresh_token,
        "expires_at_ms": credential.expires_at_ms,
    });
    if let Some(object) = document.as_object_mut() {
        if let Some(label) = value
            .get("plan_label")
            .and_then(|value| graphic(value, MAX_LABEL))
            .or_else(|| {
                value
                    .get("plan")
                    .and_then(|value| graphic(value, MAX_LABEL))
            })
        {
            object.insert("plan_label".into(), json!(label));
        }
    }
    Ok(document)
}

fn credential_from_credentials(value: &Value, _now: i64) -> Result<Credential, String> {
    let access_token = value
        .get("access_token")
        .and_then(|value| graphic(value, MAX_TOKEN))
        .ok_or("Kimi access token missing")?;
    let refresh_token = value
        .get("refresh_token")
        .and_then(|value| graphic(value, MAX_TOKEN))
        .ok_or("Kimi refresh token missing")?;
    let expires_at_ms = value
        .get("expires_at_ms")
        .and_then(Value::as_i64)
        .or_else(|| {
            // The CLI stores `expires_at` as fractional unix seconds.
            value
                .get("expires_at")
                .and_then(Value::as_f64)
                .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
                .map(|seconds| (seconds * 1000.0) as i64)
        })
        .ok_or("Kimi token expiry missing")?;
    Ok(Credential {
        access_token,
        refresh_token,
        expires_at_ms,
    })
}

pub fn validate(document: &Value, now: i64) -> Result<(), String> {
    if valid_access(document, now).is_none() {
        return Err("Invalid or expired Kimi authorization".into());
    }
    if refresh_token(document).is_none() {
        return Err("Kimi refresh token missing".into());
    }
    Ok(())
}

pub fn headers(token: &str) -> Vec<(String, String)> {
    vec![
        ("Authorization".into(), format!("Bearer {}", token.trim())),
        ("anthropic-version".into(), ANTHROPIC_VERSION.into()),
    ]
}

/// Form-encoded body the CLI sends on refresh.
pub fn refresh_body(refresh_token: &str) -> String {
    format!(
        "client_id={}&grant_type=refresh_token&refresh_token={}",
        CLIENT_ID, refresh_token
    )
}

/// Apply a token response, keeping the previous refresh token when the reply omits one.
pub fn refreshed_document(document: &Value, body: &Value, now: i64) -> Result<Value, String> {
    let access = body
        .get("access_token")
        .and_then(|value| graphic(value, MAX_TOKEN))
        .ok_or("Kimi refresh returned no access token")?;
    let mut document = document.clone();
    let object = document
        .as_object_mut()
        .ok_or("Kimi document is not an object")?;
    object.insert("access_token".into(), json!(access));
    object.insert("type".into(), json!("oauth"));
    if let Some(refresh) = body
        .get("refresh_token")
        .and_then(|value| graphic(value, MAX_TOKEN))
    {
        object.insert("refresh_token".into(), json!(refresh));
    }
    if let Some(seconds) = body.get("expires_in").and_then(Value::as_f64) {
        if seconds.is_finite() && seconds > 0.0 {
            object.insert(
                "expires_at_ms".into(),
                json!(now.saturating_add((seconds * 1000.0) as i64)),
            );
        }
    }
    if object
        .get("expires_at_ms")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        <= now
    {
        object.insert("expires_at_ms".into(), json!(now + 3_600_000));
    }
    Ok(document)
}

/// Operator-facing message for a rejected refresh.
pub fn refresh_rejection(status: u16) -> String {
    if status == 401 || status == 403 {
        "Kimi session expired; sign in again".into()
    } else {
        format!("Kimi refresh HTTP {status}")
    }
}

pub struct Client {
    http: std::sync::Arc<dyn crate::http::HttpPort>,
    origins: Origins,
}

impl Client {
    #[cfg(feature = "reqwest")]
    pub fn new() -> Result<Self, String> {
        Self::with_origins(Origins::default())
    }

    #[cfg(feature = "reqwest")]
    pub fn with_origins(origins: Origins) -> Result<Self, String> {
        crate::http::default_port(30)
            .map(|http| Self::with_http(http, origins))
            .map_err(|_| "Kimi client unavailable".into())
    }

    pub fn with_http(http: std::sync::Arc<dyn crate::http::HttpPort>, origins: Origins) -> Self {
        Self { http, origins }
    }

    pub fn origins(&self) -> &Origins {
        &self.origins
    }

    fn get(&self, path: &str, token: &str) -> Result<Value, String> {
        let owned = headers(token);
        let mut pairs: Vec<(&str, &str)> = vec![("Accept", "application/json")];
        pairs.extend(owned.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        let response = self
            .http
            .get(&format!("{}{path}", self.origins.api_base), &pairs)
            .map_err(|_| "Kimi request failed")?;
        let status = response.status;
        if status == 401 || status == 403 {
            return Err("Kimi rejected the credential".into());
        }
        if !(200..300).contains(&status) {
            return Err(format!("Kimi /{path} HTTP {status}"));
        }
        response
            .json()
            .map_err(|_| "Kimi response was not JSON".to_string())
    }

    pub fn usage(&self, token: &str) -> Result<Value, String> {
        self.get(USAGE_PATH, token)
    }

    pub fn models(&self, token: &str) -> Result<Vec<String>, String> {
        let body = self.get(MODELS_PATH, token)?;
        crate::catalog::model_ids(&body)
    }

    /// Refresh a session and return the updated document.
    pub fn refresh(&self, document: &Value, now: i64) -> Result<Value, String> {
        let refresh = refresh_token(document).ok_or("Kimi refresh token missing")?;
        let response = self
            .http
            .post_form(
                &self.origins.token_url,
                &[("Accept", "application/json")],
                &refresh_body(&refresh),
            )
            .map_err(|_| "Kimi refresh failed")?;
        let status = response.status;
        let body = response
            .json()
            .map_err(|_| "Kimi refresh response was not JSON".to_string())?;
        if !(200..300).contains(&status) {
            return Err(refresh_rejection(status));
        }
        refreshed_document(document, &body, now)
    }

    /// Refresh when the access token is at or near expiry, then return it.
    pub fn ensure_access(&self, document: &mut Value, now: i64) -> Option<String> {
        ensure_access(document, now, |current, now| self.refresh(current, now))
    }
}

fn numf(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn iso_utc(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();
    let parsed = OffsetDateTime::parse(text, &Rfc3339).ok()?;
    parsed.to_offset(time::UtcOffset::UTC).format(&Rfc3339).ok()
}

/// `/v1/usages`: the first windowed quota is the session, `usage` is the weekly pool.
pub fn usage_output(usage: &Value, _now_ms: i64) -> fabrials_types::ProviderOutput {
    use fabrials_types::ProviderOutput;

    let mut lines = Vec::new();
    if let Some(window) = usage
        .get("limits")
        .and_then(Value::as_array)
        .and_then(|limits| limits.first())
        .and_then(|first| first.get("detail"))
    {
        if let Some(line) = percent_line("Session", window) {
            lines.push(line);
        }
    }
    if let Some(line) = percent_line("Weekly", &usage["usage"]) {
        lines.push(line);
    }
    let mut output = ProviderOutput::new(ID, NAME, lines);
    output.plan = plan_slug(usage);
    output
}

fn percent_line(label: &str, window: &Value) -> Option<fabrials_types::MetricLine> {
    let limit = numf(window.get("limit"))?;
    let remaining = numf(window.get("remaining"))?;
    if !limit.is_finite() || limit <= 0.0 || !remaining.is_finite() {
        return None;
    }
    let used = ((limit - remaining) / limit * 100.0).clamp(0.0, 100.0);
    Some(fabrials_types::MetricLine::percent(
        label,
        used,
        window.get("resetTime").and_then(iso_utc),
    ))
}

/// `LEVEL_INTERMEDIATE` → `Intermediate`.
pub fn plan_label(usage: &Value) -> Option<String> {
    let level = membership_level(usage)?;
    let words = level
        .strip_prefix("LEVEL_")
        .unwrap_or(level)
        .split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let lower = word.to_ascii_lowercase();
            let mut characters = lower.chars();
            match characters.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    (!words.is_empty() && words.len() <= MAX_LABEL).then_some(words)
}

/// `LEVEL_INTERMEDIATE` → `intermediate`.
pub fn plan_slug(usage: &Value) -> Option<String> {
    let level = membership_level(usage)?;
    let slug = level
        .strip_prefix("LEVEL_")
        .unwrap_or(level)
        .to_ascii_lowercase()
        .replace('_', "-");
    (!slug.is_empty() && slug.len() <= MAX_LABEL).then_some(slug)
}

fn membership_level(usage: &Value) -> Option<&str> {
    usage
        .get("user")
        .and_then(|user| user.get("membership"))
        .and_then(|membership| membership.get("level"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|level| !level.is_empty())
}

/// The [`crate::subscription::SubscriptionProbe`] for this provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct Subscription;

impl crate::subscription::SubscriptionProbe for Subscription {
    fn provider_id(&self) -> &'static str {
        ID
    }
    fn is_subscription(&self, document: &Value) -> bool {
        document
            .get("access_token")
            .and_then(Value::as_str)
            .is_some_and(|token| !token.trim().is_empty())
    }
    fn usage_output(&self, usage: &Value, now_ms: i64) -> fabrials_types::ProviderOutput {
        usage_output(usage, now_ms)
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use fabrials_types::MetricLine;

    fn progress(lines: &[MetricLine], label: &str) -> Option<(f64, Option<String>)> {
        lines.iter().find_map(|line| match line {
            MetricLine::Progress {
                label: found,
                used,
                resets_at,
                ..
            } if found == label => Some((*used, resets_at.clone())),
            _ => None,
        })
    }

    #[test]
    fn credentials_store_becomes_a_normalized_document() {
        let file = json!({
            "access_token": "kimi-access",
            "refresh_token": "kimi-refresh",
            "expires_at": 1_790_334_247.168,
            "scope": "coding",
        });
        let document = document_from_credentials(&file, 0).unwrap();
        assert_eq!(document["type"], "oauth");
        assert_eq!(document["access_token"], "kimi-access");
        assert_eq!(document["refresh_token"], "kimi-refresh");
        assert_eq!(document["expires_at_ms"], 1_790_334_247_168i64);
        assert!(document.get("scope").is_none());
        validate(&document, 1_790_334_247_167).unwrap();
        assert!(validate(&document, 1_790_334_247_168).is_err());
        // An already normalized document round-trips.
        let again = document_from_credentials(&document, 0).unwrap();
        assert_eq!(again, document);
    }

    #[test]
    fn incomplete_credentials_are_refused() {
        assert!(document_from_credentials(&json!({"access_token": "a"}), 0).is_err());
        assert!(
            document_from_credentials(&json!({"access_token": "a", "refresh_token": "b"}), 0)
                .is_err()
        );
        assert!(
            document_from_credentials(&json!({"refresh_token": "b", "expires_at": 1.0}), 0)
                .is_err()
        );
    }

    #[test]
    fn refresh_rotates_the_session_and_keeps_the_plan() {
        let document = json!({
            "type": "oauth",
            "access_token": "kimi-old",
            "refresh_token": "kimi-refresh-old",
            "expires_at_ms": 1_000i64,
            "plan_label": "Intermediate",
            "catalog": {"ok": true},
        });
        let refreshed = refreshed_document(
            &document,
            &json!({
                "access_token": "kimi-new",
                "refresh_token": "kimi-refresh-new",
                "expires_in": 3600,
            }),
            5_000,
        )
        .unwrap();
        assert_eq!(refreshed["access_token"], "kimi-new");
        assert_eq!(refreshed["refresh_token"], "kimi-refresh-new");
        assert_eq!(refreshed["expires_at_ms"], 5_000 + 3_600_000i64);
        assert_eq!(refreshed["plan_label"], "Intermediate");
        assert_eq!(refreshed["catalog"]["ok"], true);
        // A reply without a new refresh token keeps the previous one.
        let kept =
            refreshed_document(&document, &json!({"access_token": "kimi-new"}), 5_000).unwrap();
        assert_eq!(kept["refresh_token"], "kimi-refresh-old");
        assert!(refreshed_document(&document, &json!({}), 5_000).is_err());
    }

    #[test]
    fn refresh_body_matches_the_cli() {
        let body = refresh_body("kimi-refresh");
        assert!(body.contains("grant_type=refresh_token"));
        assert!(body.contains(&format!("client_id={CLIENT_ID}")));
        assert!(body.contains("refresh_token=kimi-refresh"));
    }

    #[test]
    fn credential_headers_use_the_anthropic_surface() {
        let headers = headers(" kimi-access ");
        assert!(headers
            .iter()
            .any(|(name, value)| name == "Authorization" && value == "Bearer kimi-access"));
        assert!(headers
            .iter()
            .any(|(name, value)| name == "anthropic-version" && value == ANTHROPIC_VERSION));
    }

    #[test]
    fn usage_lines_cover_session_and_weekly_with_string_numbers() {
        let usage = json!({
            "usage": {"limit": "100", "remaining": "74", "resetTime": "2026-02-11T17:32:50.757941Z"},
            "limits": [{"window": {"duration": 300}, "detail": {"limit": "100", "remaining": "85", "resetTime": "2026-02-07T12:32:50Z"}}],
            "user": {"membership": {"level": "LEVEL_INTERMEDIATE"}}
        });
        let output = usage_output(&usage, 0);
        assert_eq!(output.provider_id, "kimi");
        assert_eq!(output.plan.as_deref(), Some("intermediate"));
        assert_eq!(plan_label(&usage).as_deref(), Some("Intermediate"));
        assert_eq!(plan_slug(&usage).as_deref(), Some("intermediate"));
        let (session, session_reset) = progress(&output.lines, "Session").unwrap();
        assert_eq!(session, 15.0);
        assert_eq!(session_reset.as_deref(), Some("2026-02-07T12:32:50Z"));
        let (weekly, _) = progress(&output.lines, "Weekly").unwrap();
        assert_eq!(weekly, 26.0);
        assert!(matches!(
            output.lines.first(),
            Some(MetricLine::Progress { label, .. }) if label == "Session"
        ));
    }

    #[test]
    fn usage_without_windows_yields_an_empty_output() {
        let output = usage_output(&json!({}), 0);
        assert!(output.lines.is_empty());
        assert!(output.plan.is_none());
    }
}
