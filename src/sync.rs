//! Push/pull SuperGrok plan + hops to ai.fabrials.com. No OAuth secrets.
//!
//! Requires `spanreed share login` (same Fabrials X JWT). Only the **active**
//! Grok account is pulled back into this install.

use fabrials_model::UsageRecord;

use crate::accounts;
use crate::app;
use crate::drivers;
use crate::grok_ledger;
use crate::http::Request;
use crate::share_session;
use crate::util;

const DEFAULT_RELAY: &str = "https://ai.fabrials.com";
const ENV_RELAY: &str = "SPANREED_RELAY_BASE";
const BATCH: usize = 200;

pub fn relay_base() -> String {
    std::env::var(ENV_RELAY)
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_RELAY.into())
}

pub fn grok_material_from_token(token: &str) -> Option<String> {
    let v = util::jwt_payload(token)?;
    v.get("sub")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            v.get("email")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        })
}

fn access_token() -> Result<String, String> {
    if crate::app::env_offline() {
        return Err("offline".into());
    }
    let base = crate::share::api_base();
    share_session::ensure_access(&base)
}

/// Best-effort after a Grok probe: push the active account + recent ledger, pull hops.
pub fn after_probe() {
    if !crate::privacy::load().sync_history {
        return;
    }
    if crate::app::env_offline() {
        return;
    }
    if share_session::load().is_none() {
        return;
    }
    if let Err(e) = run(false) {
        log::debug!("sync: {e}");
    }
}

pub fn cmd(args: &[String]) -> std::process::ExitCode {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "spanreed sync — push/pull SuperGrok plan + capture hops to ai.fabrials.com\n\n\
             Requires: spanreed share login (same X account as the dashboard).\n\
             Pushes the active Grok account (plan, pool %, request ledger).\n\
             Pulls hops for that fingerprint only — other cloud accounts stay on the VPS.\n\n\
             {ENV_RELAY}  default {DEFAULT_RELAY}\n\
             SPANREED_OFFLINE=1 skips."
        );
        return std::process::ExitCode::SUCCESS;
    }
    match run(true) {
        Ok(msg) => {
            println!("{msg}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("sync: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

pub fn run(verbose: bool) -> Result<String, String> {
    if !crate::privacy::load().sync_history {
        return Err("History sync is off. Enable explicitly: spanreed privacy sync on".into());
    }
    let token = access_token()?;
    let Some(acc) = accounts::active("grok") else {
        return Err("no active grok account".into());
    };
    let Some(grok_tok) = drivers::grok::token_for_alias(&acc.alias) else {
        return Err(format!("no grok token for {}", acc.id));
    };
    let Some(material) = grok_material_from_token(&grok_tok) else {
        return Err("grok access token has no OIDC sub — cannot fingerprint".into());
    };
    let base = relay_base();
    let _id = put_account(&base, &token, &acc, &material)?;
    let recs = grok_ledger::read_window(util::now_ms());
    let mine: Vec<_> = recs
        .into_iter()
        .filter(|r| r.account_id.as_deref() == Some(acc.id.as_str()) || r.account_id.is_none())
        .collect();
    let mut pushed = 0usize;
    for chunk in mine.chunks(BATCH) {
        pushed += post_usage(&base, &token, chunk)?;
    }
    let pulled = pull_into_local(&base, &token, &material, &acc)?;
    let _ = verbose;
    Ok(format!(
        "synced {} (pushed {pushed} hops, pulled {pulled})",
        acc.id
    ))
}

fn put_account(
    base: &str,
    access: &str,
    acc: &accounts::Account,
    material: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "provider": "grok",
        "alias": acc.alias,
        "account_id": acc.id,
        "plan_slug": acc.plan_slug,
        "plan_label": acc.plan_label,
        "used_pct": acc.used_pct,
        "resets_at": acc.resets_at,
        "material": material,
    });
    put_json(base, access, "/__spanreed/sync/account", &body.to_string())
}

fn put_json(base: &str, access: &str, path: &str, body: &str) -> Result<String, String> {
    let url = format!("{}{path}", base.trim_end_matches('/'));
    let res = put_request(&url, access, body)?;
    if res.status == 403 {
        return Err("forbidden — invite or @grokinsider subscriber required".into());
    }
    if res.status == 401 {
        return Err("unauthorized — run: spanreed share login".into());
    }
    if !(200..300).contains(&res.status) {
        return Err(format!(
            "sync account HTTP {}: {}",
            res.status,
            res.body.chars().take(160).collect::<String>()
        ));
    }
    let v = res.json().unwrap_or_else(|| serde_json::json!({}));
    Ok(v.get("id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string())
}

fn put_request(url: &str, access: &str, body: &str) -> Result<crate::http::Response, String> {
    // http::Request may only expose get/post. Use POST-with-override if needed.
    crate::http::Request::put(url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {access}"))
        .header("User-Agent", app::user_agent())
        .body(body.to_string())
        .send()
        .map_err(|e| e.to_string())
}

fn post_usage(base: &str, access: &str, recs: &[UsageRecord]) -> Result<usize, String> {
    if recs.is_empty() {
        return Ok(0);
    }
    let body = serde_json::json!({ "records": recs });
    let url = format!("{}/__spanreed/sync/usage", base.trim_end_matches('/'));
    let res = Request::post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {access}"))
        .header("User-Agent", app::user_agent())
        .body(body.to_string())
        .send()
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&res.status) {
        return Err(format!("sync usage HTTP {}", res.status));
    }
    Ok(recs.len())
}

fn pull_into_local(
    base: &str,
    access: &str,
    material: &str,
    acc: &accounts::Account,
) -> Result<usize, String> {
    let since = util::now_ms().saturating_sub(31 * 86_400_000);
    let body = serde_json::json!({ "material": material, "since_ms": since });
    let url = format!("{}/__spanreed/sync/pull", base.trim_end_matches('/'));
    let res = Request::post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {access}"))
        .header("User-Agent", app::user_agent())
        .body(body.to_string())
        .send()
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&res.status) {
        return Err(format!("sync pull HTTP {}", res.status));
    }
    let v = res.json().ok_or("sync pull json")?;
    if let Some(remote) = v.get("account") {
        if !remote.is_null() {
            let used = remote.get("used_pct").and_then(|x| x.as_f64());
            let plan = remote
                .get("plan_slug")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            let label = remote
                .get("plan_label")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            let reset = remote
                .get("resets_at")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            let _ =
                accounts::apply_snapshot(&acc.id, plan, label, used, reset, util::now_ms(), None);
        }
    }
    let mut n = 0;
    if let Some(arr) = v.get("records").and_then(|x| x.as_array()) {
        for rec in arr {
            if let Ok(r) = serde_json::from_value::<UsageRecord>(rec.clone()) {
                if grok_ledger::append(&r).is_ok() {
                    n += 1;
                }
            }
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_prefers_sub() {
        let token = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJnb29nbGUtb2F1dGgyfHVzZXJfYWJjIiwiZXhwIjoxNzAwMDAwMDAwfQ.";
        assert_eq!(
            grok_material_from_token(token).as_deref(),
            Some("google-oauth2|user_abc")
        );
    }
}
