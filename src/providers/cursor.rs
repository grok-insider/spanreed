//! Cursor provider.
//!
//! Token source: Cursor desktop SQLite state DB at the per-OS config dir's
//!   `Cursor/User/globalStorage/state.vscdb` (VS Code-style layout: Linux
//!   `~/.config`, macOS `~/Library/Application Support`, Windows `%APPDATA%`,
//!   resolved via `dirs`), reading `ItemTable` rows `cursorAuth/accessToken` etc.
//!
//! Usage via Connect-RPC over HTTPS to `api2.cursor.sh`
//! (`GetCurrentPeriodUsage`, `GetPlanInfo`, `GetCreditGrantsBalance`) plus the
//! `cursor.com` REST endpoints for the Stripe customer balance (credits) and
//! enterprise request counts, authenticated with a `WorkosCursorSessionToken`
//! cookie derived from the JWT `sub`.

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProgressFormat, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "cursor";
const NAME: &str = "Cursor";
const BASE: &str = "https://api2.cursor.sh";
const STRIPE_URL: &str = "https://cursor.com/api/auth/stripe";
const REST_USAGE_URL: &str = "https://cursor.com/api/usage";

pub struct Cursor;

struct Auth {
    access_token: String,
    membership: Option<String>,
}

/// Candidate Cursor state DB paths (per-OS config dir via `dirs`).
fn state_db_paths() -> Vec<std::path::PathBuf> {
    let cfg = creds::config_home();
    vec![
        cfg.join("Cursor/User/globalStorage/state.vscdb"),
        cfg.join("cursor/User/globalStorage/state.vscdb"),
    ]
}

