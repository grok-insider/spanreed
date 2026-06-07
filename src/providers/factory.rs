//! Factory (Droid) provider.
//!
//! Auth file: `~/.factory/auth.json` (plain JSON with access/refresh tokens).
//! (The encrypted `auth.encrypted` / `auth.v2.file` variants are not supported.)
//! Refresh: WorkOS `POST https://api.workos.com/user_management/authenticate`.
//! Usage: `POST https://api.factory.ai/api/organization/subscription/usage`.

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProgressFormat, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "factory";
const NAME: &str = "Factory";
const USAGE_URL: &str = "https://api.factory.ai/api/organization/subscription/usage";
const WORKOS_URL: &str = "https://api.workos.com/user_management/authenticate";
const WORKOS_CLIENT_ID: &str = "client_01HNM792M5G5G1A2THWPXKFMXB";

pub struct Factory;

fn auth_path() -> std::path::PathBuf {
    creds::expand("~/.factory/auth.json")
}

fn read_tokens(v: &serde_json::Value) -> (Option<String>, Option<String>) {
    let access = v
        .get("access_token")
        .or_else(|| v.get("tokens").and_then(|t| t.get("access_token")))
        .or_else(|| v.get("tokens").and_then(|t| t.get("accessToken")))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let refresh = v
        .get("refresh_token")
        .or_else(|| v.get("refreshToken"))
        .or_else(|| v.get("tokens").and_then(|t| t.get("refresh_token")))
        .or_else(|| v.get("tokens").and_then(|t| t.get("refreshToken")))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    (access, refresh)
}

fn token_valid(access: &str) -> bool {
    match util::jwt_exp_ms(access) {
        Some(exp) => util::now_ms() < exp,
        None => true, // can't tell; assume usable
    }
}

fn refresh(auth: &mut serde_json::Value, refresh_token: &str) -> Result<Option<String>, String> {
    let body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}",
        urlencode(refresh_token),
        urlencode(WORKOS_CLIENT_ID)
    );
    let resp = Request::post(WORKOS_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()?;
    if resp.status == 400 || resp.status == 401 || resp.status == 403 {
        return Err("Factory session expired. Run `droid` to log in again.".into());
    }
    if !(200..300).contains(&resp.status) {
        return Ok(None);
    }
    let json = match resp.json() {
        Some(j) => j,
        None => return Ok(None),
    };
    let new_access = match json.get("access_token").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Ok(None),
    };
    if let Some(obj) = auth.as_object_mut() {
        obj.insert("access_token".into(), serde_json::json!(new_access));
        if let Some(rt) = json.get("refresh_token").and_then(|v| v.as_str()) {
            obj.insert("refresh_token".into(), serde_json::json!(rt));
        }
    }
    let _ = std::fs::write(
        auth_path(),
        serde_json::to_string_pretty(auth).unwrap_or_default(),
    );
    Ok(Some(new_access))
}

fn token_line(usage: &serde_json::Value, key: &str, label: &str) -> Option<MetricLine> {
    let block = usage.get(key)?;
    let limit = block.get("totalAllowance")?.as_f64()?;
    if limit <= 0.0 {
        return None;
    }
    let used = block
        .get("orgTotalTokensUsed")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let resets = usage.get("endDate").and_then(util::to_iso);
    Some(MetricLine::Progress {
        label: label.into(),
        used,
        limit,
        format: ProgressFormat::Count {
            suffix: "tokens".into(),
        },
        resets_at: resets,
        color: None,
    })
}

impl Provider for Factory {
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
        let mut auth = match creds::read_json(&auth_path()) {
            Some(a) => a,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "No credentials found. Run `droid` to log in.",
                )
            }
        };

        let (access, refresh_token) = read_tokens(&auth);
        let mut access = match access {
            Some(a) => a,
            None => return ProviderOutput::error(ID, NAME, "No access token in auth file."),
        };

        if !token_valid(&access) {
            if let Some(rt) = &refresh_token {
                match refresh(&mut auth, rt) {
                    Ok(Some(new)) => access = new,
                    Ok(None) => {}
                    Err(e) => return ProviderOutput::error(ID, NAME, e),
                }
            }
        }

        let resp = match Request::post(USAGE_URL)
            .bearer(&access)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(r#"{"useCache":true}"#)
            .send()
        {
            Ok(r) => r,
            Err(e) => return ProviderOutput::error(ID, NAME, e),
        };
        if resp.is_auth_error() {
            return ProviderOutput::error(ID, NAME, "Token rejected. Run `droid` to log in again.");
        }
        if !(200..300).contains(&resp.status) {
            return ProviderOutput::error(ID, NAME, format!("usage request failed (HTTP {})", resp.status));
        }
        let data = match resp.json() {
            Some(d) => d,
            None => return ProviderOutput::error(ID, NAME, "usage response not valid JSON"),
        };
        let usage = match data.get("usage") {
            Some(u) => u,
            None => return ProviderOutput::error(ID, NAME, "no usage data returned"),
        };

        let mut lines = Vec::new();
        if let Some(l) = token_line(usage, "standard", "Standard") {
            lines.push(l);
        }
        if let Some(l) = token_line(usage, "premium", "Premium") {
            lines.push(l);
        }

        if lines.is_empty() {
            return ProviderOutput::error(ID, NAME, "no usage data returned");
        }
        ProviderOutput::new(ID, NAME, lines)
    }
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
