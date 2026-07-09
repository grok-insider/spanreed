//! Claude (Claude Code) provider.
//!
//! Reads OAuth credentials from `~/.claude/.credentials.json` (or
//! `$CLAUDE_CONFIG_DIR`), refreshes if near expiry, then queries
//! `GET https://api.anthropic.com/api/oauth/usage`.
//!
//! Paid plan renew dates are **estimated**: Anthropic's OAuth profile exposes
//! `subscription_created_at` but not `current_period_end`. We assume a monthly
//! calendar cycle (30/31-day clamp) anchored on that create time and suffix
//! values with `est.`.
//!
//! On Linux, Claude Code stores credentials in the plaintext
//! `.credentials.json` file. When a Secret Service is available the credentials
//! may instead live under a `Claude Code-credentials` item, which we read via
//! `secret-tool` as a fallback.

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::secret;
use crate::util;

const ID: &str = "claude";
const NAME: &str = "Claude";
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
const REFRESH_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const SCOPES: &str =
    "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
const REFRESH_BUFFER_MS: i64 = 5 * 60 * 1000;
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

pub struct Claude;

struct Oauth {
    access_token: String,
    refresh_token: Option<String>,
    expires_at_ms: Option<i64>,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Source {
    File,
    Secret,
}

fn claude_home() -> std::path::PathBuf {
    if let Some(dir) = creds::env("CLAUDE_CONFIG_DIR") {
        return creds::expand(&dir);
    }
    creds::expand("~/.claude")
}

fn credentials_path() -> std::path::PathBuf {
    claude_home().join(".credentials.json")
}

fn parse_oauth(value: &serde_json::Value) -> Option<Oauth> {
    let oauth = value.get("claudeAiOauth")?;
    let access_token = oauth.get("accessToken")?.as_str()?.to_string();
    if access_token.is_empty() {
        return None;
    }
    Some(Oauth {
        access_token,
        refresh_token: oauth
            .get("refreshToken")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        expires_at_ms: oauth.get("expiresAt").and_then(|v| v.as_i64()),
        subscription_type: oauth
            .get("subscriptionType")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        rate_limit_tier: oauth
            .get("rateLimitTier")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

/// Load credentials, returning the parsed full JSON, the oauth view and source.
fn load_credentials() -> Option<(serde_json::Value, Oauth, Source)> {
    // 1) plaintext file (default on Linux)
    let path = credentials_path();
    if let Some(value) = creds::read_json(&path) {
        if let Some(oauth) = parse_oauth(&value) {
            return Some((value, oauth, Source::File));
        }
    }
    // 2) OS keyring fallback (some setups store the JSON blob there)
    if let Some(text) = secret::lookup(KEYCHAIN_SERVICE) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) {
            if let Some(oauth) = parse_oauth(&value) {
                return Some((value, oauth, Source::Secret));
            }
        }
    }
    None
}

fn needs_refresh(oauth: &Oauth, now_ms: i64) -> bool {
    match oauth.expires_at_ms {
        Some(exp) => now_ms + REFRESH_BUFFER_MS >= exp,
        None => true,
    }
}

/// Persist refreshed tokens back to the source we loaded from.
fn save_credentials(full: &serde_json::Value, source: Source) {
    let text = match serde_json::to_string(full) {
        Ok(t) => t,
        Err(_) => return,
    };
    match source {
        Source::File => {
            let _ = std::fs::write(credentials_path(), text);
        }
        Source::Secret => {
            // best-effort store; ignore failures
            let _ = secret::store(KEYCHAIN_SERVICE, "Claude Code", &text);
        }
    }
}

/// Returns a fresh access token (refreshing if needed). On hard auth failure
/// returns Err with a user-facing message.
fn refresh_if_needed(
    full: &mut serde_json::Value,
    oauth: &mut Oauth,
    source: Source,
) -> Result<(), String> {
    if !needs_refresh(oauth, util::now_ms()) {
        return Ok(());
    }
    let refresh_token = match &oauth.refresh_token {
        Some(t) if !t.is_empty() => t.clone(),
        _ => return Ok(()), // nothing to refresh with; try the existing token
    };

    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": CLIENT_ID,
        "scope": SCOPES,
    });
    let resp = Request::post(REFRESH_URL)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()?;

    if resp.status == 400 || resp.status == 401 {
        let err = resp
            .json()
            .and_then(|b| b.get("error").and_then(|e| e.as_str()).map(str::to_string));
        if err.as_deref() == Some("invalid_grant") {
            return Err("Session expired. Run `claude` to log in again.".into());
        }
        return Err("Token expired. Run `claude` to log in again.".into());
    }
    if !(200..300).contains(&resp.status) {
        return Ok(()); // transient; fall back to existing token
    }

