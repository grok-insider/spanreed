//! Opt-in anonymous share of aggregated usage snapshots to api.grokinsider.net.
//!
//! Contributions are **not** tied to a user account. They feed a public pool
//! used to compare how much value (limits / pool pressure / token signals)
//! different subscription plans deliver over time.
//!
//! Daily auto-share (23:00 Europe/Madrid) is installed by `spanreed setup`
//! via [`crate::share_schedule`]. Manual `spanreed share` still works.
//!
//! Never includes provider tokens, API keys, raw capture logs, or user identity.

use crate::http::Request;
use crate::model::{MetricKind, MetricLine, ProgressFormat, ProviderOutput};
use crate::probe;
use crate::util;

const DEFAULT_API_BASE: &str = "https://api.grokinsider.net";
const ENV_API_BASE: &str = "SPANREED_API_BASE";
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
    /// Structured plan economics (100% pool API $). Schema v2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub economics: Option<crate::share_economics::ProviderEconomics>,
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

/// Map probe outputs → anonymous API snapshot.
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
        providers.push(ShareProvider {
            id: o.provider_id.clone(),
            plan: o.plan.clone().filter(|p| !p.trim().is_empty()),
            lines,
            economics,
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

/// POST anonymous snapshot to API (no auth). Returns status code on success.
pub fn post_snapshot(base: &str, snap: &ShareSnapshot, client_id: &str) -> Result<u16, String> {
    let url = format!("{}/v1/usage/snapshots", base.trim_end_matches('/'));
    let body = serde_json::to_string(snap).map_err(|e| e.to_string())?;
    let res = Request::post(url)
        .header("Content-Type", "application/json")
        .header("X-OpenUsage-Client", client_id)
        .header(
            "User-Agent",
            format!("spanreed/{} (+share)", env!("CARGO_PKG_VERSION")),
        )
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
            "spanreed share — opt-in anonymous upload of plan/quota metrics\n\n\
             Sends aggregated provider+plan lines to the public community pool\n\
             on grokinsider.net (no login, no user id). Used to track how much\n\
             value different subscription plans deliver over time.\n\n\
             Optional SPANREED_API_BASE (default {DEFAULT_API_BASE}).\n\
             SPANREED_OFFLINE=1 skips the network call."
        );
        return std::process::ExitCode::SUCCESS;
    }

    if is_offline() {
        eprintln!("share: SPANREED_OFFLINE=1 — not sending");
        return std::process::ExitCode::SUCCESS;
    }

    let outputs = probe::probe_detected();
    let version = env!("CARGO_PKG_VERSION");
    let snap = snapshot_from_outputs(&outputs, version);
    if snap.providers.is_empty() {
        eprintln!("share: no shareable metrics from detected providers");
        return std::process::ExitCode::FAILURE;
    }

    let client_id = match crate::client_id::ensure() {
        Ok(id) => id,
        Err(e) => {
            eprintln!("share: client_id: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let base = api_base();
    match post_snapshot(&base, &snap, &client_id) {
        Ok(status) => {
            println!(
                "shared {} provider(s) anonymously to {base} (HTTP {status})",
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
        assert_eq!(snap.schema_version, 1);
        assert_eq!(snap.source.app, "spanreed");
        assert_eq!(snap.source.version, "0.0.1");
        assert_eq!(snap.providers.len(), 1);
        assert_eq!(snap.providers[0].id, "grok");
        // Quota + cost text; bar charts still skipped.
        assert_eq!(snap.providers[0].lines.len(), 2);
        assert_eq!(snap.providers[0].lines[0].kind, "percent");
        assert_eq!(snap.providers[0].lines[0].used, Some(42.5));
        assert_eq!(snap.providers[0].lines[1].kind, "text");
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
