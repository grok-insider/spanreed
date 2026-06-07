//! Codex (OpenAI Codex CLI / ChatGPT) provider.
//!
//! Auth file lookup order:
//!   1. `$CODEX_HOME/auth.json`
//!   2. `~/.config/codex/auth.json`
//!   3. `~/.codex/auth.json`
//!   4. Secret Service item `Codex Auth` (via secret-tool)
//!
//! Usage: `GET https://chatgpt.com/backend-api/wham/usage`.

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "codex";
const NAME: &str = "Codex";
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const REFRESH_URL: &str = "https://auth.openai.com/oauth/token";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const KEYCHAIN_SERVICE: &str = "Codex Auth";
const REFRESH_AGE_MS: i64 = 8 * 24 * 60 * 60 * 1000;

pub struct Codex;

#[derive(Clone, Copy, PartialEq)]
enum Source {
    File(usize),
    Secret,
}

fn auth_paths() -> Vec<std::path::PathBuf> {
    if let Some(home) = creds::env("CODEX_HOME") {
        return vec![creds::expand(&home).join("auth.json")];
    }
    vec![
        creds::config_home().join("codex").join("auth.json"),
        creds::expand("~/.codex").join("auth.json"),
    ]
}

fn has_token_like(auth: &serde_json::Value) -> bool {
    auth.get("tokens")
        .and_then(|t| t.get("access_token"))
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
        || auth
            .get("OPENAI_API_KEY")
            .and_then(|v| v.as_str())
            .is_some()
}

fn load_auth() -> Option<(serde_json::Value, Source, std::path::PathBuf)> {
    for (i, path) in auth_paths().into_iter().enumerate() {
        if let Some(value) = creds::read_json(&path) {
            if has_token_like(&value) {
                return Some((value, Source::File(i), path));
            }
        }
    }
    if let Some(text) = creds::secret_tool_lookup(&[("service", KEYCHAIN_SERVICE)]) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) {
            if has_token_like(&value) {
                return Some((value, Source::Secret, std::path::PathBuf::new()));
            }
        }
    }
    None
}

fn last_refresh_ms(auth: &serde_json::Value) -> Option<i64> {
    let raw = auth.get("last_refresh")?;
    util::to_iso(raw).and_then(|iso| {
        time::OffsetDateTime::parse(&iso, &time::format_description::well_known::Rfc3339)
            .ok()
            .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
    })
}

fn needs_refresh(auth: &serde_json::Value, now_ms: i64) -> bool {
    match last_refresh_ms(auth) {
        Some(last) => now_ms - last > REFRESH_AGE_MS,
        None => true,
    }
}

fn save_auth(auth: &serde_json::Value, source: Source, path: &std::path::Path) {
    let text = match serde_json::to_string(auth) {
        Ok(t) => t,
        Err(_) => return,
    };
    match source {
        Source::File(_) => {
            let _ = std::fs::write(path, text);
        }
        Source::Secret => {
            use std::io::Write;
            if let Ok(mut child) = std::process::Command::new("secret-tool")
                .args([
                    "store",
                    "--label",
                    "Codex Auth",
                    "service",
                    KEYCHAIN_SERVICE,
                ])
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                let _ = child.wait();
            }
        }
    }
}