    let json = match resp.json() {
        Some(j) => j,
        None => return Ok(()),
    };
    let new_access = match json.get("access_token").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Ok(()),
    };
    oauth.access_token = new_access.clone();
    if let Some(rt) = json.get("refresh_token").and_then(|v| v.as_str()) {
        oauth.refresh_token = Some(rt.to_string());
    }
    if let Some(expires_in) = json.get("expires_in").and_then(|v| v.as_f64()) {
        oauth.expires_at_ms = Some(util::now_ms() + (expires_in as i64) * 1000);
    }

    // Mirror updated tokens into the full doc and persist.
    if let Some(obj) = full
        .get_mut("claudeAiOauth")
        .and_then(|v| v.as_object_mut())
    {
        obj.insert("accessToken".into(), serde_json::json!(oauth.access_token));
        if let Some(rt) = &oauth.refresh_token {
            obj.insert("refreshToken".into(), serde_json::json!(rt));
        }
        if let Some(exp) = oauth.expires_at_ms {
            obj.insert("expiresAt".into(), serde_json::json!(exp));
        }
    }
    save_credentials(full, source);
    Ok(())
}

fn build_plan(oauth: &Oauth) -> Option<String> {
    let base = util::plan_label(oauth.subscription_type.as_deref()?);
    if base.is_empty() {
        return None;
    }
    // rateLimitTier like "default_5x" -> " 5x"
    let suffix = oauth
        .rate_limit_tier
        .as_deref()
        .and_then(|rlt| {
            rlt.split(|c: char| !c.is_ascii_digit())
                .find(|s| !s.is_empty())
                .filter(|_| rlt.contains('x'))
                .map(|n| format!(" {n}x"))
        })
        .unwrap_or_default();
    Some(format!("{base}{suffix}"))
}

fn window_line(data: &serde_json::Value, key: &str, label: &str) -> Option<MetricLine> {
    let win = data.get(key)?;
    let util_pct = win.get("utilization")?.as_f64()?;
    let resets = win.get("resets_at").and_then(util::to_iso);
    Some(MetricLine::percent(label, util_pct, resets))
}

/// Friendly labels for known per-model `seven_day_*` windows. Unknown ones
/// (new model families) are still rendered, with a label derived from the
/// key suffix (`seven_day_fable` -> "Fable").
const MODEL_WINDOWS: &[(&str, &str)] = &[
    ("seven_day_opus", "Opus"),
    ("seven_day_sonnet", "Sonnet"),
    ("seven_day_omelette", "Claude Design"),
];

fn window_label(suffix: &str) -> String {
    suffix
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn model_window_lines(data: &serde_json::Value) -> Vec<MetricLine> {
    let mut lines = Vec::new();
    for (key, label) in MODEL_WINDOWS {
        if let Some(l) = window_line(data, key, label) {
            lines.push(l);
        }
    }
    let Some(obj) = data.as_object() else {
        return lines;
    };
    let mut unknown: Vec<&str> = obj
        .keys()
        .map(String::as_str)
        .filter(|k| {
            k.starts_with("seven_day_") && !MODEL_WINDOWS.iter().any(|(known, _)| known == k)
        })
        .collect();
    unknown.sort_unstable();
    for key in unknown {
        let label = window_label(&key["seven_day_".len()..]);
        if let Some(l) = window_line(data, key, &label) {
            lines.push(l);
        }
    }
    lines
}

fn parse_usage(data: &serde_json::Value) -> Vec<MetricLine> {
    let mut lines = Vec::new();
    if let Some(l) = window_line(data, "five_hour", "Session") {
        lines.push(l);
    }
    if let Some(l) = window_line(data, "seven_day", "Weekly") {
        lines.push(l);
    }
    lines.extend(model_window_lines(data));

    if let Some(extra) = data.get("extra_usage") {
        if extra
            .get("is_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let used = extra.get("used_credits").and_then(|v| v.as_f64());
            let limit = extra.get("monthly_limit").and_then(|v| v.as_f64());
            if let (Some(used), Some(limit)) = (used, limit) {
                if limit > 0.0 {
                    lines.push(MetricLine::dollars(
                        "Extra usage spent",
                        util::cents_to_dollars(used),
                        util::cents_to_dollars(limit),
                        None,
                    ));
                }
            }
        }
    }
    lines
}

impl Provider for Claude {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        credentials_path().exists()
            || creds::env("CLAUDE_CODE_OAUTH_TOKEN").is_some()
            || secret::exists(KEYCHAIN_SERVICE)
    }

    fn probe(&self) -> ProviderOutput {
        // Inference-only token override (no refresh/persistence).
        if let Some(token) = creds::env("CLAUDE_CODE_OAUTH_TOKEN") {
            return match fetch_and_build(&token) {
                Ok((plan, lines)) => ProviderOutput::new(ID, NAME, lines).with_plan(plan),
                Err(msg) => ProviderOutput::error(ID, NAME, msg),
            };
        }

        let (mut full, mut oauth, source) = match load_credentials() {
            Some(t) => t,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "No credentials found. Run `claude` to log in.",
                )
            }
        };

        if let Err(msg) = refresh_if_needed(&mut full, &mut oauth, source) {
            return ProviderOutput::error(ID, NAME, msg);
        }

        let plan = build_plan(&oauth);
        match fetch_and_build(&oauth.access_token) {
            Ok((_, lines)) => ProviderOutput::new(ID, NAME, lines).with_plan(plan),
            Err(msg) => ProviderOutput::error(ID, NAME, msg),
        }
    }
}

