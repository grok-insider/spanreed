//! Claude (Anthropic) credential shapes, headers and plan usage.
//!
//! Claude Code stores a subscription session in `~/.claude/.credentials.json` under
//! `claudeAiOauth`: an opaque `sk-ant-oat…` access token plus a rotating refresh token.
//! That session bills a Pro/Max plan. An `sk-ant-api…` key reaching the same origin
//! bills API credits, so the two kinds need different credential headers.
//!
//! OAuth hops carry `Authorization: Bearer` and `anthropic-version`; the
//! `oauth-2025-04-20` beta header and `x-app: cli` are sent the way the official client
//! sends them. Verified against `api.anthropic.com` on 2026-09-25 with a Max 20x
//! session: `GET /v1/models` and `POST /v1/messages` answer 200 for the OAuth
//! credential, the same token as `x-api-key` answers 401, and the `claude-cli` user
//! agent is not required.

use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub use crate::oauth::{ensure_access, valid_access};

pub const ID: &str = "claude";
pub const NAME: &str = "Claude";
pub const API_BASE: &str = "https://api.anthropic.com";
pub const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
/// Subscription (claude.ai) authorization. The console flow is a different entry
/// point for API organizations, which the relay does not use.
pub const AUTHORIZE_URL: &str = "https://claude.com/cai/oauth/authorize";
/// Anthropic's manual redirect: the page shows a code the operator pastes back
/// instead of calling a loopback listener.
pub const MANUAL_REDIRECT_URL: &str = "https://platform.claude.com/oauth/code/callback";
pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const OAUTH_BETA: &str = "oauth-2025-04-20";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const OAUTH_TOKEN_PREFIX: &str = "sk-ant-oat";
pub const USAGE_PATH: &str = "/api/oauth/usage";
pub const PROFILE_PATH: &str = "/api/oauth/profile";
pub const MODELS_PATH: &str = "/v1/models";
/// Refresh asks only for what the relay uses; the CLI asks for more.
pub const SCOPES: &str = "user:inference user:profile";
pub const REFRESH_BUFFER_MS: i64 = 300_000;
const AUTHORIZE_URL_ENV: &str = "AI_RELAY_CLAUDE_AUTHORIZE_URL";
const TOKEN_URL_ENV: &str = "AI_RELAY_CLAUDE_TOKEN_URL";
const API_BASE_ENV: &str = "AI_RELAY_CLAUDE_API_BASE";

const MAX_TOKEN: usize = 16 * 1024;
const MAX_LABEL: usize = 120;
const MAX_CODE: usize = 4096;

fn env_origin(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
        .unwrap_or_else(|| default.trim_end_matches('/').to_string())
}

/// Pinned origins, overridable the same way the Grok adapter is for fixtures.
pub fn api_base() -> String {
    env_origin(API_BASE_ENV, API_BASE)
}

pub fn token_url() -> String {
    env_origin(TOKEN_URL_ENV, TOKEN_URL)
}

pub fn authorize_url_base() -> String {
    env_origin(AUTHORIZE_URL_ENV, AUTHORIZE_URL)
}

/// RFC 3986 unreserved characters only, so a redirect URI survives as one query value.
fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Authorization URL for the relay's own session. The PKCE verifier stays on the
/// host; only `state` and the challenge travel.
pub fn authorize_url(state: &str, code_challenge: &str) -> String {
    let mut url = authorize_url_base();
    let query = [
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", MANUAL_REDIRECT_URL),
        ("scope", SCOPES),
        ("state", state),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
    ]
    .iter()
    .map(|(name, value)| format!("{}={}", encode_component(name), encode_component(value)))
    .collect::<Vec<_>>()
    .join("&");
    url.push('?');
    url.push_str(&query);
    url
}

/// Request body for the authorization-code exchange, as the official client sends it.
pub fn exchange_body(code: &str, state: &str, code_verifier: &str) -> Value {
    json!({
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": MANUAL_REDIRECT_URL,
        "client_id": CLIENT_ID,
        "code_verifier": code_verifier,
        "state": state,
    })
}

/// Build the stored session from an authorization-code token response. A code
/// exchange always carries a refresh token: without one the session cannot rotate.
pub fn session_document(body: &Value, now: i64) -> Result<Value, String> {
    let access = body
        .get("access_token")
        .and_then(|value| graphic(value, MAX_TOKEN))
        .ok_or("Claude authorization returned no access token")?;
    let refresh = body
        .get("refresh_token")
        .and_then(|value| graphic(value, MAX_TOKEN))
        .ok_or("Claude authorization returned no refresh token")?;
    let mut document = json!({
        "type": "oauth",
        "access_token": access,
        "refresh_token": refresh,
    });
    let object = document
        .as_object_mut()
        .ok_or("Claude document is not an object")?;
    object.insert("expires_at_ms".into(), json!(expiry_from(body, now)));
    if let Some(seconds) = body.get("refresh_token_expires_in").and_then(Value::as_f64) {
        if seconds.is_finite() && seconds > 0.0 {
            object.insert(
                "refresh_expires_at_ms".into(),
                json!(now.saturating_add((seconds * 1000.0) as i64)),
            );
        }
    }
    validate(&document, now)?;
    Ok(document)
}

fn expiry_from(body: &Value, now: i64) -> i64 {
    body.get("expires_in")
        .and_then(Value::as_f64)
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .map(|seconds| now.saturating_add((seconds * 1000.0) as i64))
        .unwrap_or_else(|| now + 3_600_000)
}

