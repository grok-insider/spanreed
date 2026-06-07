//! Kiro provider.
//!
//! Reads Kiro's local usage cache from its SQLite state DB (Linux path
//! `~/.config/Kiro/User/globalStorage/state.vscdb`), key `kiro.kiroAgent`,
//! nested `kiro.resourceNotifications.usageState.usageBreakdowns`. Shows the
//! primary CREDIT pool. Detection also accepts the AWS SSO token file.

use crate::creds;
use crate::model::{MetricLine, ProgressFormat, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "kiro";
const NAME: &str = "Kiro";
const STATE_KEY: &str = "kiro.kiroAgent";

pub struct Kiro;

fn state_db() -> std::path::PathBuf {
    creds::config_home().join("Kiro/User/globalStorage/state.vscdb")
}

fn auth_token_file() -> std::path::PathBuf {
    creds::expand("~/.aws/sso/cache/kiro-auth-token.json")
}

/// Read and parse the nested usageState JSON from the SQLite ItemTable.
fn read_usage_state() -> Option<serde_json::Value> {
    let db = state_db();
    if !db.exists() {
        return None;
    }
    let raw = creds::sqlite_query_one(
        &db,
        "SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1",
        &[&STATE_KEY],
    )?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
    parsed.get("kiro.resourceNotifications.usageState").cloned()
}

fn num(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64())
}

/// Pick the CREDIT breakdown (or the first available).
fn primary_breakdown(usage_state: &serde_json::Value) -> Option<&serde_json::Value> {
    let breakdowns = usage_state.get("usageBreakdowns")?.as_array()?;
    breakdowns
        .iter()
        .find(|b| {
            b.get("resourceType")
                .or_else(|| b.get("type"))
                .and_then(|v| v.as_str())
                == Some("CREDIT")
        })
        .or_else(|| breakdowns.first())
}

impl Provider for Kiro {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        state_db().exists() || auth_token_file().exists()
    }

    fn probe(&self) -> ProviderOutput {
        let usage_state = match read_usage_state() {
            Some(s) => s,
            None => {
                // Visible (token/state present) but no readable usage cache.
                if auth_token_file().exists() {
                    return ProviderOutput::new(
                        ID,
                        NAME,
                        vec![MetricLine::Badge {
                            label: "Status".into(),
                            text: "No usage data".into(),
                            color: Some("#a3a3a3".into()),
                            subtitle: None,
                        }],
                    );
                }
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "Kiro usage data unavailable. Open the Kiro account dashboard once and try again.",
                );
            }
        };

        match parse_usage_state(&usage_state) {
            Some(lines) => ProviderOutput::new(ID, NAME, lines).with_plan(parse_plan(&usage_state)),
            // Visible but no usable breakdown/limit yet.
            None => ProviderOutput::new(
                ID,
                NAME,
                vec![MetricLine::Badge {
                    label: "Status".into(),
                    text: "No usage data".into(),
                    color: Some("#a3a3a3".into()),
                    subtitle: None,
                }],
            ),
        }
    }
}

/// Parse the normalized usageState into Credits / Bonus Credits / Overages
/// lines. Returns None when there is no usable primary breakdown.
fn parse_usage_state(usage_state: &serde_json::Value) -> Option<Vec<MetricLine>> {
    let primary = primary_breakdown(usage_state)?;
    let limit = num(primary.get("usageLimit")).unwrap_or(0.0);
    let used = num(primary.get("currentUsage")).unwrap_or(0.0);
    if limit <= 0.0 {
        return None;
    }

    let resets = primary.get("resetDate").and_then(util::to_iso);
    let mut lines = vec![MetricLine::Progress {
        label: "Credits".into(),
        used,
        limit,
        format: ProgressFormat::Count {
            suffix: "credits".into(),
        },
        resets_at: resets,
        color: None,
    }];

    // Bonus / free-trial credit pool, when present.
    let bonus = primary
        .get("freeTrialUsage")
        .filter(|f| num(f.get("usageLimit")).unwrap_or(0.0) > 0.0)
        .or_else(|| {
            primary
                .get("bonuses")
                .and_then(|b| b.as_array())
                .and_then(|arr| arr.first())
        });
    if let Some(bonus) = bonus {
        let blimit = num(bonus.get("usageLimit")).unwrap_or(0.0);
        let bused = num(bonus.get("currentUsage")).unwrap_or(0.0);
        if blimit > 0.0 {
            lines.push(MetricLine::Progress {
                label: "Bonus Credits".into(),
                used: bused,
                limit: blimit,
                format: ProgressFormat::Count {
                    suffix: "credits".into(),
                },
                resets_at: None,
                color: None,
            });
        }
    }

    // Overage status badge if present.
    if let Some(status) = usage_state
        .get("overageConfiguration")
        .and_then(|o| o.get("overageStatus"))
        .and_then(|v| v.as_str())
    {
        lines.push(MetricLine::Badge {
            label: "Overages".into(),
            text: util::plan_label(status),
            color: None,
            subtitle: None,
        });
    }

    Some(lines)
}

fn parse_plan(usage_state: &serde_json::Value) -> Option<String> {
    usage_state
        .get("subscriptionInfo")
        .and_then(|s| s.get("subscriptionTitle"))
        .and_then(|v| v.as_str())
        .map(util::plan_label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_credits_bonus_and_overages() {
        let us = serde_json::json!({
            "usageBreakdowns": [
                { "resourceType": "CREDIT", "usageLimit": 1000, "currentUsage": 250, "resetDate": "2026-07-01T00:00:00Z",
                  "freeTrialUsage": { "usageLimit": 50, "currentUsage": 10 } }
            ],
            "overageConfiguration": { "overageStatus": "enabled" },
            "subscriptionInfo": { "subscriptionTitle": "kiro pro" }
        });
        let lines = parse_usage_state(&us).unwrap();
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Progress { label, used, limit, .. } if label == "Credits" && *used == 250.0 && *limit == 1000.0)));
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Progress { label, used, limit, .. } if label == "Bonus Credits" && *used == 10.0 && *limit == 50.0)));
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Badge { label, text, .. } if label == "Overages" && text == "Enabled")));
        assert_eq!(parse_plan(&us).as_deref(), Some("Kiro Pro"));
    }

    #[test]
    fn none_when_limit_zero() {
        let us = serde_json::json!({ "usageBreakdowns": [ { "resourceType": "CREDIT", "usageLimit": 0 } ] });
        assert!(parse_usage_state(&us).is_none());
    }
}
