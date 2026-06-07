//! Kimi Code provider.
//!
//! Token store: `~/.kimi/credentials/kimi-code.json`.
//! Usage: `GET https://api.kimi.com/coding/v1/usages`.
//! Refresh: `POST https://auth.kimi.com/api/oauth/token` (form-encoded).

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "kimi";
const NAME: &str = "Kimi Code";
const USAGE_URL: &str = "https://api.kimi.com/coding/v1/usages";
const REFRESH_URL: &str = "https://auth.kimi.com/api/oauth/token";
const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
const REFRESH_BUFFER_MS: i64 = 5 * 60 * 1000;

pub struct Kimi;

fn cred_path() -> std::path::PathBuf {
    creds::expand("~/.kimi/credentials/kimi-code.json")
}

fn expires_at_ms(auth: &serde_json::Value) -> Option<i64> {
    // `expires_at` is unix seconds (possibly fractional).
    let secs = auth.get("expires_at")?.as_f64()?;
    Some((secs * 1000.0) as i64)
}

fn refresh_if_needed(auth: &mut serde_json::Value) -> Result<(), String> {
    let now = util::now_ms();
    let due = match expires_at_ms(auth) {
        Some(exp) => now + REFRESH_BUFFER_MS >= exp,
        None => true,
    };
    if !due {
        return Ok(());
    }
    let refresh_token = auth
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let refresh_token = match refresh_token {
        Some(t) => t.to_string(),
        None => return Ok(()),
    };

    let body = format!(
        "client_id={}&grant_type=refresh_token&refresh_token={}",
        CLIENT_ID, refresh_token
    );
    let resp = Request::post(REFRESH_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()?;

    if resp.status == 401 || resp.status == 403 {
        return Err("Kimi session expired. Run `kimi login` again.".into());
    }
    if !(200..300).contains(&resp.status) {
        return Ok(());
    }
    let json = match resp.json() {
        Some(j) => j,
        None => return Ok(()),
    };
    let new_access = match json.get("access_token").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Ok(()),
    };

    if let Some(obj) = auth.as_object_mut() {
        obj.insert("access_token".into(), serde_json::json!(new_access));
        if let Some(rt) = json.get("refresh_token").and_then(|v| v.as_str()) {
            obj.insert("refresh_token".into(), serde_json::json!(rt));
        }
        if let Some(expires_in) = json.get("expires_in").and_then(|v| v.as_f64()) {
            obj.insert(
                "expires_at".into(),
                serde_json::json!((now as f64) / 1000.0 + expires_in),
            );
        }
    }
    let _ = std::fs::write(
        cred_path(),
        serde_json::to_string_pretty(auth).unwrap_or_default(),
    );
    Ok(())
}

/// Parse a string-or-number JSON field to f64.
fn numf(v: Option<&serde_json::Value>) -> Option<f64> {
    match v? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

impl Provider for Kimi {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        cred_path().exists()
    }

    fn probe(&self) -> ProviderOutput {
        let mut auth = match creds::read_json(&cred_path()) {
            Some(a) => a,
            None => return ProviderOutput::error(ID, NAME, "Not logged in. Run `kimi login`."),
        };

        if let Err(e) = refresh_if_needed(&mut auth) {
            return ProviderOutput::error(ID, NAME, e);
        }
        let token = match auth.get("access_token").and_then(|v| v.as_str()) {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => return ProviderOutput::error(ID, NAME, "No access token. Run `kimi login`."),
        };

        let resp = match Request::get(USAGE_URL)
            .bearer(&token)
            .header("Accept", "application/json")
            .send()
        {
            Ok(r) => r,
            Err(e) => return ProviderOutput::error(ID, NAME, e),
        };
        if resp.is_auth_error() {
            return ProviderOutput::error(ID, NAME, "Token rejected. Run `kimi login` again.");
        }
        if !(200..300).contains(&resp.status) {
            return ProviderOutput::error(ID, NAME, format!("usage request failed (HTTP {})", resp.status));
        }
        let data = match resp.json() {
            Some(d) => d,
            None => return ProviderOutput::error(ID, NAME, "usage response not valid JSON"),
        };

        let mut lines = Vec::new();

        // Overall (weekly) usage.
        if let Some(usage) = data.get("usage") {
            let limit = numf(usage.get("limit"));
            let remaining = numf(usage.get("remaining"));
            if let (Some(limit), Some(remaining)) = (limit, remaining) {
                if limit > 0.0 {
                    let used_pct = ((limit - remaining) / limit * 100.0).clamp(0.0, 100.0);
                    let resets = usage.get("resetTime").and_then(util::to_iso);
                    lines.push(MetricLine::percent("Weekly", used_pct, resets));
                }
            }
        }

        // First windowed quota = session (5h).
        if let Some(window) = data
            .get("limits")
            .and_then(|l| l.as_array())
            .and_then(|arr| arr.first())
            .and_then(|w| w.get("detail"))
        {
            let limit = numf(window.get("limit"));
            let remaining = numf(window.get("remaining"));
            if let (Some(limit), Some(remaining)) = (limit, remaining) {
                if limit > 0.0 {
                    let used_pct = ((limit - remaining) / limit * 100.0).clamp(0.0, 100.0);
                    let resets = window.get("resetTime").and_then(util::to_iso);
                    lines.insert(0, MetricLine::percent("Session", used_pct, resets));
                }
            }
        }

        if lines.is_empty() {
            return ProviderOutput::error(ID, NAME, "no usage windows returned");
        }

        let plan = data
            .get("user")
            .and_then(|u| u.get("membership"))
            .and_then(|m| m.get("level"))
            .and_then(|v| v.as_str())
            .map(|lvl| util::plan_label(&lvl.replace("LEVEL_", "").replace('_', " ")));

        ProviderOutput::new(ID, NAME, lines).with_plan(plan)
    }
}
