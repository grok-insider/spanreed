//! GitHub Copilot provider (opt-in).
//!
//! Copilot is **not** auto-detected from `gh`. A GitHub token only means you
//! use GitHub — not that you have Copilot. Multi-account `gh` setups also make
//! "active account" the wrong token.
//!
//! Setup once:
//!   `spanreed auth copilot`              interactive (pick gh user or paste)
//!   `spanreed auth copilot --user LOGIN` import that gh account's token
//!   `spanreed auth copilot --token-stdin` read a token from stdin
//!   `spanreed auth logout copilot`       forget the stored credential
//!
//! Storage (first hit wins on read):
//!   1. keyring service `spanreed:copilot`
//!   2. file `~/.config/spanreed/copilot.token` (mode 0600)
//! Metadata (login only, no secret): `~/.config/spanreed/copilot.json`
//!
//! Usage API: `GET https://api.github.com/copilot_internal/user`.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::Command;

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::secret;
use crate::util;

const ID: &str = "copilot";
const NAME: &str = "Copilot";
const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";
const KEYRING_SERVICE: &str = "spanreed:copilot";
const GH_KEYRING_SERVICE: &str = "gh:github.com";

pub struct Copilot;

fn config_dir() -> PathBuf {
    creds::config_home().join("spanreed")
}

fn token_path() -> PathBuf {
    config_dir().join("copilot.token")
}

fn meta_path() -> PathBuf {
    config_dir().join("copilot.json")
}