fn read_item(db: &std::path::Path, key: &str) -> Option<String> {
    creds::sqlite_query_one(db, "SELECT value FROM ItemTable WHERE key = ?1", &[&key])
        .map(|v| v.trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
}

fn load_auth() -> Option<Auth> {
    let db = creds::first_existing(&state_db_paths())?;
    let access_token = read_item(&db, "cursorAuth/accessToken")?;
    let membership = read_item(&db, "cursorAuth/stripeMembershipType");
    Some(Auth {
        access_token,
        membership,
    })
}

fn rpc(path: &str, token: &str) -> Result<serde_json::Value, String> {
    let resp = Request::post(format!("{BASE}{path}"))
        .bearer(token)
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .body("{}")
        .send()?;
    if resp.is_auth_error() {
        return Err("Token rejected. Re-authenticate in Cursor.".into());
    }
    if !(200..300).contains(&resp.status) {
        return Err(format!("Cursor RPC {path} failed (HTTP {})", resp.status));
    }
    resp.json()
        .ok_or_else(|| format!("Cursor RPC {path} returned invalid JSON"))
}

/// Derive `(userId, WorkosCursorSessionToken)` from the access token's JWT
/// `sub` claim (e.g. `google-oauth2|user_abc` -> `user_abc`).
fn session_token(access_token: &str) -> Option<(String, String)> {
    let sub = util::jwt_payload(access_token)?
        .get("sub")?
        .as_str()?
        .to_string();
    let user_id = sub
        .split('|')
        .next_back()
        .filter(|s| !s.is_empty())?
        .to_string();
    let token = format!("{user_id}%3A%3A{access_token}");
    Some((user_id, token))
}

/// Stripe prepaid balance in cents (abs of a negative `customerBalance`).
fn fetch_stripe_balance(session: &str) -> i64 {
    let resp = match Request::get(STRIPE_URL)
        .header("Cookie", format!("WorkosCursorSessionToken={session}"))
        .send()
    {
        Ok(r) if (200..300).contains(&r.status) => r,
        _ => return 0,
    };
    let balance = resp
        .json()
        .and_then(|j| j.get("customerBalance").and_then(|v| v.as_f64()))
        .unwrap_or(0.0);
    if balance < 0.0 {
        (-balance) as i64
    } else {
        0
    }
}

/// Combined credits line: grant balance + Stripe prepaid balance (cents).
fn credits_line(
    grant_total_cents: i64,
    grant_used_cents: i64,
    stripe_cents: i64,
) -> Option<MetricLine> {
    let combined = grant_total_cents.max(0) + stripe_cents.max(0);
    if combined <= 0 {
        return None;
    }
    Some(MetricLine::dollars(
        "Credits",
        util::cents_to_dollars(grant_used_cents.max(0) as f64),
        util::cents_to_dollars(combined as f64),
        None,
    ))
}

/// Enterprise request-count line from the REST `/api/usage` payload.
fn requests_line(rest: &serde_json::Value) -> Option<MetricLine> {
    let gpt4 = rest.get("gpt-4")?;
    let limit = gpt4.get("maxRequestUsage")?.as_f64()?;
    if limit <= 0.0 {
        return None;
    }
    let used = gpt4
        .get("numRequests")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    Some(MetricLine::Progress {
        label: "Requests".into(),
        used,
        limit,
        format: ProgressFormat::Count {
            suffix: "requests".into(),
        },
        resets_at: None,
        color: None,
    })
}

fn parse_usage(usage: &serde_json::Value) -> Vec<MetricLine> {
    let mut lines = Vec::new();
    let plan_usage = usage.get("planUsage");

    // Total usage as a percentage when available.
    if let Some(pu) = plan_usage {
        if let Some(total_pct) = pu.get("totalPercentUsed").and_then(|v| v.as_f64()) {
            if total_pct.is_finite() {
                lines.push(MetricLine::percent("Total usage", total_pct, None));
            }
        } else if let (Some(limit), Some(remaining)) = (
            pu.get("limit").and_then(|v| v.as_f64()),
            pu.get("remaining").and_then(|v| v.as_f64()),
        ) {
            if limit > 0.0 {
                let pct = (limit - remaining) / limit * 100.0;
                lines.push(MetricLine::percent("Total usage", pct, None));
            }
        }

        if let Some(auto) = pu.get("autoPercentUsed").and_then(|v| v.as_f64()) {
            if auto.is_finite() {
                lines.push(MetricLine::percent("Auto usage", auto, None));
            }
        }
        if let Some(api) = pu.get("apiPercentUsed").and_then(|v| v.as_f64()) {
            if api.is_finite() {
                lines.push(MetricLine::percent("API usage", api, None));
            }
        }

        // Bonus spend (free credits from model providers), if any.
        if let Some(bonus) = pu.get("bonusSpend").and_then(|v| v.as_f64()) {
            if bonus > 0.0 {
                lines.push(MetricLine::text(
                    "Bonus spend",
                    format!("${:.2}", util::cents_to_dollars(bonus)),
                ));
            }
        }
    }

    // On-demand spend (individual budget).
    if let Some(sl) = usage.get("spendLimitUsage") {
        let limit = sl
            .get("individualLimit")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let used = sl
            .get("individualUsed")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if limit > 0.0 {
            lines.push(MetricLine::dollars(
                "On-demand",
                util::cents_to_dollars(used),
                util::cents_to_dollars(limit),
                None,
            ));
        }
    }

    lines
}

impl Provider for Cursor {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        state_db_paths().iter().any(|p| p.exists())
    }

    fn probe(&self) -> ProviderOutput {
        let auth = match load_auth() {
            Some(a) => a,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "No Cursor auth found. Sign in to the Cursor app.",
                )
            }
        };

        let usage = match rpc(
            "/aiserver.v1.DashboardService/GetCurrentPeriodUsage",
            &auth.access_token,
        ) {
            Ok(v) => v,
            Err(e) => return ProviderOutput::error(ID, NAME, e),
        };

        let mut lines = parse_usage(&usage);

        // Credits: grant balance (RPC) + Stripe prepaid balance (cookie).
        let session = session_token(&auth.access_token);
        let (grant_total, grant_used) = rpc(
            "/aiserver.v1.DashboardService/GetCreditGrantsBalance",
            &auth.access_token,
        )
        .ok()
        .filter(|g| {
            g.get("hasCreditGrants")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .map(|g| (cents_field(&g, "totalCents"), cents_field(&g, "usedCents")))
        .unwrap_or((0, 0));
        let stripe = session
            .as_ref()
            .map(|(_, tok)| fetch_stripe_balance(tok))
            .unwrap_or(0);
        if let Some(line) = credits_line(grant_total, grant_used, stripe) {
            lines.insert(0, line);
        }

        // Enterprise request count (best-effort).
        if let Some((user_id, tok)) = &session {
            if let Ok(resp) = Request::get(format!("{REST_USAGE_URL}?user={}", urlencode(user_id)))
                .header("Cookie", format!("WorkosCursorSessionToken={tok}"))
                .send()
            {
                if (200..300).contains(&resp.status) {
                    if let Some(rest) = resp.json() {
                        if let Some(line) = requests_line(&rest) {
                            lines.push(line);
                        }
                    }
                }
            }
        }

        if lines.is_empty() {
            return ProviderOutput::error(ID, NAME, "no usage data returned");
        }

        // Plan name (best-effort).
        let plan = rpc(
            "/aiserver.v1.DashboardService/GetPlanInfo",
            &auth.access_token,
        )
        .ok()
        .and_then(|info| {
            info.get("planInfo")
                .and_then(|p| p.get("planName"))
                .and_then(|v| v.as_str())
                .map(util::plan_label)
        })
        .or_else(|| auth.membership.as_deref().map(util::plan_label));

        if let Some(p) = &plan {
            lines.insert(0, MetricLine::text("Plan", p.clone()));
        }

        ProviderOutput::new(ID, NAME, lines).with_plan(plan)
    }
}

