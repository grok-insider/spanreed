//! GitHub Copilot provider.
//!
//! Token discovery (Linux):
//!   1. `gh auth token` (GitHub CLI)
//!   2. Secret Service item `gh:github.com` (via secret-tool)
//!   3. file fallback `~/.config/gh/hosts.yml` oauth_token
//! Usage: `GET https://api.github.com/copilot_internal/user`.

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "copilot";
const NAME: &str = "Copilot";
const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";

pub struct Copilot;

/// Read the gh token via the CLI (most reliable across keyring/file backends).
fn token_from_gh_cli() -> Option<String> {
    let out = std::process::Command::new("gh")
        .args(["auth", "token"])
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

/// Secret Service fallback (gh stores under `gh:github.com`).
fn token_from_secret() -> Option<String> {
    let raw = creds::secret_tool_lookup(&[("service", "gh:github.com")])?;
    // gh sometimes prefixes go-keyring-base64: on stored secrets.
    if let Some(b64) = raw.strip_prefix("go-keyring-base64:") {
        return util::base64_decode_str(b64).filter(|s| !s.is_empty());
    }
    Some(raw)
}

/// Plain hosts.yml fallback: scan for `oauth_token: <token>`.
fn token_from_hosts_file() -> Option<String> {
    let path = creds::config_home().join("gh").join("hosts.yml");
    let text = creds::read_file(&path)?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("oauth_token:") {
            let t = rest.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn token() -> Option<String> {
    token_from_gh_cli()
        .or_else(token_from_secret)
        .or_else(token_from_hosts_file)
}

/// used = 100 - percent_remaining (clamped).
fn snapshot_line(label: &str, snap: &serde_json::Value, resets: &Option<String>) -> Option<MetricLine> {
    let pct_remaining = snap.get("percent_remaining")?.as_f64()?;
    let used = (100.0 - pct_remaining).clamp(0.0, 100.0);
    Some(MetricLine::percent(label, used, resets.clone()))
}

impl Provider for Copilot {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        token().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let token = match token() {
            Some(t) => t,
            None => {
                return ProviderOutput::error(ID, NAME, "No GitHub token. Run `gh auth login`.")
            }
        };

        let resp = match Request::get(USAGE_URL)
            .header("Authorization", format!("token {token}"))
            .header("Accept", "application/json")
            .header("Editor-Version", "vscode/1.96.2")
            .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
            .header("User-Agent", "GitHubCopilotChat/0.26.7")
            .header("X-Github-Api-Version", "2025-04-01")
            .send()
        {
            Ok(r) => r,
            Err(e) => return ProviderOutput::error(ID, NAME, e),
        };
        if resp.is_auth_error() {
            return ProviderOutput::error(ID, NAME, "Token rejected. Run `gh auth login`.");
        }
        if !(200..300).contains(&resp.status) {
            return ProviderOutput::error(ID, NAME, format!("usage request failed (HTTP {})", resp.status));
        }
        let data = match resp.json() {
            Some(d) => d,
            None => return ProviderOutput::error(ID, NAME, "usage response not valid JSON"),
        };

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
            return ProviderOutput::error(ID, NAME, "no usage quotas returned (free tier or no Copilot)");
        }

        let plan = data
            .get("copilot_plan")
            .and_then(|v| v.as_str())
            .map(util::plan_label);

        ProviderOutput::new(ID, NAME, lines).with_plan(plan)
    }
}
