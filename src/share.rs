//! Opt-in **authenticated** share of aggregated usage snapshots to
//! fabrials.com (Fabrials account via Sign in with X).
//!
//! Contributions are tied to a stable `user_id` on the server (not the raw
//! install UUID). They feed a public pool used to compare how much value
//! (limits / pool pressure / token signals) different subscription plans
//! deliver over time.
//!
//! **Login once:** `spanreed share login` (device code → browser X login).
//! **At most one meaningful sample per product day** (Europe/Madrid); the
//! server **upserts** the same day. Auto-share is installed by
//! `spanreed setup` via [`crate::share_schedule`].
//!
//! Never includes provider tokens, API keys, or raw capture logs.

use std::path::PathBuf;

use crate::http::Request;
use crate::model::{MetricKind, MetricLine, ProgressFormat, ProviderOutput};
use crate::probe;
use crate::util;

const DEFAULT_API_BASE: &str = "https://fabrials.com/api/spanreed";
const ENV_API_BASE: &str = "SPANREED_API_BASE";
const ENV_OFFLINE: &str = "SPANREED_OFFLINE";
const LAST_SHARE_DAY_FILE: &str = "last_share_day";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ShareSnapshot {
    pub schema_version: u32,
    pub captured_at: String,
    pub source: ShareSource,
    pub providers: Vec<ShareProvider>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ShareSource {
    pub app: String,
    pub version: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ShareProvider {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub lines: Vec<ShareLine>,
    /// Structured plan economics (100% pool API $). Schema v2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub economics: Option<crate::share_economics::ProviderEconomics>,
    /// Early pool resets observed on this install (does not change at 100% week).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resets: Vec<crate::epoch::ResetEvent>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ShareLine {
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Map probe outputs → community share API snapshot.
///
/// Includes quota/plan/cost lines (and errors) plus structured **economics**
/// (observed API $ and at 100% pool estimates). Multi-model mixes are valued in
/// the CLI, not re-blended on the web.
pub fn snapshot_from_outputs(outputs: &[ProviderOutput], version: &str) -> ShareSnapshot {
    let mut providers = Vec::new();
    let mut any_econ = false;
    for o in outputs {
        let lines: Vec<ShareLine> = o
            .lines
            .iter()
            .filter(|l| {
                matches!(
                    l.kind(),
                    MetricKind::Quota | MetricKind::Plan | MetricKind::Cost | MetricKind::Error
                )
            })
            .filter_map(line_to_share)
            .collect();
        if lines.is_empty() {
            continue;
        }
        let by_model = crate::share_economics::model_breakdown_for(&o.provider_id);
        let economics = crate::share_economics::from_output(o, by_model);
        if economics.is_some() {
            any_econ = true;
        }
        let since = util::now_ms().saturating_sub(2 * 86_400_000);
        let resets: Vec<_> = crate::epoch::recent_events(since)
            .into_iter()
            .filter(|e| e.provider == o.provider_id)
            .collect();
        providers.push(ShareProvider {
            id: o.provider_id.clone(),
            plan: o.plan.clone().filter(|p| !p.trim().is_empty()),
            lines,
            economics,
            resets,
        });
    }
    ShareSnapshot {
        // v2 when any economics block present; API still accepts v1 lines-only.
        schema_version: if any_econ { 2 } else { 1 },
        captured_at: util::ms_to_iso(util::now_ms())
            .unwrap_or_else(|| "1970-01-01T00:00:00Z".into()),
        source: ShareSource {
            app: "spanreed".into(),
            version: version.into(),
        },
        providers,
    }
}

fn line_to_share(line: &MetricLine) -> Option<ShareLine> {
    match line {
        MetricLine::Progress {
            label,
            used,
            limit,
            format,
            resets_at,
            ..
        } => {
            // Keep format kinds distinct so the web does not render absolute
            // counts (Cursor/Factory/Kiro) as percentages.
            let kind = match format {
                ProgressFormat::Percent => "percent",
                ProgressFormat::Dollars => "dollars",
                ProgressFormat::Count { .. } => "count",
            };
            Some(ShareLine {
                kind: kind.into(),
                label: label.clone(),
                used: Some(*used),
                limit: Some(*limit),
                resets_at: resets_at.clone(),
                value: None,
            })
        }
        MetricLine::Text { label, value, .. } => Some(ShareLine {
            kind: "text".into(),
            label: label.clone(),
            used: None,
            limit: None,
            resets_at: None,
            value: Some(value.clone()),
        }),
        MetricLine::Badge { label, text, .. } => Some(ShareLine {
            kind: "badge".into(),
            label: label.clone(),
            used: None,
            limit: None,
            resets_at: None,
            value: Some(text.clone()),
        }),
        MetricLine::BarChart { .. } => None,
    }
}

pub fn api_base() -> String {
    std::env::var(ENV_API_BASE).unwrap_or_else(|_| DEFAULT_API_BASE.into())
}

pub fn is_offline() -> bool {
    matches!(
        std::env::var(ENV_OFFLINE).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

fn last_share_path() -> PathBuf {
    crate::app::config_dir().join(LAST_SHARE_DAY_FILE)
}

/// Last successfully recorded share day (`YYYY-MM-DD`), if any.
pub fn last_shared_day() -> Option<String> {
    let raw = crate::creds::read_file(&last_share_path())?;
    let day = raw.trim();
    if day.len() == 10 && day.as_bytes()[4] == b'-' && day.as_bytes()[7] == b'-' {
        Some(day.to_string())
    } else {
        None
    }
}

/// Whether a share is still due given last recorded day and today's key.
pub fn is_due_for_day(last: Option<&str>, today: &str) -> bool {
    match last {
        None => true,
        Some(d) => d != today,
    }
}

/// Whether a share is still due for the current product day.
pub fn is_due_today() -> bool {
    is_due_for_day(last_shared_day().as_deref(), &util::today_day_key_madrid())
}

/// Persist successful (or server-already-counted) share for the product day.
pub fn mark_shared_day(day: &str) -> Result<(), String> {
    let p = last_share_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir last_share_day: {e}"))?;
    }
    std::fs::write(&p, format!("{day}\n")).map_err(|e| format!("write last_share_day: {e}"))
}

/// Outcome of a share POST (for due-gate bookkeeping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostOutcome {
    /// Accepted by the API (2xx).
    Accepted(u16),
    /// Rate-limited — treat as already counted for the day when possible.
    AlreadyCounted,
}

/// POST authenticated snapshot (Bearer + install client id).
pub fn post_snapshot(
    base: &str,
    access_token: &str,
    snap: &ShareSnapshot,
    client_id: &str,
) -> Result<PostOutcome, String> {
    let url = format!("{}/v1/usage/snapshots", base.trim_end_matches('/'));
    let body = serde_json::to_string(snap).map_err(|e| e.to_string())?;
    let res = Request::post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("X-Spanreed-Client", client_id)
        .header(
            "User-Agent",
            format!("spanreed/{} (+share)", env!("CARGO_PKG_VERSION")),
        )
        .body(body)
        .send()
        .map_err(|e| e.to_string())?;
    if res.status >= 200 && res.status < 300 {
        Ok(PostOutcome::Accepted(res.status))
    } else if res.status == 429 {
        Ok(PostOutcome::AlreadyCounted)
    } else if res.status == 401 {
        Err("share unauthorized — run: spanreed share login".into())
    } else {
        Err(format!(
            "share failed HTTP {}: {}",
            res.status,
            res.body.chars().take(200).collect::<String>()
        ))
    }
}

pub fn cmd(args: &[String]) -> std::process::ExitCode {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "spanreed share — authenticated upload of plan/quota metrics\n\n\
             Requires a Grok Insider account (Sign in with X) linked once via:\n\
               spanreed share login\n\n\
             Sends aggregated provider+plan lines to the public community pool\n\
             on fabrials.com. Server identity is your account (not install id).\n\n\
             At most one local send per day (product TZ {tz}) unless --force.\n\
             Same-day re-send upserts on the server.\n\n\
             Subcommands: login | logout | status\n\
             Optional SPANREED_API_BASE (default {DEFAULT_API_BASE}).\n\
             SPANREED_OFFLINE=1 skips the network call.\n\
             --force  bypass local same-day skip.",
            tz = util::SHARE_TZ_LABEL
        );
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(sub) = args.first().map(String::as_str) {
        match sub {
            "login" => return crate::share_session::cmd_login(),
            "logout" => return crate::share_session::cmd_logout(),
            "status" => return crate::share_session::cmd_status(),
            _ => {}
        }
    }

    let force = args.iter().any(|a| a == "--force");
    match share_once(force) {
        Ok(msg) => {
            println!("{msg}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) if e.starts_with("share: SPANREED_OFFLINE") => {
            eprintln!("{e}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) if e.contains("already sent") => {
            eprintln!("{e}");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Probe + POST. `force` bypasses the local same-day skip.
pub fn share_once(force: bool) -> Result<String, String> {
    if !crate::privacy::load().share_metrics {
        return Err(
            "Metrics sharing is off. Enable explicitly: spanreed privacy metrics on".into(),
        );
    }
    if is_offline() {
        return Err("share: SPANREED_OFFLINE=1 — not sending".into());
    }

    let day = util::today_day_key_madrid();
    if !force && !is_due_today() {
        return Err(format!(
            "share: already sent for {day} (use --force to retry)"
        ));
    }

    let base = api_base();
    let access = crate::share_session::ensure_access(&base)?;

    let outputs = probe::probe_detected();
    let version = env!("CARGO_PKG_VERSION");
    let snap = snapshot_from_outputs(&outputs, version);
    if snap.providers.is_empty() {
        return Err("share: no shareable metrics from detected providers".into());
    }

    let client_id = crate::client_id::ensure().map_err(|e| format!("share: client_id: {e}"))?;

    let outcome = match post_snapshot(&base, &access, &snap, &client_id) {
        Ok(o) => o,
        Err(e) if e.contains("unauthorized") => {
            let sess = crate::share_session::refresh_access(&base)?;
            post_snapshot(&base, &sess.access_token, &snap, &client_id)?
        }
        Err(e) => return Err(e),
    };

    match outcome {
        PostOutcome::Accepted(status) => {
            if let Err(e) = mark_shared_day(&day) {
                return Ok(format!(
                    "shared {} provider(s) to {base} (HTTP {status}, day {day}); warning: {e}",
                    snap.providers.len()
                ));
            }
            Ok(format!(
                "shared {} provider(s) to {base} (HTTP {status}, day {day})",
                snap.providers.len()
            ))
        }
        PostOutcome::AlreadyCounted => {
            let _ = mark_shared_day(&day);
            Ok(format!(
                "share: rate-limited for {day} (HTTP 429) — marked local day"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MetricKind, MetricLine, ProgressFormat, ProviderOutput};

    #[test]
    fn due_gate_skips_same_day_only() {
        assert!(is_due_for_day(None, "2026-08-10"));
        assert!(is_due_for_day(Some("2026-08-09"), "2026-08-10"));
        assert!(!is_due_for_day(Some("2026-08-10"), "2026-08-10"));
    }

    #[test]
    fn maps_quota_progress_and_skips_cost_charts() {
        let out = ProviderOutput {
            reset_inventory: None,
            provider_id: "grok".into(),
            display_name: "Grok".into(),
            plan: Some("SuperGrok".into()),
            lines: vec![
                MetricLine::Progress {
                    kind: MetricKind::Quota,
                    label: "Weekly".into(),
                    used: 42.5,
                    limit: 100.0,
                    format: ProgressFormat::Percent,
                    resets_at: Some("2026-08-17T00:00:00Z".into()),
                    color: None,
                },
                MetricLine::Text {
                    kind: MetricKind::Cost,
                    label: "Last 30 Days".into(),
                    value: "$31 · 46M tokens".into(),
                    color: None,
                    subtitle: None,
                },
                MetricLine::BarChart {
                    kind: MetricKind::Cost,
                    label: "Spend".into(),
                    points: vec![],
                    note: None,
                    color: None,
                },
            ],
        };
        let snap = snapshot_from_outputs(&[out], "0.0.1");
        // Cost text + weekly pool → schema v2 economics payload.
        assert_eq!(snap.schema_version, 2);
        assert_eq!(snap.source.app, "spanreed");
        assert_eq!(snap.source.version, "0.0.1");
        assert_eq!(snap.providers.len(), 1);
        assert_eq!(snap.providers[0].id, "grok");
        // Quota + cost text; bar charts still skipped.
        assert_eq!(snap.providers[0].lines.len(), 2);
        assert_eq!(snap.providers[0].lines[0].kind, "percent");
        assert_eq!(snap.providers[0].lines[0].used, Some(42.5));
        assert_eq!(snap.providers[0].lines[1].kind, "text");
        assert!(snap.providers[0].economics.is_some());
        // No secret fields in serialized JSON.
        let v = serde_json::to_value(&snap).unwrap();
        let s = v.to_string().to_ascii_lowercase();
        assert!(!s.contains("\"token\""));
        assert!(!s.contains("password"));
    }

    #[test]
    fn maps_count_progress_as_count_not_percent() {
        // Absolute used/limit (e.g. request pools) must not become "50%".
        let out = ProviderOutput {
            reset_inventory: None,
            provider_id: "cursor".into(),
            display_name: "Cursor".into(),
            plan: None,
            lines: vec![MetricLine::Progress {
                kind: MetricKind::Quota,
                label: "Requests".into(),
                used: 50.0,
                limit: 500.0,
                format: ProgressFormat::Count {
                    suffix: "reqs".into(),
                },
                resets_at: None,
                color: None,
            }],
        };
        let snap = snapshot_from_outputs(&[out], "0.0.1");
        assert_eq!(snap.providers[0].lines[0].kind, "count");
        assert_eq!(snap.providers[0].lines[0].used, Some(50.0));
        assert_eq!(snap.providers[0].lines[0].limit, Some(500.0));
        assert_ne!(snap.providers[0].lines[0].kind, "percent");
    }

    #[test]
    fn snapshot_json_has_no_secret_field_names() {
        let out = ProviderOutput {
            reset_inventory: None,
            provider_id: "codex".into(),
            display_name: "Codex".into(),
            plan: None,
            lines: vec![MetricLine::Progress {
                kind: MetricKind::Quota,
                label: "5h".into(),
                used: 1.0,
                limit: 100.0,
                format: ProgressFormat::Percent,
                resets_at: None,
                color: None,
            }],
        };
        let snap = snapshot_from_outputs(&[out], "0.0.1");
        let keys = collect_keys(&serde_json::to_value(&snap).unwrap());
        for bad in [
            "token",
            "secret",
            "password",
            "authorization",
            "api_key",
            "credential",
        ] {
            assert!(
                keys.iter().all(|k| !k.to_ascii_lowercase().contains(bad)),
                "forbidden key substring {bad} in {keys:?}"
            );
        }
    }

    fn collect_keys(v: &serde_json::Value) -> Vec<String> {
        match v {
            serde_json::Value::Object(m) => {
                let mut out = Vec::new();
                for (k, child) in m {
                    out.push(k.clone());
                    out.extend(collect_keys(child));
                }
                out
            }
            serde_json::Value::Array(a) => a.iter().flat_map(collect_keys).collect(),
            _ => vec![],
        }
    }
}
