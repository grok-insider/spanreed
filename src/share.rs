//! Opt-in share of aggregated usage snapshots to api.grokinsider.net.
//!
//! Never includes provider tokens, API keys, or raw capture logs.

use crate::http::Request;
use crate::model::{MetricKind, MetricLine, ProgressFormat, ProviderOutput};
use crate::probe;
use crate::util;

const DEFAULT_API_BASE: &str = "https://api.grokinsider.net";
const ENV_API_BASE: &str = "SPANREED_API_BASE";
const ENV_TOKEN: &str = "SPANREED_SHARE_TOKEN";
const ENV_OFFLINE: &str = "SPANREED_OFFLINE";

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

/// Map probe outputs → API snapshot (quota/plan/error lines only).
pub fn snapshot_from_outputs(outputs: &[ProviderOutput], version: &str) -> ShareSnapshot {
    let mut providers = Vec::new();
    for o in outputs {
        let lines: Vec<ShareLine> = o
            .lines
            .iter()
            .filter(|l| {
                matches!(
                    l.kind(),
                    MetricKind::Quota | MetricKind::Plan | MetricKind::Error
                )
            })
            .filter_map(line_to_share)
            .collect();
        if lines.is_empty() && o.lines.is_empty() {
            continue;
        }
        // Always include provider if it has shareable lines, or if errored with badge.
        if lines.is_empty() {
            continue;
        }
        providers.push(ShareProvider {
            id: o.provider_id.clone(),
            plan: o.plan.clone().filter(|p| !p.trim().is_empty()),
            lines,
        });
    }
    ShareSnapshot {
        schema_version: 1,
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

pub fn share_token() -> Option<String> {
    std::env::var(ENV_TOKEN)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

pub fn is_offline() -> bool {
    matches!(
        std::env::var(ENV_OFFLINE).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

/// POST snapshot to API. Returns status code on success path.
pub fn post_snapshot(base: &str, token: &str, snap: &ShareSnapshot) -> Result<u16, String> {
    let url = format!("{}/v1/usage/snapshots", base.trim_end_matches('/'));
    let body = serde_json::to_string(snap).map_err(|e| e.to_string())?;
    let res = Request::post(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .map_err(|e| e.to_string())?;
    if res.status >= 200 && res.status < 300 {
        Ok(res.status)
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
            "spanreed share — opt-in upload of aggregated quotas to grokinsider.net\n\n\
             Requires SPANREED_SHARE_TOKEN (Bearer access JWT from a logged-in session).\n\
             Optional SPANREED_API_BASE (default {DEFAULT_API_BASE}).\n\
             SPANREED_OFFLINE=1 skips the network call."
        );
        return std::process::ExitCode::SUCCESS;
    }

    if is_offline() {
        eprintln!("share: SPANREED_OFFLINE=1 — not sending");
        return std::process::ExitCode::SUCCESS;
    }

    let Some(token) = share_token() else {
        eprintln!(
            "share: set SPANREED_SHARE_TOKEN to a Grok Insider access JWT\n\
             (log in at grokinsider.net, then export the access token for CLI use)."
        );
        return std::process::ExitCode::FAILURE;
    };

    let outputs = probe::probe_detected();
    let version = env!("CARGO_PKG_VERSION");
    let snap = snapshot_from_outputs(&outputs, version);
    if snap.providers.is_empty() {
        eprintln!("share: no quota/plan metrics from detected providers");
        return std::process::ExitCode::FAILURE;
    }

    let base = api_base();
    match post_snapshot(&base, &token, &snap) {
        Ok(status) => {
            println!(
                "shared {} provider(s) to {base} (HTTP {status})",
                snap.providers.len()
            );
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MetricKind, MetricLine, ProgressFormat, ProviderOutput};

    #[test]
    fn maps_quota_progress_and_skips_cost_charts() {
        let out = ProviderOutput {
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
        assert_eq!(snap.schema_version, 1);
        assert_eq!(snap.source.app, "spanreed");
        assert_eq!(snap.source.version, "0.0.1");
        assert_eq!(snap.providers.len(), 1);
        assert_eq!(snap.providers[0].id, "grok");
        assert_eq!(snap.providers[0].lines.len(), 1);
        assert_eq!(snap.providers[0].lines[0].kind, "percent");
        assert_eq!(snap.providers[0].lines[0].used, Some(42.5));
        // No secret fields in serialized JSON.
        let v = serde_json::to_value(&snap).unwrap();
        let s = v.to_string().to_ascii_lowercase();
        assert!(!s.contains("token"));
        assert!(!s.contains("password"));
    }

    #[test]
    fn maps_count_progress_as_count_not_percent() {
        // Absolute used/limit (e.g. request pools) must not become "50%".
        let out = ProviderOutput {
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
