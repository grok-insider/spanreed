//! Cursor provider.
//!
//! Token source (Linux): Cursor desktop SQLite state DB
//!   `~/.config/Cursor/User/globalStorage/state.vscdb`
//! reading `ItemTable` rows `cursorAuth/accessToken` etc.
//!
//! Usage via Connect-RPC over HTTPS to `api2.cursor.sh`:
//!   POST /aiserver.v1.DashboardService/GetCurrentPeriodUsage
//!   POST /aiserver.v1.DashboardService/GetPlanInfo

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "cursor";
const NAME: &str = "Cursor";
const BASE: &str = "https://api2.cursor.sh";

pub struct Cursor;

struct Auth {
    access_token: String,
    membership: Option<String>,
}

/// Candidate Cursor state DB paths on Linux.
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
    resp.json().ok_or_else(|| format!("Cursor RPC {path} returned invalid JSON"))
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
    }

    // On-demand spend (individual budget).
    if let Some(sl) = usage.get("spendLimitUsage") {
        let limit = sl.get("individualLimit").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let used = sl.get("individualUsed").and_then(|v| v.as_f64()).unwrap_or(0.0);
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

        // Surface plan as a text line too if present.
        if let Some(p) = &plan {
            lines.insert(0, MetricLine::text("Plan", p.clone()));
        }

        ProviderOutput::new(ID, NAME, lines).with_plan(plan)
    }
}