/// Read the opt-in token spanreed owns (keyring, then file).
fn stored_token() -> Option<String> {
    if let Some(t) = secret::lookup(KEYRING_SERVICE) {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    let text = creds::read_file(&token_path())?;
    let t = text.trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

fn write_token_file(token: &str) -> Result<(), String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let path = token_path();
    std::fs::write(&path, format!("{token}\n")).map_err(|e| format!("write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn write_meta(login: &str) -> Result<(), String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let path = meta_path();
    let body = serde_json::json!({ "login": login });
    std::fs::write(&path, format!("{body}\n")).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

fn clear_meta() {
    let _ = std::fs::remove_file(meta_path());
}

fn clear_token_file() {
    let _ = std::fs::remove_file(token_path());
}

/// Persist token: prefer keyring; always keep a 0600 file fallback so auth works
/// without `secret-tool`.
fn store_credential(token: &str, login: &str) -> Result<(), String> {
    let _ = secret::store(KEYRING_SERVICE, "spanreed Copilot token", token);
    write_token_file(token)?;
    write_meta(login)?;
    Ok(())
}

fn clear_credential() {
    let _ = secret::delete(KEYRING_SERVICE);
    clear_token_file();
    clear_meta();
}

/// GitHub logins listed under `~/.config/gh/hosts.yml` for github.com.
pub fn list_gh_users() -> Vec<String> {
    let path = creds::config_home().join("gh").join("hosts.yml");
    let text = match creds::read_file(&path) {
        Some(t) => t,
        None => return Vec::new(),
    };
    parse_gh_hosts_users(&text)
}

/// Token for a `gh` multi-account user (CLI first, then keyring by username).
pub fn token_for_gh_user(user: &str) -> Option<String> {
    token_from_gh_cli_user(user).or_else(|| {
        let raw = secret::lookup_user(GH_KEYRING_SERVICE, user)?;
        if let Some(b64) = raw.strip_prefix("go-keyring-base64:") {
            return util::base64_decode_str(b64).filter(|s| !s.is_empty());
        }
        let t = raw.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    })
}

fn token_from_gh_cli_user(user: &str) -> Option<String> {
    let out = Command::new("gh")
        .args(["auth", "token", "-u", user])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Minimal YAML scrape of:
/// ```yaml
/// github.com:
///   users:
///     alice: {}
///     bob:
///   user: alice
/// ```
fn parse_gh_hosts_users(text: &str) -> Vec<String> {
    let mut users = Vec::new();
    let mut in_github = false;
    let mut in_users = false;
    let mut users_indent: Option<usize> = None;
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let content = line.trim_start();
        if indent == 0 && content.starts_with("github.com") {
            in_github = true;
            in_users = false;
            users_indent = None;
            continue;
        }
        if indent == 0 {
            in_github = false;
            in_users = false;
            users_indent = None;
            continue;
        }
        if !in_github {
            continue;
        }
        if content.starts_with("users:") {
            in_users = true;
            users_indent = Some(indent);
            continue;
        }
        if in_users {
            let base = users_indent.unwrap_or(indent);
            // Sibling keys of `users:` (same indent) end the block.
            if indent <= base {
                in_users = false;
                users_indent = None;
                continue;
            }
            // Nested under users: "0xfell:" or "0xfell: {}"
            if let Some(name) = content.split(':').next() {
                let name = name.trim();
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    && !users.iter().any(|u| u == name)
                {
                    users.push(name.to_string());
                }
            }
        }
    }
    users
}

/// used = 100 - percent_remaining (clamped).
fn snapshot_line(
    label: &str,
    snap: &serde_json::Value,
    resets: &Option<String>,
) -> Option<MetricLine> {
    let pct_remaining = snap.get("percent_remaining")?.as_f64()?;
    let used = (100.0 - pct_remaining).clamp(0.0, 100.0);
    Some(MetricLine::percent(label, used, resets.clone()))
}

/// Probe the Copilot usage API with a token. Returns Ok((login, plan, lines)) or Err(msg).
pub fn validate_token(token: &str) -> Result<(String, Option<String>, Vec<MetricLine>), String> {
    let resp = Request::get(USAGE_URL)
        .header("Authorization", format!("token {token}"))
        .header("Accept", "application/json")
        .header("Editor-Version", "vscode/1.96.2")
        .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
        .header("User-Agent", "GitHubCopilotChat/0.26.7")
        .header("X-Github-Api-Version", "2025-04-01")
        .send()
        .map_err(|e| e.to_string())?;
    if resp.is_auth_error() {
        return Err("Token rejected by GitHub.".into());
    }
    if !(200..300).contains(&resp.status) {
        return Err(format!("usage request failed (HTTP {})", resp.status));
    }
    let data = resp
        .json()
        .ok_or_else(|| "usage response not valid JSON".to_string())?;

    let login = data
        .get("login")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let sku = data
        .get("access_type_sku")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if sku == "no_access" {
        return Err(format!(
            "account {login} has no Copilot access (access_type_sku=no_access)"
        ));
    }

    let resets = data.get("quota_reset_date").and_then(util::to_iso);
    let mut lines = Vec::new();
    if let Some(snaps) = data.get("quota_snapshots") {
        if let Some(prem) = snaps.get("premium_interactions") {
            if let Some(l) = snapshot_line("Premium", prem, &resets) {
                lines.push(l);
            }
        }
        if let Some(chat) = snaps.get("chat") {
            if let Some(l) = snapshot_line("Chat", chat, &resets) {
                lines.push(l);
            }
        }
        if let Some(comp) = snaps.get("completions") {
            if let Some(l) = snapshot_line("Completions", comp, &resets) {
                lines.push(l);
            }
        }
    }
    if lines.is_empty() {
        return Err(format!(
            "no usage quotas returned for {login} (free tier or no Copilot)"
        ));
    }

    let plan = data
        .get("copilot_plan")
        .and_then(|v| v.as_str())
        .map(util::plan_label);

    Ok((login, plan, lines))
}

/// Interactive / flag-driven auth. See module docs.
pub fn cmd_auth(args: &[String]) -> Result<(), String> {
    let token_stdin = args.iter().any(|a| a == "--token-stdin");
    let user = args
        .iter()
        .position(|a| a == "--user")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str());

    let token = if token_stdin {
        let mut buf = String::new();
        io::stdin()
            .read_line(&mut buf)
            .map_err(|e| format!("read stdin: {e}"))?;
        let t = buf.trim().to_string();
        if t.is_empty() {
            return Err("empty token on stdin".into());
        }
        t
    } else if let Some(u) = user {
        token_for_gh_user(u).ok_or_else(|| {
            format!(
                "no token for gh user '{u}'. Run `gh auth login -u {u}` or pass --token-stdin."
            )
        })?
    } else if let Some(t) = creds::env("SPANREED_GITHUB_TOKEN").or_else(|| creds::env("GH_TOKEN"))
    {
        t
    } else {
        resolve_token_interactive()?
    };

    let (login, plan, lines) = validate_token(&token)?;
    store_credential(&token, &login)?;
    let plan_s = plan.as_deref().unwrap_or("linked");
    println!("Copilot linked as {login} ({plan_s})");
    for line in &lines {
        match line {
            MetricLine::Progress {
                label,
                used,
                format: crate::model::ProgressFormat::Percent,
                ..
            } => println!("  {label}: {used:.0}%"),
            _ => {}
        }
    }
    Ok(())
}

fn resolve_token_interactive() -> Result<String, String> {
    let users = list_gh_users();
    let mut stdout = io::stdout();
    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    if !users.is_empty() {
        println!("Link GitHub Copilot to spanreed (opt-in; not auto-detected from gh).\n");
        for (i, u) in users.iter().enumerate() {
            println!("  {}) {}", i + 1, u);
        }
        println!("  {}) Paste a token", users.len() + 1);
        print!("Choice [1-{}]: ", users.len() + 1);
        let _ = stdout.flush();
        let mut line = String::new();
        stdin
            .read_line(&mut line)
            .map_err(|e| format!("read choice: {e}"))?;
        let choice = line.trim().parse::<usize>().unwrap_or(0);
        if choice >= 1 && choice <= users.len() {
            let u = &users[choice - 1];
            return token_for_gh_user(u).ok_or_else(|| {
                format!("no token for gh user '{u}'. Run `gh auth login -u {u}` or paste a token.")
            });
        }
        if choice == users.len() + 1 {
            print!("Token: ");
            let _ = stdout.flush();
            let mut tok = String::new();
            stdin
                .read_line(&mut tok)
                .map_err(|e| format!("read token: {e}"))?;
            let t = tok.trim().to_string();
            if t.is_empty() {
                return Err("empty token".into());
            }
            return Ok(t);
        }
        return Err("invalid choice".into());
    }

    println!("No gh accounts found in ~/.config/gh/hosts.yml.");
    print!("Paste a GitHub token with Copilot access: ");
    let _ = stdout.flush();
    let mut tok = String::new();
    stdin
        .read_line(&mut tok)
        .map_err(|e| format!("read token: {e}"))?;
    let t = tok.trim().to_string();
    if t.is_empty() {
        return Err("empty token".into());
    }
    Ok(t)
}

pub fn cmd_logout() -> Result<(), String> {
    clear_credential();
    println!("Copilot credential removed.");
    Ok(())
}

impl Provider for Copilot {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        stored_token().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let token = match stored_token() {
            Some(t) => t,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "Not linked. Run `spanreed auth copilot`.",
                )
            }
        };

        match validate_token(&token) {
            Ok((_login, plan, lines)) => ProviderOutput::new(ID, NAME, lines).with_plan(plan),
            Err(e) => ProviderOutput::error(ID, NAME, e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_used_is_inverse_of_remaining() {
        let snap = serde_json::json!({ "percent_remaining": 30.0 });
        let line = snapshot_line("Premium", &snap, &None).unwrap();
        match line {
            MetricLine::Progress { label, used, .. } => {
                assert_eq!(label, "Premium");
                assert_eq!(used, 70.0);
            }
            _ => panic!("expected progress"),
        }
    }

    #[test]
    fn snapshot_none_without_percent() {
        assert!(snapshot_line("X", &serde_json::json!({}), &None).is_none());
    }

    #[test]
    fn parse_hosts_users_multi() {
        let yml = r#"
github.com:
    users:
        0xfell: {}
        grok-insider:
    git_protocol: ssh
    user: grok-insider
"#;
        let users = parse_gh_hosts_users(yml);
        assert_eq!(users, vec!["0xfell".to_string(), "grok-insider".to_string()]);
    }

    #[test]
    fn parse_hosts_empty_without_github() {
        assert!(parse_gh_hosts_users("gitlab.com:\n  user: x\n").is_empty());
    }
}