/// Fetch usage with a token and build the lines (plan comes from creds, so the
/// returned plan here is always None — caller supplies it).
fn fetch_and_build(access_token: &str) -> Result<(Option<String>, Vec<MetricLine>), String> {
    let resp = Request::get(USAGE_URL)
        .bearer(access_token.trim())
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-code/2.1.69")
        .send()?;

    if resp.is_auth_error() {
        return Err("Token rejected. Run `claude` to log in again.".into());
    }
    if !(200..300).contains(&resp.status) {
        return Err(format!("usage request failed (HTTP {})", resp.status));
    }
    let data = resp.json().ok_or("usage response not valid JSON")?;
    let mut lines = parse_usage(&data);
    if lines.is_empty() {
        return Err("no usage windows returned".into());
    }
    // Estimated plan renew / last (soft-fail; OAuth has no period-end field).
    lines.extend(fetch_plan_period_lines(access_token));
    // Append local-log cost estimate (Last 30 Days + Usage Trend).
    lines.extend(crate::cost::cost_lines(crate::cost::Source::Claude));
    Ok((None, lines))
}

fn fetch_plan_period_lines(access_token: &str) -> Vec<MetricLine> {
    let resp = Request::get(PROFILE_URL)
        .bearer(access_token.trim())
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-code/2.1.69")
        .send()
        .ok();
    let Some(resp) = resp else {
        return Vec::new();
    };
    if !(200..300).contains(&resp.status) {
        return Vec::new();
    }
    let Some(data) = resp.json() else {
        return Vec::new();
    };
    parse_plan_period_lines(&data)
}

