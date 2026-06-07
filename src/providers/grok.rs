//! xAI / Grok (Grok CLI / SuperGrok Build) provider.
//!
//! Reads the Grok CLI auth file `~/.grok/auth.json`, which is a map of entries
//! keyed by an account/client identifier. Each entry has `key` (the access
//! token, a JWT), optional `refresh_token`, and `expires_at`. We pick the first
//! usable entry, refresh proactively (or on 401), then query
//! `GET https://cli-chat-proxy.grok.com/v1/billing` with the special
//! `X-XAI-Token-Auth: xai-grok-cli` header.

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "grok";
const NAME: &str = "Grok";
const BILLING_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing";
const SETTINGS_URL: &str = "https://cli-chat-proxy.grok.com/v1/settings";
const REFRESH_URL: &str = "https://auth.x.ai/oauth2/token";
const TOKEN_AUTH_HEADER: &str = "xai-grok-cli";
const DEFAULT_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const REFRESH_BUFFER_MS: i64 = 5 * 60 * 1000;
const LOGIN_HINT: &str = "Grok auth expired. Run `grok login` again.";

pub struct Grok;

fn auth_path() -> std::path::PathBuf {
    creds::expand("~/.grok/auth.json")
}

/// One resolved auth entry plus the bookkeeping needed to refresh/persist it.
struct AuthState {
    /// The whole auth.json document (mutated on refresh).
    doc: serde_json::Value,
    /// The key of the entry in use.
    entry_key: String,
    /// Current access token.
    token: String,
}

fn entry_expires_at_ms(entry: &serde_json::Value) -> Option<i64> {
    let raw = entry.get("expires_at").or_else(|| entry.get("expires"))?;
    util::to_iso(raw).and_then(|iso| {
        time::OffsetDateTime::parse(&iso, &time::format_description::well_known::Rfc3339)
            .ok()
            .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
    })
}

fn needs_refresh(entry: &serde_json::Value, token: &str, now_ms: i64) -> bool {
    let entry_ms = entry_expires_at_ms(entry);
    let token_ms = util::jwt_exp_ms(token);
    let entry_due = entry_ms
        .map(|ms| now_ms + REFRESH_BUFFER_MS >= ms)
        .unwrap_or(false);
    let token_due = token_ms
        .map(|ms| now_ms + REFRESH_BUFFER_MS >= ms)
        .unwrap_or(false);
    entry_due || token_due
}

fn is_expired(entry: &serde_json::Value, token: &str, now_ms: i64) -> bool {
    let ms = util::jwt_exp_ms(token).or_else(|| entry_expires_at_ms(entry));
    match ms {
        Some(ms) => now_ms >= ms,
        None => false,
    }
}