fn refresh_if_needed(
    auth: &mut serde_json::Value,
    source: Source,
    path: &std::path::Path,
) -> Result<(), String> {
    if !needs_refresh(auth, util::now_ms()) {
        return Ok(());
    }
    let refresh_token = auth
        .get("tokens")
        .and_then(|t| t.get("refresh_token"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let refresh_token = match refresh_token {
        Some(t) => t.to_string(),
        None => return Ok(()),
    };

    let body = format!(
        "grant_type=refresh_token&client_id={}&refresh_token={}",
        urlencode(CLIENT_ID),
        urlencode(&refresh_token)
    );
    let resp = Request::post(REFRESH_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()?;

    if resp.status == 400 || resp.status == 401 {
        let code = resp.json().and_then(|b| {
            b.get("error")
                .and_then(|e| e.get("code").or(Some(e)))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        });
        return Err(match code.as_deref() {
            Some("refresh_token_expired") => "Session expired. Run `codex` to log in again.",
            Some("refresh_token_reused") => "Token conflict. Run `codex` to log in again.",
            Some("refresh_token_invalidated") => "Token revoked. Run `codex` to log in again.",
            _ => "Token expired. Run `codex` to log in again.",
        }
        .into());
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

    if let Some(tokens) = auth.get_mut("tokens").and_then(|v| v.as_object_mut()) {
        tokens.insert("access_token".into(), serde_json::json!(new_access));
        if let Some(rt) = json.get("refresh_token").and_then(|v| v.as_str()) {
            tokens.insert("refresh_token".into(), serde_json::json!(rt));
        }
        if let Some(idt) = json.get("id_token").and_then(|v| v.as_str()) {
            tokens.insert("id_token".into(), serde_json::json!(idt));
        }
    }
    if let Some(obj) = auth.as_object_mut() {
        if let Some(iso) = util::ms_to_iso(util::now_ms()) {
            obj.insert("last_refresh".into(), serde_json::json!(iso));
        }
    }
    save_auth(auth, source, path);
    Ok(())
}

fn window_progress(win: &serde_json::Value, label: &str, now_sec: i64) -> Option<MetricLine> {
    let used = win.get("used_percent")?.as_f64()?;
    let resets = win.get("reset_at").and_then(|r| {
        // reset_at is unix seconds; if missing, compute from window seconds.
        if r.is_number() {
            util::to_iso(r)
        } else {
            None
        }
    });
    let resets = resets.or_else(|| {
        let secs = win.get("limit_window_seconds")?.as_i64()?;
        util::ms_to_iso((now_sec + secs) * 1000)
    });
    Some(MetricLine::percent(label, used, resets))
}

fn parse_usage(data: &serde_json::Value) -> Vec<MetricLine> {
    let now_sec = util::now_ms() / 1000;
    let mut lines = Vec::new();

    if let Some(rl) = data.get("rate_limit") {
        if let Some(w) = rl.get("primary_window") {
            if let Some(l) = window_progress(w, "Session", now_sec) {
                lines.push(l);
            }
        }
        if let Some(w) = rl.get("secondary_window") {
            if let Some(l) = window_progress(w, "Weekly", now_sec) {
                lines.push(l);
            }
        }
    }

    if let Some(review) = data
        .get("code_review_rate_limit")
        .and_then(|c| c.get("primary_window"))
    {
        if let Some(l) = window_progress(review, "Reviews", now_sec) {
            lines.push(l);
        }
    }

    if let Some(credits) = data.get("credits") {
        if credits
            .get("has_credits")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            if let Some(balance) = credits.get("balance").and_then(|v| v.as_f64()) {
                lines.push(MetricLine::Text {
                    label: "Credits".into(),
                    value: format!("${balance:.2}"),
                    color: None,
                    subtitle: None,
                });
            }
        }
    }

    lines
}

fn build_plan(data: &serde_json::Value) -> Option<String> {
    let plan = data.get("plan_type").and_then(|v| v.as_str())?;
    let label = util::plan_label(plan);
    if label.is_empty() {
        None
    } else {
        Some(label)
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

impl Provider for Codex {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        auth_paths().iter().any(|p| p.exists())
            || creds::secret_tool_lookup(&[("service", KEYCHAIN_SERVICE)]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let (mut auth, source, path) = match load_auth() {
            Some(t) => t,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "No credentials found. Run `codex` to log in.",
                )
            }
        };

        if let Err(msg) = refresh_if_needed(&mut auth, source, &path) {
            return ProviderOutput::error(ID, NAME, msg);
        }

        let access_token = auth
            .get("tokens")
            .and_then(|t| t.get("access_token"))
            .and_then(|v| v.as_str());
        let access_token = match access_token {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => return ProviderOutput::error(ID, NAME, "No access token in auth file."),
        };
        let account_id = auth
            .get("tokens")
            .and_then(|t| t.get("account_id"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let mut req = Request::get(USAGE_URL)
            .bearer(&access_token)
            .header("Accept", "application/json")
            .header("User-Agent", "spanreed");
        if let Some(acc) = &account_id {
            req = req.header("ChatGPT-Account-Id", acc);
        }
        let resp = match req.send() {
            Ok(r) => r,
            Err(e) => return ProviderOutput::error(ID, NAME, e),
        };
        if resp.is_auth_error() {
            return ProviderOutput::error(ID, NAME, "Token rejected. Run `codex` to log in again.");
        }
        if !(200..300).contains(&resp.status) {
            return ProviderOutput::error(
                ID,
                NAME,
                format!("usage request failed (HTTP {})", resp.status),
            );
        }
        let data = match resp.json() {
            Some(d) => d,
            None => return ProviderOutput::error(ID, NAME, "usage response not valid JSON"),
        };
        let plan = build_plan(&data);
        let mut lines = parse_usage(&data);
        if lines.is_empty() {
            return ProviderOutput::error(ID, NAME, "no usage windows returned");
        }
        lines.extend(crate::cost::cost_lines(crate::cost::Source::Codex));
        ProviderOutput::new(ID, NAME, lines).with_plan(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(lines: &[MetricLine], label: &str) -> Option<f64> {
        lines.iter().find_map(|l| match l {
            MetricLine::Progress {
                label: lab, used, ..
            } if lab == label => Some(*used),
            _ => None,
        })
    }

    #[test]
    fn parses_rate_limit_windows_and_reviews_and_credits() {
        let data = serde_json::json!({
            "plan_type": "plus",
            "rate_limit": {
                "primary_window": { "used_percent": 6, "reset_at": 1738300000, "limit_window_seconds": 18000 },
                "secondary_window": { "used_percent": 24, "reset_at": 1738900000, "limit_window_seconds": 604800 }
            },
            "code_review_rate_limit": {
                "primary_window": { "used_percent": 2, "reset_at": 1738900000, "limit_window_seconds": 604800 }
            },
            "credits": { "has_credits": true, "unlimited": false, "balance": 5.39 }
        });
        let lines = parse_usage(&data);
        assert_eq!(used(&lines, "Session"), Some(6.0));
        assert_eq!(used(&lines, "Weekly"), Some(24.0));
        assert_eq!(used(&lines, "Reviews"), Some(2.0));
        // credits surface as a text line
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Text { label, value, .. } if label == "Credits" && value == "$5.39")));
        assert_eq!(build_plan(&data).as_deref(), Some("Plus"));
    }

    #[test]
    fn credits_hidden_without_has_credits() {
        let data = serde_json::json!({
            "rate_limit": { "primary_window": { "used_percent": 1, "reset_at": 1, "limit_window_seconds": 18000 } },
            "credits": { "has_credits": false, "balance": 0 }
        });
        let lines = parse_usage(&data);
        assert!(!lines
            .iter()
            .any(|l| matches!(l, MetricLine::Text { label, .. } if label == "Credits")));
    }
}