/// Infer monthly Plan renews / Last renew from oauth/profile (estimated).
fn parse_plan_period_lines(profile: &serde_json::Value) -> Vec<MetricLine> {
    let org = match profile.get("organization") {
        Some(o) => o,
        None => return Vec::new(),
    };
    let status = org
        .get("subscription_status")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !status.eq_ignore_ascii_case("active") {
        return Vec::new();
    }
    let billing = org
        .get("billing_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // stripe_subscription is the normal Pro/Max path; skip pure prepaid/API.
    if !billing.is_empty()
        && !billing.contains("subscription")
        && !billing.eq_ignore_ascii_case("stripe_subscription")
    {
        // Still allow empty billing_type if status is active.
        if billing == "prepaid" || billing == "invoice" {
            return Vec::new();
        }
    }
    let Some(created) = org
        .get("subscription_created_at")
        .and_then(util::to_iso)
        .and_then(|iso| util::parse_iso_dt(&iso))
    else {
        return Vec::new();
    };
    let now = time::OffsetDateTime::now_utc();
    let (last, next) = util::monthly_cycle_bounds(created, now);
    let mut lines = Vec::new();
    if let Some(next_iso) = util::offset_dt_to_iso(next) {
        lines.push(MetricLine::text(
            "Plan renews",
            util::format_plan_renew_value(&next_iso, true),
        ));
    }
    if let Some(last_iso) = util::offset_dt_to_iso(last) {
        lines.push(MetricLine::text(
            "Last renew",
            util::format_plan_last_value(&last_iso, true),
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProgressFormat;

    fn progress(lines: &[MetricLine], label: &str) -> Option<(f64, f64)> {
        lines.iter().find_map(|l| match l {
            MetricLine::Progress {
                label: lab,
                used,
                limit,
                ..
            } if lab == label => Some((*used, *limit)),
            _ => None,
        })
    }

    #[test]
    fn parses_windows_and_extra_usage() {
        let data = serde_json::json!({
            "five_hour": { "utilization": 25, "resets_at": "2026-01-28T15:00:00Z" },
            "seven_day": { "utilization": 40, "resets_at": "2026-02-01T00:00:00Z" },
            "seven_day_opus": { "utilization": 3, "resets_at": "2026-02-01T00:00:00Z" },
            "extra_usage": { "is_enabled": true, "used_credits": 500, "monthly_limit": 10000 }
        });
        let lines = parse_usage(&data);
        assert_eq!(progress(&lines, "Session"), Some((25.0, 100.0)));
        assert_eq!(progress(&lines, "Weekly"), Some((40.0, 100.0)));
        assert_eq!(progress(&lines, "Opus"), Some((3.0, 100.0)));
        // extra usage: 500c used / 10000c limit -> $5 / $100
        assert_eq!(progress(&lines, "Extra usage spent"), Some((5.0, 100.0)));
        // resets_at carried through on Session
        assert!(matches!(
            lines
                .iter()
                .find(|l| matches!(l, MetricLine::Progress { label, .. } if label == "Session")),
            Some(MetricLine::Progress {
                resets_at: Some(_),
                format: ProgressFormat::Percent,
                ..
            })
        ));
    }

    #[test]
    fn unknown_model_windows_render_with_derived_labels() {
        let data = serde_json::json!({
            "five_hour": { "utilization": 10 },
            "seven_day": { "utilization": 20 },
            "seven_day_sonnet": { "utilization": 0 },
            "seven_day_fable": { "utilization": 47, "resets_at": "2026-06-15T00:00:00Z" },
            "seven_day_omelette": { "utilization": 5 }
        });
        let lines = parse_usage(&data);
        assert_eq!(progress(&lines, "Fable"), Some((47.0, 100.0)));
        assert_eq!(progress(&lines, "Sonnet"), Some((0.0, 100.0)));
        // Known keys keep their friendly labels.
        assert_eq!(progress(&lines, "Claude Design"), Some((5.0, 100.0)));
        assert!(progress(&lines, "Omelette").is_none());
        // Multi-word suffixes title-case per word.
        assert_eq!(window_label("fable_mini"), "Fable Mini");
    }

    #[test]
    fn extra_usage_skipped_when_disabled() {
        let data = serde_json::json!({
            "five_hour": { "utilization": 1 },
            "extra_usage": { "is_enabled": false, "used_credits": 500, "monthly_limit": 10000 }
        });
        let lines = parse_usage(&data);
        assert!(progress(&lines, "Extra usage spent").is_none());
    }

    #[test]
    fn plan_includes_rate_limit_tier() {
        let oauth = Oauth {
            access_token: "x".into(),
            refresh_token: None,
            expires_at_ms: None,
            subscription_type: Some("max".into()),
            rate_limit_tier: Some("default_20x".into()),
        };
        assert_eq!(build_plan(&oauth).as_deref(), Some("Max 20x"));
    }

    #[test]
    fn plan_period_estimated_from_profile() {
        let created = "2025-07-17T15:18:17.382884Z";
        let profile = serde_json::json!({
            "organization": {
                "billing_type": "stripe_subscription",
                "subscription_status": "active",
                "subscription_created_at": created
            }
        });
        let lines = parse_plan_period_lines(&profile);
        assert!(lines.iter().any(|l| matches!(
            l,
            MetricLine::Text { label, value, .. }
            if label == "Plan renews" && value.contains(" · est.")
        )));
        assert!(lines.iter().any(|l| matches!(
            l,
            MetricLine::Text { label, value, .. }
            if label == "Last renew" && value.ends_with(" · est.")
        )));
        let anchor = util::parse_iso_dt(created).unwrap();
        let (last, next) = util::monthly_cycle_bounds(anchor, time::OffsetDateTime::now_utc());
        let expect_last =
            util::format_plan_last_value(&util::offset_dt_to_iso(last).unwrap(), true);
        let expect_renew =
            util::format_plan_renew_value(&util::offset_dt_to_iso(next).unwrap(), true);
        assert!(
            lines.iter().any(|l| matches!(
                l,
                MetricLine::Text { label, value, .. }
                if label == "Last renew" && value == &expect_last
            )),
            "lines={lines:?} expect_last={expect_last}"
        );
        assert!(
            lines.iter().any(|l| matches!(
                l,
                MetricLine::Text { label, value, .. }
                if label == "Plan renews" && value == &expect_renew
            )),
            "lines={lines:?} expect_renew={expect_renew}"
        );
    }

    #[test]
    fn plan_period_skipped_when_inactive() {
        let profile = serde_json::json!({
            "organization": {
                "billing_type": "stripe_subscription",
                "subscription_status": "canceled",
                "subscription_created_at": "2025-07-17T15:18:17Z"
            }
        });
        assert!(parse_plan_period_lines(&profile).is_empty());
    }
}
