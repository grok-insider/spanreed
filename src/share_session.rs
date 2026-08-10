//! Grok Insider session for authenticated community share.
//!
//! Obtained via device authorization (`spanreed share login`). Stored under
//! the spanreed config dir with restrictive file permissions where possible.

use std::path::PathBuf;

use crate::creds;
use crate::http::Request;
use crate::share;

const SESSION_FILE: &str = "share_session.json";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShareSession {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: u64,
    /// Unix seconds when access was last obtained (approx).
    #[serde(default)]
    pub obtained_at_unix: i64,
}

fn session_path() -> PathBuf {
    creds::config_home().join("spanreed").join(SESSION_FILE)
}

pub fn load() -> Option<ShareSession> {
    let raw = creds::read_file(&session_path())?;
    serde_json::from_str(raw.trim()).ok()
}

pub fn save(session: &ShareSession) -> Result<(), String> {
    let p = session_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir session: {e}"))?;
    }
    let body = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    std::fs::write(&p, format!("{body}\n")).map_err(|e| format!("write session: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn clear() -> Result<(), String> {
    let p = session_path();
    if p.exists() {
        std::fs::remove_file(&p).map_err(|e| format!("remove session: {e}"))?;
    }
    Ok(())
}

#[allow(dead_code)] // used by setup status / future UI hooks
pub fn is_logged_in() -> bool {
    load().is_some_and(|s| !s.refresh_token.trim().is_empty())
}

/// Refresh access token using stored refresh; updates disk session.
pub fn refresh_access(base: &str) -> Result<ShareSession, String> {
    let mut sess =
        load().ok_or_else(|| "not logged in — run: spanreed share login".to_string())?;
    let url = format!("{}/auth/refresh", base.trim_end_matches('/'));
    let body = serde_json::json!({ "refresh_token": sess.refresh_token });
    let res = Request::post(url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .map_err(|e| e.to_string())?;
    if res.status < 200 || res.status >= 300 {
        return Err(format!(
            "refresh failed HTTP {}: {}",
            res.status,
            res.body.chars().take(160).collect::<String>()
        ));
    }
    let v = res
        .json()
        .ok_or_else(|| "refresh: invalid json".to_string())?;
    let access = v
        .get("access_token")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "refresh: missing access_token".to_string())?;
    let refresh = v
        .get("refresh_token")
        .and_then(|x| x.as_str())
        .unwrap_or(sess.refresh_token.as_str());
    sess.access_token = access.to_string();
    sess.refresh_token = refresh.to_string();
    sess.expires_in = v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(900);
    sess.obtained_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    save(&sess)?;
    Ok(sess)
}

/// Valid access token, refreshing if missing/expired-ish (skew 60s).
pub fn ensure_access(base: &str) -> Result<String, String> {
    let sess = load().ok_or_else(|| "not logged in — run: spanreed share login".to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let exp_at = sess.obtained_at_unix + sess.expires_in as i64;
    if !sess.access_token.is_empty() && now < exp_at.saturating_sub(60) {
        return Ok(sess.access_token);
    }
    Ok(refresh_access(base)?.access_token)
}

/// Device authorization login (RFC 8628-style).
pub fn cmd_login() -> std::process::ExitCode {
    if share::is_offline() {
        eprintln!("share login: SPANREED_OFFLINE=1 — not starting device flow");
        return std::process::ExitCode::SUCCESS;
    }
    let base = share::api_base();
    let url = format!("{}/v1/usage/device/code", base.trim_end_matches('/'));
    let res = match Request::post(url).send() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("share login: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    if res.status < 200 || res.status >= 300 {
        eprintln!(
            "share login: HTTP {}: {}",
            res.status,
            res.body.chars().take(200).collect::<String>()
        );
        return std::process::ExitCode::FAILURE;
    }
    let v = match res.json() {
        Some(j) => j,
        None => {
            eprintln!("share login: invalid json from device/code");
            return std::process::ExitCode::FAILURE;
        }
    };
    let device_code = v
        .get("device_code")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let user_code = v
        .get("user_code")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let verification = v
        .get("verification_uri_complete")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("verification_uri").and_then(|x| x.as_str()))
        .unwrap_or("https://grokinsider.net/open-usage/link");
    let interval = v
        .get("interval")
        .and_then(|x| x.as_u64())
        .unwrap_or(5)
        .max(2);
    let expires_in = v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(600);

    if device_code.is_empty() || user_code.is_empty() {
        eprintln!("share login: missing device_code/user_code");
        return std::process::ExitCode::FAILURE;
    }

    println!("spanreed share login\n");
    println!("  1. Open:  {verification}");
    println!("  2. Code:  {user_code}");
    println!("  3. Sign in with X and approve this CLI\n");
    println!("Waiting for approval (up to {expires_in}s)…");

    let poll_url = format!("{}/v1/usage/device/poll", base.trim_end_matches('/'));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(expires_in);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_secs(interval));
        let body = serde_json::json!({ "device_code": device_code });
        let res = match Request::post(&poll_url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("share login: poll error: {e}");
                continue;
            }
        };
        if res.status == 400 {
            // authorization_pending
            continue;
        }
        if res.status < 200 || res.status >= 300 {
            eprintln!(
                "share login: poll HTTP {}: {}",
                res.status,
                res.body.chars().take(160).collect::<String>()
            );
            return std::process::ExitCode::FAILURE;
        }
        let v = match res.json() {
            Some(j) => j,
            None => {
                eprintln!("share login: poll invalid json");
                return std::process::ExitCode::FAILURE;
            }
        };
        let access = v
            .get("access_token")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let refresh = v
            .get("refresh_token")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if access.is_empty() || refresh.is_empty() {
            eprintln!("share login: missing tokens in poll response");
            return std::process::ExitCode::FAILURE;
        }
        let sess = ShareSession {
            access_token: access,
            refresh_token: refresh,
            token_type: v
                .get("token_type")
                .and_then(|x| x.as_str())
                .unwrap_or("Bearer")
                .to_string(),
            expires_in: v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(900),
            obtained_at_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        };
        if let Err(e) = save(&sess) {
            eprintln!("share login: save session: {e}");
            return std::process::ExitCode::FAILURE;
        }
        println!("Logged in. Daily share can run without the browser.");
        return std::process::ExitCode::SUCCESS;
    }
    eprintln!("share login: timed out waiting for approval");
    std::process::ExitCode::FAILURE
}

pub fn cmd_logout() -> std::process::ExitCode {
    match clear() {
        Ok(()) => {
            println!("share: logged out (local session removed)");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("share logout: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

pub fn cmd_status() -> std::process::ExitCode {
    match load() {
        Some(s) if !s.refresh_token.is_empty() => {
            println!("share: logged in (refresh present)");
            if let Some(day) = share::last_shared_day() {
                println!("share: last shared day {day}");
            } else {
                println!("share: last shared day: never");
            }
            std::process::ExitCode::SUCCESS
        }
        _ => {
            println!("share: not logged in — run: spanreed share login");
            std::process::ExitCode::FAILURE
        }
    }
}