fn read_refresh_token(entry: &serde_json::Value) -> Option<String> {
    for key in ["refresh_token", "refresh"] {
        if let Some(s) = entry.get(key).and_then(|v| v.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn read_client_id(entry_key: &str, entry: &serde_json::Value) -> String {
    if let Some(cid) = entry.get("oidc_client_id").and_then(|v| v.as_str()) {
        let cid = cid.trim();
        if !cid.is_empty() {
            return cid.to_string();
        }
    }
    // Some keys encode the client id as the last `::`-separated segment.
    if let Some(last) = entry_key.split("::").last() {
        let last = last.trim();
        if !last.is_empty() && last != entry_key {
            return last.to_string();
        }
    }
    DEFAULT_CLIENT_ID.to_string()
}

/// Attempt a refresh of `entry_key`; on success mutate the doc and return the
/// new token. Returns Err(message) on a hard auth failure.
fn refresh(doc: &mut serde_json::Value, entry_key: &str) -> Result<Option<String>, String> {
    let entry = match doc.get(entry_key) {
        Some(e) => e.clone(),
        None => return Ok(None),
    };
    let refresh_token = match read_refresh_token(&entry) {
        Some(t) => t,
        None => return Ok(None),
    };
    let client_id = read_client_id(entry_key, &entry);

    let body = format!(
        "grant_type=refresh_token&client_id={}&refresh_token={}",
        urlencode(&client_id),
        urlencode(&refresh_token)
    );
    let resp = Request::post(REFRESH_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()?;

    if resp.status == 400 || resp.status == 401 || resp.status == 403 {
        return Err(LOGIN_HINT.to_string());
    }
    if !(200..300).contains(&resp.status) {
        return Ok(None); // transient
    }
    let json = match resp.json() {
        Some(j) => j,
        None => return Ok(None),
    };
    let access = match json.get("access_token").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Ok(None),
    };

    // Mutate the entry in the doc.
    if let Some(obj) = doc.get_mut(entry_key).and_then(|v| v.as_object_mut()) {
        obj.insert("key".into(), serde_json::json!(access));
        if let Some(rt) = json.get("refresh_token").and_then(|v| v.as_str()) {
            if !rt.trim().is_empty() {
                obj.insert("refresh_token".into(), serde_json::json!(rt.trim()));
            }
        }
        let now = util::now_ms();
        let expires_at = json
            .get("expires_in")
            .and_then(|v| v.as_f64())
            .filter(|n| *n > 0.0)
            .map(|n| now + (n as i64) * 1000)
            .or_else(|| util::jwt_exp_ms(&access))
            .unwrap_or(now + 3600 * 1000);
        if let Some(iso) = util::ms_to_iso(expires_at) {
            obj.insert("expires_at".into(), serde_json::json!(iso));
        }
    }

    // Best-effort persist.
    if let Ok(text) = serde_json::to_string_pretty(doc) {
        let _ = std::fs::write(auth_path(), text);
    }
    Ok(Some(access))
}

/// Load the first usable auth entry, refreshing if needed.
fn load_auth() -> Result<AuthState, String> {
    let doc = creds::read_json(&auth_path())
        .ok_or_else(|| "Grok not logged in. Run `grok login`.".to_string())?;
    if !doc.is_object() {
        return Err("Grok not logged in. Run `grok login`.".into());
    }

    let now = util::now_ms();
    let keys: Vec<String> = doc.as_object().unwrap().keys().cloned().collect();
    let mut doc = doc;
    let mut expired_candidate = false;

    for entry_key in keys {
        let entry = doc
            .get(&entry_key)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if !entry.is_object() {
            continue;
        }
        let token = entry
            .get("key")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if token.is_empty() {
            continue;
        }

        if needs_refresh(&entry, &token, now) {
            match refresh(&mut doc, &entry_key)? {
                Some(new_token) => {
                    return Ok(AuthState {
                        doc,
                        entry_key,
                        token: new_token,
                    })
                }
                None => {
                    if !is_expired(&entry, &token, now) {
                        return Ok(AuthState {
                            doc,
                            entry_key,
                            token,
                        });
                    }
                    expired_candidate = true;
                    continue;
                }
            }
        }
        return Ok(AuthState {
            doc,
            entry_key,
            token,
        });
    }

    if expired_candidate {
        Err(LOGIN_HINT.into())
    } else {
        Err("Grok auth invalid. Run `grok login` again.".into())
    }
}

fn units(obj: Option<&serde_json::Value>) -> Option<f64> {
    obj?.get("val")?.as_f64()
}

fn fetch_billing(token: &str) -> Result<crate::http::Response, String> {
    Request::get(BILLING_URL)
        .bearer(token)
        .header("X-XAI-Token-Auth", TOKEN_AUTH_HEADER)
        .header("Accept", "application/json")
        .header("User-Agent", "spanreed")
        .send()
}

fn fetch_plan(token: &str) -> Option<String> {
    let resp = Request::get(SETTINGS_URL)
        .bearer(token)
        .header("X-XAI-Token-Auth", TOKEN_AUTH_HEADER)
        .header("Accept", "application/json")
        .header("User-Agent", "spanreed")
        .send()
        .ok()?;
    if !(200..300).contains(&resp.status) {
        return None;
    }
    let data = resp.json()?;
    data.get("subscription_tier_display")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl Provider for Grok {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        auth_path().exists()
    }

    fn probe(&self) -> ProviderOutput {
        let mut auth = match load_auth() {
            Ok(a) => a,
            Err(msg) => return ProviderOutput::error(ID, NAME, msg),
        };

        // Fetch billing, retrying once on auth error after a refresh.
        let mut resp = match fetch_billing(&auth.token) {
            Ok(r) => r,
            Err(_) => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "Grok billing request failed. Check your connection.",
                )
            }
        };
        if resp.is_auth_error() {
            match refresh(&mut auth.doc, &auth.entry_key) {
                Ok(Some(new_token)) => {
                    auth.token = new_token;
                    match fetch_billing(&auth.token) {
                        Ok(r) => resp = r,
                        Err(_) => {
                            return ProviderOutput::error(
                                ID,
                                NAME,
                                "Grok billing request failed. Check your connection.",
                            )
                        }
                    }
                }
                Ok(None) => return ProviderOutput::error(ID, NAME, LOGIN_HINT),
                Err(msg) => return ProviderOutput::error(ID, NAME, msg),
            }
        }

        if resp.is_auth_error() {
            return ProviderOutput::error(ID, NAME, LOGIN_HINT);
        }
        if !(200..300).contains(&resp.status) {
            return ProviderOutput::error(
                ID,
                NAME,
                format!(
                    "Grok billing request failed (HTTP {}). Try again later.",
                    resp.status
                ),
            );
        }

        let data = match resp.json() {
            Some(d) => d,
            None => return ProviderOutput::error(ID, NAME, "Grok billing response changed."),
        };
        let config = match data.get("config") {
            Some(c) if c.is_object() => c,
            _ => return ProviderOutput::error(ID, NAME, "Grok billing response changed."),
        };

        let lines = match parse_billing(config) {
            Some(l) => l,
            None => return ProviderOutput::error(ID, NAME, "Grok billing response changed."),
        };

        let plan = fetch_plan(&auth.token);
        ProviderOutput::new(ID, NAME, lines).with_plan(plan)
    }
}

/// Parse the billing `config` into Credits-used + Pay-as-you-go lines.
/// Returns None when required fields are missing/invalid.
fn parse_billing(config: &serde_json::Value) -> Option<Vec<MetricLine>> {
    let used = units(config.get("used"))?;
    let limit = units(config.get("monthlyLimit"))?;
    let on_demand_cap = units(config.get("onDemandCap"))?;
    if limit <= 0.0 {
        return None;
    }
    let resets_at = config.get("billingPeriodEnd").and_then(util::to_iso)?;

    let used_pct = (used / limit * 100.0).clamp(0.0, 100.0);
    let mut lines = vec![MetricLine::percent(
        "Credits used",
        used_pct,
        Some(resets_at),
    )];

    let payg = if on_demand_cap > 0.0 {
        format!("{} cap", on_demand_cap as i64)
    } else {
        "Disabled".to_string()
    };
    lines.push(MetricLine::Badge {
        label: "Pay as you go".into(),
        text: payg,
        color: Some(if on_demand_cap > 0.0 {
            "#22c55e".into()
        } else {
            "#a3a3a3".into()
        }),
        subtitle: None,
    });
    Some(lines)
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_credits_used_and_payg_disabled() {
        let config = serde_json::json!({
            "monthlyLimit": { "val": 60000 },
            "used": { "val": 15000 },
            "onDemandCap": { "val": 0 },
            "billingPeriodEnd": "2026-06-01T00:00:00+00:00"
        });
        let lines = parse_billing(&config).unwrap();
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Progress { label, used, .. } if label == "Credits used" && *used == 25.0)));
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Badge { label, text, .. } if label == "Pay as you go" && text == "Disabled")));
    }

    #[test]
    fn payg_cap_when_enabled() {
        let config = serde_json::json!({
            "monthlyLimit": { "val": 100 }, "used": { "val": 0 }, "onDemandCap": { "val": 500 },
            "billingPeriodEnd": "2026-06-01T00:00:00+00:00"
        });
        let lines = parse_billing(&config).unwrap();
        assert!(lines
            .iter()
            .any(|l| matches!(l, MetricLine::Badge { text, .. } if text == "500 cap")));
    }

    #[test]
    fn none_when_limit_missing() {
        assert!(parse_billing(&serde_json::json!({ "used": { "val": 1 } })).is_none());
    }
}