/// Operator-facing message for a rejected code exchange.
pub fn authorization_rejection(body: &Value) -> String {
    match body.get("error").and_then(Value::as_str).unwrap_or("") {
        "invalid_grant" => "Claude rejected the authorization code; start again".into(),
        "invalid_request" | "invalid_client" | "unauthorized_client" => {
            "Claude rejected the authorization request".into()
        }
        _ => "Claude could not complete the authorization".into(),
    }
}

/// A pasted code must be a bounded, printable single token.
pub fn valid_authorization_code(code: &str) -> bool {
    let code = code.trim();
    !code.is_empty() && code.len() <= MAX_CODE && code.bytes().all(|byte| byte.is_ascii_graphic())
}

/// Credential headers for one Claude hop. OAuth sessions and API keys share the
/// origin but never the credential header.
pub fn headers(token: &str) -> Vec<(String, String)> {
    let token = token.trim();
    if is_oauth(token) {
        vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("anthropic-version".into(), ANTHROPIC_VERSION.into()),
            ("anthropic-beta".into(), OAUTH_BETA.into()),
            ("x-app".into(), "cli".into()),
        ]
    } else {
        vec![
            ("x-api-key".into(), token.to_string()),
            ("anthropic-version".into(), ANTHROPIC_VERSION.into()),
        ]
    }
}

pub fn is_oauth(token: &str) -> bool {
    token.trim().starts_with(OAUTH_TOKEN_PREFIX)
}

fn graphic(value: &Value, max: usize) -> Option<String> {
    let text = value.as_str()?.trim();
    (!text.is_empty() && text.len() <= max && text.bytes().all(|b| b.is_ascii_graphic()))
        .then(|| text.to_string())
}

/// Display text: printable, control characters refused. Names carry spaces.
fn label(value: &Value, max: usize) -> Option<String> {
    let text = value.as_str()?.trim();
    (!text.is_empty() && text.len() <= max && !text.chars().any(char::is_control))
        .then(|| text.to_string())
}

fn field(document: &Value, key: &str) -> Option<String> {
    graphic(document.get(key)?, MAX_LABEL)
}

pub fn refresh_token(document: &Value) -> Option<String> {
    document
        .get("refresh_token")
        .and_then(|value| graphic(value, MAX_TOKEN))
}

pub fn expiry_ms(document: &Value) -> Option<i64> {
    document.get("expires_at_ms")?.as_i64()
}

/// Stable subject for identity comparison: the account, else the organization.
pub fn identity(document: &Value) -> Option<String> {
    field(document, "account_uuid").or_else(|| field(document, "organization_uuid"))
}

pub fn email(document: &Value) -> Option<String> {
    field(document, "email")
}

pub fn subscription_type(document: &Value) -> Option<String> {
    field(document, "subscription_type")
}

pub fn rate_limit_tier(document: &Value) -> Option<String> {
    field(document, "rate_limit_tier")
}

/// `Max 20x` from the credential's own plan fields.
pub fn plan_label(document: &Value) -> Option<String> {
    plan_label_for(
        subscription_type(document).as_deref(),
        rate_limit_tier(document).as_deref(),
    )
}

fn plan_label_for(subscription: Option<&str>, tier: Option<&str>) -> Option<String> {
    let kind = subscription?.trim();
    if kind.is_empty() {
        return None;
    }
    let mut label = kind
        .chars()
        .enumerate()
        .map(|(index, character)| {
            if index == 0 {
                character.to_uppercase().to_string()
            } else {
                character.to_string()
            }
        })
        .collect::<String>();
    if let Some(multiplier) = tier_marker(tier) {
        label.push_str(&multiplier);
    }
    Some(label)
}

/// `default_claude_max_20x` → ` 20x`.
fn tier_marker(tier: Option<&str>) -> Option<String> {
    let tier = tier?;
    if !tier.contains('x') {
        return None;
    }
    tier.split(|character: char| !character.is_ascii_digit())
        .find(|part| !part.is_empty())
        .map(|digits| format!(" {digits}x"))
}