/// Read a cents field that may be a number or a numeric string.
fn cents_field(v: &serde_json::Value, key: &str) -> i64 {
    match v.get(key) {
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(serde_json::Value::String(s)) => s.trim().parse::<i64>().unwrap_or(0),
        _ => 0,
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
    fn parses_percent_lines_and_bonus_and_on_demand() {
        let usage = serde_json::json!({
            "planUsage": {
                "totalPercentUsed": 15.48,
                "autoPercentUsed": 0.0,
                "apiPercentUsed": 46.44,
                "bonusSpend": 1234
            },
            "spendLimitUsage": { "individualLimit": 10000, "individualUsed": 2500 }
        });
        let lines = parse_usage(&usage);
        assert_eq!(used(&lines, "Total usage"), Some(15.48));
        assert_eq!(used(&lines, "API usage"), Some(46.44));
        // bonus spend 1234c -> $12.34 text
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Text { label, value, .. } if label == "Bonus spend" && value == "$12.34")));
        // on-demand 2500c/10000c -> $25 / $100
        assert_eq!(used(&lines, "On-demand"), Some(25.0));
    }

    #[test]
    fn credits_line_combines_grant_and_stripe() {
        // grant: total 40000c, used 10000c; stripe prepaid 5000c -> limit $450, used $100
        let line = credits_line(40000, 10000, 5000).unwrap();
        match line {
            MetricLine::Progress {
                label, used, limit, ..
            } => {
                assert_eq!(label, "Credits");
                assert_eq!(used, 100.0);
                assert_eq!(limit, 450.0);
            }
            _ => panic!("expected progress"),
        }
        assert!(credits_line(0, 0, 0).is_none());
    }

    #[test]
    fn session_token_derives_user_id_from_sub() {
        // sub = "google-oauth2|user_abc"
        let jwt = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJnb29nbGUtb2F1dGgyfHVzZXJfYWJjIn0.";
        let (uid, tok) = session_token(jwt).unwrap();
        assert_eq!(uid, "user_abc");
        assert!(tok.starts_with("user_abc%3A%3A"));
    }

    #[test]
    fn requests_line_from_enterprise_rest() {
        let rest = serde_json::json!({ "gpt-4": { "numRequests": 120, "maxRequestUsage": 500 } });
        let line = requests_line(&rest).unwrap();
        assert!(
            matches!(line, MetricLine::Progress { used, limit, .. } if used == 120.0 && limit == 500.0)
        );
        assert!(requests_line(&serde_json::json!({})).is_none());
    }

    #[test]
    fn cents_field_handles_string_or_number() {
        assert_eq!(
            cents_field(&serde_json::json!({"totalCents": 40000}), "totalCents"),
            40000
        );
        assert_eq!(
            cents_field(&serde_json::json!({"totalCents": "40000"}), "totalCents"),
            40000
        );
        assert_eq!(cents_field(&serde_json::json!({}), "totalCents"), 0);
    }
}