/// Plan slug and label from `GET /api/oauth/profile`.
pub fn plan_from_profile(profile: &Value) -> (Option<String>, Option<String>) {
    let organization = profile.get("organization").unwrap_or(profile);
    let slug = organization
        .get("organization_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .and_then(|value| value.strip_prefix("claude_"))
        .filter(|slug| !slug.is_empty() && slug.len() <= 32)
        .map(str::to_string);
    let tier = organization
        .get("rate_limit_tier")
        .and_then(Value::as_str)
        .map(str::to_string);
    let label = plan_label_for(slug.as_deref(), tier.as_deref());
    (slug, label)
}

pub fn email_from_profile(profile: &Value) -> Option<String> {
    field(profile.get("account")?, "email")
}

pub fn organization_name(profile: &Value) -> Option<String> {
    let organization = profile.get("organization").unwrap_or(profile);
    label(organization.get("name")?, MAX_LABEL)
}

pub fn validate(document: &Value, now: i64) -> Result<(), String> {
    if valid_access(document, now).is_none() {
        return Err("Invalid or expired Claude authorization".into());
    }
    if refresh_token(document).is_none() {
        return Err("Claude refresh token missing".into());
    }
    Ok(())
}

/// Accept the Claude Code credentials file (`claudeAiOauth`), a bare OAuth object, or
/// an already normalized document. Only fields the relay uses are kept.
pub fn document_from_credentials(value: &Value, now: i64) -> Result<Value, String> {
    let oauth = value.get("claudeAiOauth").unwrap_or(value);
    let access = oauth
        .get("accessToken")
        .or_else(|| oauth.get("access_token"))
        .and_then(|value| graphic(value, MAX_TOKEN))
        .ok_or("Claude access token missing")?;
    let refresh = oauth
        .get("refreshToken")
        .or_else(|| oauth.get("refresh_token"))
        .and_then(|value| graphic(value, MAX_TOKEN))
        .ok_or("Claude refresh token missing")?;
    let expires = oauth
        .get("expiresAt")
        .or_else(|| oauth.get("expires_at_ms"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let mut document = json!({
        "type": "oauth",
        "access_token": access,
        "refresh_token": refresh,
        "expires_at_ms": expires,
    });
    let object = document.as_object_mut().expect("object");
    for (source, target) in [
        ("refreshTokenExpiresAt", "refresh_expires_at_ms"),
        ("subscriptionType", "subscription_type"),
        ("rateLimitTier", "rate_limit_tier"),
        ("accountUuid", "account_uuid"),
        ("account_uuid", "account_uuid"),
        ("organizationUuid", "organization_uuid"),
        ("organization_uuid", "organization_uuid"),
        ("email", "email"),
    ] {
        if let Some(oauth) = oauth.get(source) {
            if target.ends_with("_ms") {
                if let Some(number) = oauth.as_i64() {
                    object.insert(target.into(), json!(number));
                }
                continue;
            }
            if let Some(text) = graphic(oauth, MAX_LABEL) {
                object.insert(target.into(), json!(text));
            }
        }
    }
    if !oauth.get("expiresAt").is_some_and(Value::is_i64) {
        // No usable expiry: force a refresh before the first hop.
        object.insert("expires_at_ms".into(), json!(now.min(0)));
    }
    Ok(document)
}

pub struct Client {
    http: reqwest::blocking::Client,
}

impl Client {
    pub fn new() -> Result<Self, String> {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map(|http| Self { http })
            .map_err(|_| "Claude client unavailable".into())
    }

    fn get(&self, path: &str, token: &str) -> Result<Value, String> {
        let mut request = self
            .http
            .get(format!("{}{path}", api_base()))
            .header("Accept", "application/json");
        for (name, value) in headers(token) {
            request = request.header(name, value);
        }
        let response = request.send().map_err(|_| "Claude request failed")?;
        let status = response.status().as_u16();
        if status == 401 || status == 403 {
            return Err("Claude rejected the credential".into());
        }
        if !(200..300).contains(&status) {
            return Err(format!("Claude /{path} HTTP {status}"));
        }
        response
            .json::<Value>()
            .map_err(|_| "Claude response was not JSON".to_string())
    }

    pub fn usage(&self, token: &str) -> Result<Value, String> {
        self.get(USAGE_PATH, token)
    }

    pub fn profile(&self, token: &str) -> Result<Value, String> {
        self.get(PROFILE_PATH, token)
    }

    pub fn models(&self, token: &str) -> Result<Vec<String>, String> {
        let body = self.get(&format!("{MODELS_PATH}?limit=1000"), token)?;
        crate::catalog::model_ids(&body)
    }

    /// Exchange an authorization code (with the host-held verifier) for a session.
    pub fn exchange_code(
        &self,
        code: &str,
        state: &str,
        code_verifier: &str,
        now: i64,
    ) -> Result<Value, String> {
        if !valid_authorization_code(code) {
            return Err("Claude authorization code is invalid".into());
        }
        let response = self
            .http
            .post(token_url())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&exchange_body(code.trim(), state, code_verifier))
            .send()
            .map_err(|_| "Claude authorization request failed")?;
        let status = response.status().as_u16();
        let body = response
            .json::<Value>()
            .map_err(|_| "Claude authorization response was not JSON".to_string())?;
        if matches!(status, 400 | 401 | 403) {
            return Err(authorization_rejection(&body));
        }
        if !(200..300).contains(&status) {
            return Err(format!("Claude authorization HTTP {status}"));
        }
        session_document(&body, now)
    }

    /// Refresh a session and return the updated document. The provider rotates the
    /// refresh token, so the caller must persist the replacement.
    pub fn refresh(&self, document: &Value, now: i64) -> Result<Value, String> {
        let refresh = refresh_token(document).ok_or("Claude refresh token missing")?;
        let payload = json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh,
            "client_id": CLIENT_ID,
            "scope": SCOPES,
        });
        let response = self
            .http
            .post(token_url())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&payload)
            .send()
            .map_err(|_| "Claude refresh failed")?;
        let status = response.status().as_u16();
        let body = response
            .json::<Value>()
            .map_err(|_| "Claude refresh response was not JSON".to_string())?;
        if status == 400 || status == 401 {
            return Err(refresh_rejection(&body));
        }
        if !(200..300).contains(&status) {
            return Err(format!("Claude refresh HTTP {status}"));
        }
        refreshed_document(document, &body, now)
    }

    /// Refresh when the access token is at or near expiry, then return it.
    pub fn ensure_access(&self, document: &mut Value, now: i64) -> Option<String> {
        ensure_access(document, now, |current, now| self.refresh(current, now))
    }
}

/// Apply a successful token response: rotated refresh token, new expiry, and a
/// fallback expiry for responses that omit `expires_in`.
pub fn refreshed_document(document: &Value, body: &Value, now: i64) -> Result<Value, String> {
    let access = body
        .get("access_token")
        .and_then(|value| graphic(value, MAX_TOKEN))
        .ok_or("Claude refresh returned no access token")?;
    let mut document = document.clone();
    let object = document
        .as_object_mut()
        .ok_or("Claude document is not an object")?;
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
    if let Some(seconds) = body.get("refresh_token_expires_in").and_then(Value::as_f64) {
        if seconds.is_finite() && seconds > 0.0 {
            object.insert(
                "refresh_expires_at_ms".into(),
                json!(now.saturating_add((seconds * 1000.0) as i64)),
            );
        }
    }
    Ok(document)
}

/// Operator-facing message for a rejected refresh.
pub fn refresh_rejection(body: &Value) -> String {
    let error = body.get("error").and_then(Value::as_str).unwrap_or("");
    if error == "invalid_grant" {
        "Claude session expired; authorize the account again".into()
    } else {
        "Claude refresh was rejected".into()
    }
}

/// `five_hour`, `seven_day`, legacy `seven_day_*` windows, structured `limits`
/// entries, and on-demand spend.
///
/// Plan windows stay `Session` and `Weekly`. Any other `limits` entry that
/// names a model or surface becomes one more quota line, using that name as
/// the label. A later provider can emit the same progress lines and the
/// account card will show them without a separate layout.
pub fn usage_output(usage: &Value, now_ms: i64) -> fabrials_model::ProviderOutput {
    use fabrials_model::ProviderOutput;

    let mut lines = Vec::new();
    for (key, label) in [("five_hour", "Session"), ("seven_day", "Weekly")] {
        if let Some(line) = window_line(usage, key, label) {
            lines.push(line);
        }
    }
    append_plan_windows_from_limits(usage, &mut lines);
    for (key, label) in [
        ("seven_day_opus", "Opus"),
        ("seven_day_sonnet", "Sonnet"),
        ("seven_day_omelette", "Claude Design"),
    ] {
        if let Some(line) = window_line(usage, key, label) {
            lines.push(line);
        }
    }
    let mut unknown = usage
        .as_object()
        .map(|object| {
            object
                .iter()
                .filter(|(key, value)| {
                    key.starts_with("seven_day_")
                        && !matches!(
                            key.as_str(),
                            "seven_day_opus" | "seven_day_sonnet" | "seven_day_omelette"
                        )
                        && window_percent(value).is_some()
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    unknown.sort();
    for key in unknown {
        let label = window_label(&key["seven_day_".len()..]);
        if has_progress_label(&lines, &label) {
            continue;
        }
        if let Some(line) = window_line(usage, &key, &label) {
            lines.push(line);
        }
    }
    append_scoped_limits(usage, &mut lines);
    if let Some(line) = extra_usage_line(usage) {
        lines.push(line);
    }
    if let Some(line) = plan_start_line(usage, now_ms) {
        lines.push(line);
    }
    ProviderOutput::new(ID, NAME, lines)
}

/// Session and weekly plan windows, when the flat fields are absent.
fn append_plan_windows_from_limits(usage: &Value, lines: &mut Vec<fabrials_model::MetricLine>) {
    let Some(limits) = usage.get("limits").and_then(Value::as_array) else {
        return;
    };
    if !has_progress_label(lines, "Session") {
        if let Some(line) = plan_limit_line(limits, "session", "Session") {
            lines.insert(0, line);
        }
    }
    if !has_progress_label(lines, "Weekly") {
        if let Some(line) = plan_limit_line(limits, "weekly_all", "Weekly") {
            let index = usize::from(has_progress_label(lines, "Session"));
            lines.insert(index, line);
        }
    }
}

fn plan_limit_line(
    limits: &[Value],
    kind: &str,
    label: &str,
) -> Option<fabrials_model::MetricLine> {
    let entry = limits.iter().find(|entry| {
        entry.get("kind").and_then(Value::as_str) == Some(kind) && scope_name(entry).is_none()
    })?;
    let percent = limit_percent(entry)?;
    let resets = entry.get("resets_at").and_then(iso_utc);
    Some(fabrials_model::MetricLine::percent(label, percent, resets))
}

/// Model and surface windows from `limits`, in payload order.
fn append_scoped_limits(usage: &Value, lines: &mut Vec<fabrials_model::MetricLine>) {
    let Some(limits) = usage.get("limits").and_then(Value::as_array) else {
        return;
    };
    for entry in limits {
        if plan_limit_entry(entry) {
            continue;
        }
        let Some(label) = scope_name(entry) else {
            continue;
        };
        if has_progress_label(lines, &label) {
            continue;
        }
        let Some(percent) = limit_percent(entry) else {
            continue;
        };
        let resets = entry.get("resets_at").and_then(iso_utc);
        lines.push(fabrials_model::MetricLine::percent(label, percent, resets));
    }
}

fn plan_limit_entry(entry: &Value) -> bool {
    let kind = entry.get("kind").and_then(Value::as_str).unwrap_or("");
    matches!(kind, "session" | "weekly_all") && scope_name(entry).is_none()
}

fn scope_name(entry: &Value) -> Option<String> {
    let scope = entry.get("scope")?;
    for pointer in ["/model/display_name", "/model/id"] {
        if let Some(name) = scope.pointer(pointer).and_then(Value::as_str) {
            if let Some(name) = clean_scope_name(name) {
                return Some(name);
            }
        }
    }
    scope
        .get("surface")
        .and_then(Value::as_str)
        .and_then(clean_scope_name)
}

fn clean_scope_name(value: &str) -> Option<String> {
    let name = value.trim();
    if name.is_empty() || name.len() > 80 || name.chars().any(|character| character.is_control()) {
        return None;
    }
    Some(name.to_string())
}

fn limit_percent(entry: &Value) -> Option<f64> {
    entry
        .get("percent")
        .and_then(Value::as_f64)
        .filter(|percent| percent.is_finite() && (0.0..=100.0).contains(percent))
}

fn has_progress_label(lines: &[fabrials_model::MetricLine], label: &str) -> bool {
    lines.iter().any(|line| match line {
        fabrials_model::MetricLine::Progress { label: found, .. } => {
            found.eq_ignore_ascii_case(label)
        }
        _ => false,
    })
}

fn window_percent(value: &Value) -> Option<f64> {
    value
        .get("utilization")
        .and_then(Value::as_f64)
        .filter(|percent| percent.is_finite() && (0.0..=100.0).contains(percent))
}

fn window_line(usage: &Value, key: &str, label: &str) -> Option<fabrials_model::MetricLine> {
    let window = usage.get(key)?;
    let percent = window_percent(window)?;
    let resets = window.get("resets_at").and_then(iso_utc);
    Some(fabrials_model::MetricLine::percent(label, percent, resets))
}

fn extra_usage_line(usage: &Value) -> Option<fabrials_model::MetricLine> {
    use fabrials_model::{MetricKind, MetricLine};
    let extra = usage.get("extra_usage")?;
    if extra.get("is_enabled").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let used = extra.get("used_credits").and_then(Value::as_f64)?;
    let limit = extra.get("monthly_limit").and_then(Value::as_f64)?;
    if !used.is_finite() || !limit.is_finite() || limit <= 0.0 {
        return None;
    }
    Some(MetricLine::dollars(
        MetricKind::Quota,
        "Extra usage spent",
        used / 100.0,
        limit / 100.0,
        None,
    ))
}

/// The OAuth session exposes the subscription start, not the period end.
fn plan_start_line(usage: &Value, now_ms: i64) -> Option<fabrials_model::MetricLine> {
    use fabrials_model::{MetricKind, MetricLine};
    let created = usage
        .get("subscription_created_at")
        .and_then(iso_utc)
        .and_then(|iso| OffsetDateTime::parse(&iso, &Rfc3339).ok())?;
    let now = OffsetDateTime::from_unix_timestamp_nanos((now_ms as i128) * 1_000_000).ok()?;
    let next = monthly_anniversary(created, now)?;
    Some(MetricLine::text(
        MetricKind::Plan,
        "Plan renews",
        format!(
            "{} · est.",
            next.format(&Rfc3339).unwrap_or_else(|_| "unknown".into())
        ),
    ))
}

/// Next monthly anniversary of `created` at or after `now`, clamped to short months.
fn monthly_anniversary(created: OffsetDateTime, now: OffsetDateTime) -> Option<OffsetDateTime> {
    let day = created.day();
    let mut year = now.year();
    let mut month = now.month();
    for _ in 0..24 {
        let date = time::Date::from_calendar_date(year, month, day.min(days_in_month(year, month)))
            .ok()?;
        let candidate = created.replace_date(date);
        if candidate >= now {
            return Some(candidate);
        }
        if month == time::Month::December {
            month = time::Month::January;
            year += 1;
        } else {
            month = month.next();
        }
    }
    None
}

fn days_in_month(year: i32, month: time::Month) -> u8 {
    let (next_year, next_month) = if month == time::Month::December {
        (year + 1, time::Month::January)
    } else {
        (year, month.next())
    };
    let first = time::Date::from_calendar_date(next_year, next_month, 1).ok();
    first
        .and_then(|date| date.previous_day())
        .map(|date| date.day())
        .unwrap_or(28)
}

fn window_label(suffix: &str) -> String {
    let label = suffix
        .split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if label.is_empty() {
        "Weekly".into()
    } else {
        label
    }
}

fn iso_utc(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();
    let parsed = OffsetDateTime::parse(text, &Rfc3339).ok()?;
    parsed.to_offset(time::UtcOffset::UTC).format(&Rfc3339).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabrials_model::{MetricLine, ProgressFormat};

    fn progress_labels(lines: &[MetricLine]) -> Vec<String> {
        lines
            .iter()
            .filter_map(|line| match line {
                MetricLine::Progress { label, .. } => Some(label.clone()),
                _ => None,
            })
            .collect()
    }

    fn progress(lines: &[MetricLine], label: &str) -> Option<(f64, f64, Option<String>)> {
        lines.iter().find_map(|line| match line {
            MetricLine::Progress {
                label: found,
                used,
                limit,
                resets_at,
                ..
            } if found == label => Some((*used, *limit, resets_at.clone())),
            _ => None,
        })
    }

    #[test]
    fn oauth_and_api_keys_use_different_credential_headers() {
        let oauth = headers("sk-ant-oat01-session");
        assert!(
            oauth
                .iter()
                .any(|(name, value)| name == "Authorization"
                    && value == "Bearer sk-ant-oat01-session")
        );
        assert!(oauth
            .iter()
            .any(|(name, value)| name == "anthropic-beta" && value == OAUTH_BETA));
        assert!(oauth
            .iter()
            .any(|(name, value)| name == "anthropic-version" && value == ANTHROPIC_VERSION));
        assert!(!oauth.iter().any(|(name, _)| name == "x-api-key"));

        let key = headers("sk-ant-api03-key");
        assert!(key
            .iter()
            .any(|(name, value)| name == "x-api-key" && value == "sk-ant-api03-key"));
        assert!(key
            .iter()
            .any(|(name, value)| name == "anthropic-version" && value == ANTHROPIC_VERSION));
        assert!(!key.iter().any(|(name, _)| name == "Authorization"));
    }

    #[test]
    fn credentials_file_becomes_a_normalized_document() {
        let file = json!({
            "mcpOAuth": {"ignored": true},
            "claudeAiOauth": {
                "accessToken": "sk-ant-oat01-access",
                "refreshToken": "sk-ant-ort01-refresh",
                "expiresAt": 1_790_344_247_168i64,
                "refreshTokenExpiresAt": 1_792_625_467_168i64,
                "scopes": ["user:inference"],
                "subscriptionType": "max",
                "rateLimitTier": "default_claude_max_20x",
            }
        });
        let document = document_from_credentials(&file, 0).unwrap();
        assert_eq!(document["access_token"], "sk-ant-oat01-access");
        assert_eq!(document["refresh_token"], "sk-ant-ort01-refresh");
        assert_eq!(document["expires_at_ms"], 1_790_344_247_168i64);
        assert_eq!(document["refresh_expires_at_ms"], 1_792_625_467_168i64);
        assert_eq!(document["subscription_type"], "max");
        assert_eq!(document["type"], "oauth");
        assert!(document.get("mcpOAuth").is_none());
        assert!(document.get("scopes").is_none());
        assert_eq!(plan_label(&document).as_deref(), Some("Max 20x"));
        validate(&document, 1_790_344_247_167).unwrap();
        assert!(validate(&document, 1_790_344_247_168).is_err());
    }

    #[test]
    fn credentials_without_expiry_force_a_refresh() {
        let file = json!({
            "accessToken": "sk-ant-oat01-access",
            "refreshToken": "sk-ant-ort01-refresh",
        });
        let document = document_from_credentials(&file, 1_700_000_000_000).unwrap();
        assert!(expiry_ms(&document).unwrap() <= 1_700_000_000_000);
        assert!(valid_access(&document, 1_700_000_000_000).is_none());
    }

    #[test]
    fn incomplete_credentials_are_refused() {
        assert!(document_from_credentials(&json!({"claudeAiOauth": {}}), 0).is_err());
        assert!(document_from_credentials(
            &json!({"claudeAiOauth": {"accessToken": "sk-ant-oat01-a"}}),
            0
        )
        .is_err());
        let missing_refresh = json!({
            "access_token": "sk-ant-oat01-a",
            "expires_at_ms": 5_000i64,
        });
        assert!(validate(&missing_refresh, 1_000).is_err());
    }

    #[test]
    fn window_lines_cover_session_weekly_models_and_extra_usage() {
        let usage = json!({
            "five_hour": {"utilization": 2.0, "resets_at": "2026-09-25T14:59:59.858958+00:00"},
            "seven_day": {"utilization": 27.0, "resets_at": "2026-09-30T19:59:59+00:00"},
            "seven_day_opus": {"utilization": 3.0},
            "seven_day_fable": {"utilization": 47.0},
            "seven_day_cowork": null,
            "extra_usage": {"is_enabled": true, "used_credits": 500.0, "monthly_limit": 10_000.0},
            "subscription_created_at": "2025-11-04T17:18:12.186065Z"
        });
        let output = usage_output(&usage, 1_790_344_247_168);
        assert_eq!(output.provider_id, "claude");
        let (used, limit, resets) = progress(&output.lines, "Session").unwrap();
        assert_eq!((used, limit), (2.0, 100.0));
        assert_eq!(resets.as_deref(), Some("2026-09-25T14:59:59.858958Z"));
        assert!(progress(&output.lines, "Weekly").is_some());
        assert!(progress(&output.lines, "Opus").is_some());
        assert!(progress(&output.lines, "Fable").is_some());
        assert!(progress(&output.lines, "Cowork").is_none());
        let (spent, cap, _) = progress(&output.lines, "Extra usage spent").unwrap();
        assert_eq!((spent, cap), (5.0, 100.0));
        assert!(matches!(
            output.lines.iter().find(|line| matches!(line, MetricLine::Progress { label, format: ProgressFormat::Dollars, .. } if label == "Extra usage spent")),
            Some(_)
        ));
        assert!(output.lines.iter().any(|line| matches!(
            line,
            MetricLine::Text { label, value, .. }
                if label == "Plan renews" && value.ends_with(" · est.")
        )));
    }

    #[test]
    fn structured_limits_add_named_model_windows_without_duplicating_plan_windows() {
        let usage = json!({
            "five_hour": {"utilization": 0.0, "resets_at": "2026-09-25T17:00:00+00:00"},
            "seven_day": {"utilization": 0.0, "resets_at": "2026-10-01T22:00:00+00:00"},
            "seven_day_opus": null,
            "seven_day_sonnet": null,
            "seven_day_fable": null,
            "limits": [
                {"kind": "session", "group": "session", "percent": 0, "resets_at": "2026-09-25T17:00:00+00:00", "scope": null, "is_active": true},
                {"kind": "weekly_all", "group": "weekly", "percent": 0, "resets_at": "2026-10-01T22:00:00+00:00", "scope": null, "is_active": false},
                {
                    "kind": "weekly_scoped",
                    "group": "weekly",
                    "percent": 0,
                    "resets_at": "2026-10-02T05:00:00+00:00",
                    "scope": {"model": {"id": null, "display_name": "Fable"}, "surface": null},
                    "is_active": false
                },
                {
                    "kind": "weekly_scoped",
                    "percent": 12,
                    "resets_at": "2026-10-02T05:00:00+00:00",
                    "scope": {"model": {"id": null, "display_name": "  "}, "surface": "cowork"}
                }
            ]
        });
        let output = usage_output(&usage, 0);
        let labels = progress_labels(&output.lines);
        assert_eq!(labels.iter().filter(|label| *label == "Session").count(), 1);
        assert_eq!(labels.iter().filter(|label| *label == "Weekly").count(), 1);
        let (used, limit, resets) = progress(&output.lines, "Fable").unwrap();
        assert_eq!((used, limit), (0.0, 100.0));
        assert_eq!(resets.as_deref(), Some("2026-10-02T05:00:00Z"));
        let (used, _, _) = progress(&output.lines, "cowork").unwrap();
        assert_eq!(used, 12.0);
        assert!(progress(&output.lines, "Opus").is_none());
    }

    #[test]
    fn structured_limits_fill_plan_windows_and_keep_a_scoped_session() {
        let usage = json!({
            "five_hour": null,
            "seven_day": null,
            "limits": [
                {"kind": "session", "percent": 4, "resets_at": "2026-09-25T14:59:59+00:00", "scope": null},
                {"kind": "weekly_all", "percent": 29, "resets_at": "2026-09-30T19:59:59+00:00", "scope": null},
                {
                    "kind": "session",
                    "percent": 8,
                    "resets_at": "2026-09-25T18:00:00+00:00",
                    "scope": {"model": {"id": null, "display_name": "Fable"}, "surface": null}
                },
                {"kind": "weekly_scoped", "percent": 140, "scope": {"model": {"display_name": "Dropped"}}},
                {"kind": "weekly_scoped", "percent": 3, "scope": {"model": {"display_name": ""}}}
            ]
        });
        let output = usage_output(&usage, 0);
        let (session, _, session_reset) = progress(&output.lines, "Session").unwrap();
        let (weekly, _, _) = progress(&output.lines, "Weekly").unwrap();
        let (fable, _, fable_reset) = progress(&output.lines, "Fable").unwrap();
        assert_eq!(session, 4.0);
        assert_eq!(session_reset.as_deref(), Some("2026-09-25T14:59:59Z"));
        assert_eq!(weekly, 29.0);
        assert_eq!(fable, 8.0);
        assert_eq!(fable_reset.as_deref(), Some("2026-09-25T18:00:00Z"));
        assert!(progress(&output.lines, "Dropped").is_none());
        assert_eq!(
            progress_labels(&output.lines),
            vec![
                "Session".to_string(),
                "Weekly".to_string(),
                "Fable".to_string()
            ]
        );
    }

    #[test]
    fn a_flat_model_window_is_not_repeated_from_limits() {
        let usage = json!({
            "seven_day_opus": {"utilization": 3.0, "resets_at": "2026-09-30T19:59:59+00:00"},
            "limits": [{
                "kind": "weekly_scoped",
                "percent": 9,
                "scope": {"model": {"display_name": "opus"}}
            }]
        });
        let output = usage_output(&usage, 0);
        let (used, _, _) = progress(&output.lines, "Opus").unwrap();
        assert_eq!(used, 3.0);
        assert_eq!(progress_labels(&output.lines), vec!["Opus".to_string()]);
    }

    #[test]
    fn windows_accept_percentages_from_strings_and_skip_missing_resets() {
        let usage = json!({
            "five_hour": {"utilization": "12.5"},
            "seven_day": {"utilization": 140.0},
            "seven_day_sonnet": {"utilization": 0.0, "resets_at": "not-a-date"},
        });
        let output = usage_output(&usage, 0);
        assert!(progress(&output.lines, "Session").is_none());
        assert!(progress(&output.lines, "Weekly").is_none());
        let (used, _, resets) = progress(&output.lines, "Sonnet").unwrap();
        assert_eq!(used, 0.0);
        assert!(resets.is_none());
    }

    #[test]
    fn profile_maps_organization_type_and_tier() {
        let profile = json!({
            "organization": {
                "name": "Example Organization",
                "organization_type": "claude_max",
                "rate_limit_tier": "default_claude_max_20x",
                "billing_type": "stripe_subscription",
                "subscription_status": "active"
            },
            "account": {"email": "owner@example.com"}
        });
        assert_eq!(
            plan_from_profile(&profile),
            (Some("max".into()), Some("Max 20x".into()))
        );
        assert_eq!(
            email_from_profile(&profile).as_deref(),
            Some("owner@example.com")
        );
        assert_eq!(
            organization_name(&profile).as_deref(),
            Some("Example Organization")
        );
        assert_eq!(plan_from_profile(&json!({})), (None, None));
    }

    #[test]
    fn refresh_rotates_the_session_and_keeps_the_plan() {
        let document = json!({
            "type": "oauth",
            "access_token": "sk-ant-oat01-old",
            "refresh_token": "sk-ant-ort01-old",
            "expires_at_ms": 1_000i64,
            "subscription_type": "max",
            "rate_limit_tier": "default_claude_max_20x",
            "catalog": {"ok": true, "models": ["claude-opus-5"]},
        });
        let body = json!({
            "access_token": "sk-ant-oat01-new",
            "refresh_token": "sk-ant-ort01-new",
            "expires_in": 3600,
            "refresh_token_expires_in": 1_209_600,
        });
        let refreshed = refreshed_document(&document, &body, 5_000).unwrap();
        assert_eq!(refreshed["access_token"], "sk-ant-oat01-new");
        assert_eq!(refreshed["refresh_token"], "sk-ant-ort01-new");
        assert_eq!(refreshed["expires_at_ms"], 5_000 + 3_600_000i64);
        assert_eq!(refreshed["refresh_expires_at_ms"], 5_000 + 1_209_600_000i64);
        assert_eq!(refreshed["subscription_type"], "max");
        assert_eq!(refreshed["catalog"]["models"][0], "claude-opus-5");
        assert_eq!(plan_label(&refreshed).as_deref(), Some("Max 20x"));
    }

    #[test]
    fn refresh_without_expiry_or_access_token_is_refused() {
        let document = json!({"type": "oauth", "access_token": "sk-ant-oat01-old", "refresh_token": "sk-ant-ort01-old", "expires_at_ms": 1});
        assert!(refreshed_document(
            &document,
            &json!({"refresh_token": "sk-ant-ort01-new"}),
            5_000
        )
        .is_err());
        let refreshed = refreshed_document(
            &document,
            &json!({"access_token": "sk-ant-oat01-new"}),
            5_000,
        )
        .unwrap();
        assert_eq!(refreshed["expires_at_ms"], 5_000 + 3_600_000i64);
        assert_eq!(refreshed["refresh_token"], "sk-ant-ort01-old");
    }

    #[test]
    fn refresh_rejections_are_operator_facing() {
        assert_eq!(
            refresh_rejection(&json!({"error": "invalid_grant"})),
            "Claude session expired; authorize the account again"
        );
        assert_eq!(
            refresh_rejection(&json!({"error": "rate_limited"})),
            "Claude refresh was rejected"
        );
        assert_eq!(refresh_rejection(&json!({})), "Claude refresh was rejected");
    }

    #[test]
    fn authorize_url_carries_the_relay_client_pkce_and_manual_redirect() {
        let url = authorize_url("state-value", "challenge-value");
        assert!(
            url.starts_with("https://claude.com/cai/oauth/authorize?"),
            "{url}"
        );
        for expected in [
            "response_type=code",
            &format!("client_id={CLIENT_ID}"),
            "redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback",
            "scope=user%3Ainference%20user%3Aprofile",
            "state=state-value",
            "code_challenge=challenge-value",
            "code_challenge_method=S256",
        ] {
            assert!(url.contains(expected), "{expected} missing from {url}");
        }
        assert!(!url.contains("code_verifier"), "{url}");
    }

    #[test]
    fn exchange_body_matches_the_official_client() {
        let body = exchange_body("code-1", "state-1", "verifier-1");
        assert_eq!(body["grant_type"], "authorization_code");
        assert_eq!(body["code"], "code-1");
        assert_eq!(body["state"], "state-1");
        assert_eq!(body["code_verifier"], "verifier-1");
        assert_eq!(body["client_id"], CLIENT_ID);
        assert_eq!(body["redirect_uri"], MANUAL_REDIRECT_URL);
    }

    #[test]
    fn session_document_keeps_both_tokens_and_the_expiry() {
        let document = session_document(
            &json!({
                "access_token": "sk-ant-oat01-new",
                "refresh_token": "sk-ant-ort01-new",
                "expires_in": 3600,
                "refresh_token_expires_in": 1_209_600,
                "scope": "user:inference user:profile",
            }),
            5_000,
        )
        .unwrap();
        assert_eq!(document["type"], "oauth");
        assert_eq!(document["access_token"], "sk-ant-oat01-new");
        assert_eq!(document["refresh_token"], "sk-ant-ort01-new");
        assert_eq!(document["expires_at_ms"], 5_000 + 3_600_000i64);
        assert_eq!(document["refresh_expires_at_ms"], 5_000 + 1_209_600_000i64);
        assert!(document.get("scope").is_none());
        assert!(valid_access(&document, 5_000).is_some());
    }

    #[test]
    fn a_code_exchange_without_a_refresh_token_is_refused() {
        let error = session_document(&json!({"access_token": "sk-ant-oat01-x"}), 0).unwrap_err();
        assert!(error.contains("no refresh token"), "{error}");
        assert!(session_document(&json!({"refresh_token": "sk-ant-ort01-x"}), 0).is_err());
        assert!(!valid_authorization_code(""));
        assert!(!valid_authorization_code(&"c".repeat(MAX_CODE + 1)));
        assert!(valid_authorization_code("abc123"));
    }

    #[test]
    fn authorization_rejections_are_operator_facing() {
        assert_eq!(
            authorization_rejection(&json!({"error": "invalid_grant"})),
            "Claude rejected the authorization code; start again"
        );
        assert_eq!(
            authorization_rejection(&json!({"error": "invalid_request"})),
            "Claude rejected the authorization request"
        );
        assert_eq!(
            authorization_rejection(&json!({})),
            "Claude could not complete the authorization"
        );
    }

    #[test]
    fn monthly_anniversary_clamps_short_months() {
        let created = OffsetDateTime::parse("2025-11-04T17:18:12Z", &Rfc3339).unwrap();
        let now = OffsetDateTime::parse("2026-09-25T10:00:00Z", &Rfc3339).unwrap();
        let next = monthly_anniversary(created, now).unwrap();
        assert_eq!(next.format(&Rfc3339).unwrap(), "2026-10-04T17:18:12Z");
        let january = OffsetDateTime::parse("2026-01-31T00:00:00Z", &Rfc3339).unwrap();
        let february = monthly_anniversary(january, january).unwrap();
        assert_eq!(february.format(&Rfc3339).unwrap(), "2026-01-31T00:00:00Z");
        let march = monthly_anniversary(
            january,
            OffsetDateTime::parse("2026-02-01T00:00:00Z", &Rfc3339).unwrap(),
        )
        .unwrap();
        assert_eq!(march.format(&Rfc3339).unwrap(), "2026-02-28T00:00:00Z");
    }
}
