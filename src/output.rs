//! Render provider outputs in the formats the CLI exposes.

use crate::model::{BarChartPoint, MetricLine, ProgressFormat, ProviderOutput};

const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Render bar-chart points as a compact unicode sparkline.
fn sparkline(points: &[BarChartPoint]) -> String {
    if points.is_empty() {
        return String::new();
    }
    let max = points.iter().map(|p| p.value).fold(0.0_f64, f64::max);
    if max <= 0.0 {
        return SPARK[0].to_string().repeat(points.len());
    }
    points
        .iter()
        .map(|p| {
            let idx = ((p.value / max) * (SPARK.len() - 1) as f64).round() as usize;
            SPARK[idx.min(SPARK.len() - 1)]
        })
        .collect()
}

/// Format a progress line's used/limit as a short human string.
fn fmt_progress(used: f64, limit: f64, format: &ProgressFormat) -> String {
    match format {
        ProgressFormat::Percent => format!("{:.0}%", used),
        ProgressFormat::Dollars => format!("${:.2} / ${:.2}", used, limit),
        ProgressFormat::Count { suffix } => format!("{:.0}/{:.0} {}", used, limit, suffix),
    }
}

/// A ` · resets in 3h 12m` suffix for a progress line, or empty when there is
/// no (future) reset time.
fn reset_suffix(resets_at: &Option<String>) -> String {
    resets_at
        .as_deref()
        .and_then(crate::util::reset_in)
        .map(|s| format!(" · resets in {s}"))
        .unwrap_or_default()
}

/// Percentage for a progress line (for bar/severity), or None.
pub fn line_percent(line: &MetricLine) -> Option<f64> {
    match line {
        MetricLine::Progress {
            used,
            limit,
            format,
            ..
        } => Some(match format {
            ProgressFormat::Percent => *used,
            _ => {
                if *limit > 0.0 {
                    used / limit * 100.0
                } else {
                    0.0
                }
            }
        }),
        _ => None,
    }
}

/// Providers allowed to drive the collapsed Waybar bar text. Everything else
/// (Copilot, Cursor, ...) is still shown in the tooltip but never sets the bar.
const BAR_PROVIDERS: &[&str] = &["claude", "codex", "grok"];

/// Plan labels treated as non-paid; a provider on one of these never drives the
/// bar. Matched case-insensitively against each whitespace-token of the plan.
const FREE_PLAN_LABELS: &[&str] = &["free", "guest"];

/// Weekly utilization (%) at/above which the bar escalates from the Session
/// window to the Weekly window. Matches `severity()`'s warning band so the bar
/// turns yellow exactly when it starts reflecting the weekly constraint.
const WEEKLY_ESCALATE_PCT: f64 = 80.0;

/// True when `plan` denotes an active paid subscription (Some, non-empty, and
/// not a known free/guest tier). Stale/unknown plans (`None`) are not paid.
fn is_paid_plan(plan: &Option<String>) -> bool {
    match plan.as_deref().map(str::trim) {
        Some(p) if !p.is_empty() => !p
            .split_whitespace()
            .any(|tok| FREE_PLAN_LABELS.iter().any(|f| tok.eq_ignore_ascii_case(f))),
        _ => false,
    }
}

/// True when a provider may contribute to the bar text: it's in the allow-list,
/// on a paid plan, and didn't error.
fn bar_eligible(out: &ProviderOutput) -> bool {
    BAR_PROVIDERS.contains(&out.provider_id.as_str()) && is_paid_plan(&out.plan) && !out.has_error()
}

/// The percentage a provider contributes to the bar.
///
/// Anchored on the Session (5h) window so a freshly-reset session isn't masked
/// by a higher long-window value. Escalates to the Weekly window only when
/// Weekly is itself in the warning band (>= `WEEKLY_ESCALATE_PCT`) and higher
/// than Session, so a near-exhausted weekly limit still surfaces. Providers
/// without a Session window (e.g. Grok's single "Weekly") fall back to
/// their first progress line.
fn provider_bar_pct(out: &ProviderOutput) -> Option<f64> {
    let labeled = |want: &str| {
        out.lines.iter().find_map(|l| match l {
            MetricLine::Progress { label, .. } if label == want => line_percent(l),
            _ => None,
        })
    };
    let session = labeled("Session");
    let weekly = labeled("Weekly");
    match (session, weekly) {
        (Some(s), Some(w)) if w >= WEEKLY_ESCALATE_PCT && w > s => Some(w),
        (Some(s), _) => Some(s),
        (None, Some(w)) => Some(w),
        (None, None) => out.lines.iter().find_map(line_percent),
    }
}

/// Plain multi-line human output for the terminal.
pub fn plain(outputs: &[ProviderOutput]) -> String {
    let mut s = String::new();
    for out in outputs {
        let plan = out
            .plan
            .as_deref()
            .map(|p| format!(" ({p})"))
            .unwrap_or_default();
        s.push_str(&format!("{}{}\n", out.display_name, plan));
        for line in &out.lines {
            match line {
                MetricLine::Text { label, value, .. } => {
                    s.push_str(&format!("  {label}: {value}\n"));
                }
                MetricLine::Badge { label, text, .. } => {
                    s.push_str(&format!("  {label}: {text}\n"));
                }
                MetricLine::Progress {
                    label,
                    used,
                    limit,
                    format,
                    resets_at,
                    ..
                } => {
                    s.push_str(&format!(
                        "  {label}: {}{}\n",
                        fmt_progress(*used, *limit, format),
                        reset_suffix(resets_at)
                    ));
                }
                MetricLine::BarChart { label, points, .. } => {
                    s.push_str(&format!("  {label}: {}\n", sparkline(points)));
                }
            }
        }
        s.push('\n');
    }
    s.trim_end().to_string()
}

/// Severity CSS class for Waybar based on a percentage.
fn severity(pct: f64) -> &'static str {
    if pct >= 95.0 {
        "critical"
    } else if pct >= 80.0 {
        "warning"
    } else {
        "ok"
    }
}

/// Waybar custom-module JSON: a compact `{text, tooltip, class, percentage}`.
///
/// `text` is the last-used paid Claude/Codex/Grok provider's primary metric
/// (e.g. "claude 42%"), falling back to highest utilization when no local
/// activity signal is available. The tooltip lists every provider/line.
pub fn waybar(outputs: &[ProviderOutput]) -> serde_json::Value {
    waybar_with_activity(outputs, crate::activity::last_activity_ms)
}

/// Like [`waybar`], but activity timestamps come from `activity` (test seam).
pub fn waybar_with_activity(
    outputs: &[ProviderOutput],
    activity: impl Fn(&str) -> Option<i64>,
) -> serde_json::Value {
    let mut tooltip = String::new();

    // Tooltip: every detected provider and line, unchanged.
    for out in outputs {
        let plan = out
            .plan
            .as_deref()
            .map(|p| format!(" ({p})"))
            .unwrap_or_default();
        tooltip.push_str(&format!("<b>{}{}</b>\n", out.display_name, plan));
        for line in &out.lines {
            match line {
                MetricLine::Progress {
                    label,
                    used,
                    limit,
                    format,
                    resets_at,
                    ..
                } => {
                    tooltip.push_str(&format!(
                        "  {label}: {}{}\n",
                        fmt_progress(*used, *limit, format),
                        reset_suffix(resets_at)
                    ));
                }
                MetricLine::Text { label, value, .. } => {
                    tooltip.push_str(&format!("  {label}: {value}\n"));
                }
                MetricLine::Badge { label, text, .. } => {
                    tooltip.push_str(&format!("  {label}: {text}\n"));
                }
                MetricLine::BarChart { label, points, .. } => {
                    tooltip.push_str(&format!("  {label}: {}\n", sparkline(points)));
                }
            }
        }
        tooltip.push('\n');
    }

    // Bar text: last-used eligible provider when an activity signal exists;
    // otherwise highest Session-anchored utilization (legacy fallback).
    let candidates: Vec<(&ProviderOutput, f64, Option<i64>)> = outputs
        .iter()
        .filter(|out| bar_eligible(out))
        .filter_map(|out| {
            provider_bar_pct(out).map(|pct| {
                let act = activity(out.provider_id.as_str());
                (out, pct, act)
            })
        })
        .collect();

    let pick = candidates
        .iter()
        .filter_map(|(out, pct, act)| act.map(|ms| (*out, *pct, ms)))
        .max_by_key(|(_, _, ms)| *ms)
        .map(|(out, pct, _)| (out, pct))
        .or_else(|| {
            candidates
                .iter()
                .max_by(|(_, a, _), (_, b, _)| {
                    a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(out, pct, _)| (*out, *pct))
        });

    let (text, pct) = match pick {
        Some((out, pct)) => (format!("{} {pct:.0}%", out.provider_id), pct),
        None => ("no data".to_string(), 0.0),
    };
    serde_json::json!({
        "text": text,
        "tooltip": tooltip.trim_end(),
        "class": severity(pct),
        "percentage": pct.round() as i64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BarChartPoint;

    fn sample() -> Vec<ProviderOutput> {
        vec![
            ProviderOutput::new(
                "claude",
                "Claude",
                vec![
                    MetricLine::percent("Session", 12.0, None),
                    MetricLine::percent("Weekly", 81.0, None),
                ],
            )
            .with_plan(Some("Max".into())),
            ProviderOutput::new(
                "codex",
                "Codex",
                vec![MetricLine::percent("Session", 5.0, None)],
            ),
        ]
    }

    #[test]
    fn severity_thresholds() {
        assert_eq!(severity(79.0), "ok");
        assert_eq!(severity(80.0), "warning");
        assert_eq!(severity(94.9), "warning");
        assert_eq!(severity(95.0), "critical");
        assert_eq!(severity(100.0), "critical");
    }

    /// Deterministic bar selection: ignore local activity signals.
    fn waybar_no_activity(outputs: &[ProviderOutput]) -> serde_json::Value {
        waybar_with_activity(outputs, |_| None)
    }

    #[test]
    fn waybar_picks_worst_metric_and_class() {
        let j = waybar_no_activity(&sample());
        assert_eq!(j["text"], "claude 81%");
        assert_eq!(j["class"], "warning");
        assert_eq!(j["percentage"], 81);
        assert!(j["tooltip"]
            .as_str()
            .unwrap()
            .contains("<b>Claude (Max)</b>"));
    }

    #[test]
    fn waybar_empty_is_no_data() {
        let j = waybar_no_activity(&[]);
        assert_eq!(j["text"], "no data");
        assert_eq!(j["class"], "ok");
    }

    #[test]
    fn is_paid_plan_excludes_free_and_none() {
        assert!(is_paid_plan(&Some("Max 20x".into())));
        assert!(is_paid_plan(&Some("Pro".into())));
        assert!(is_paid_plan(&Some("SuperGrok Heavy".into())));
        assert!(is_paid_plan(&Some("X Premium+".into())));
        assert!(!is_paid_plan(&Some("Free".into())));
        assert!(!is_paid_plan(&Some("free".into())));
        assert!(!is_paid_plan(&Some("Free Workspace".into())));
        assert!(!is_paid_plan(&Some("Guest".into())));
        assert!(!is_paid_plan(&Some("".into())));
        assert!(!is_paid_plan(&None));
    }

    #[test]
    fn waybar_bar_anchors_on_session_not_higher_weekly() {
        // The core bug: weekly 45% / session 0% (just reset) must show 0%,
        // not the misleading 45%.
        let outputs = vec![ProviderOutput::new(
            "claude",
            "Claude",
            vec![
                MetricLine::percent("Session", 0.0, None),
                MetricLine::percent("Weekly", 45.0, None),
            ],
        )
        .with_plan(Some("Max 20x".into()))];
        let j = waybar_no_activity(&outputs);
        assert_eq!(j["text"], "claude 0%");
        assert_eq!(j["percentage"], 0);
        assert_eq!(j["class"], "ok");
    }

    #[test]
    fn waybar_escalates_to_weekly_when_weekly_critical() {
        // Weekly near-exhaustion still surfaces over a calm session.
        let outputs = vec![ProviderOutput::new(
            "claude",
            "Claude",
            vec![
                MetricLine::percent("Session", 10.0, None),
                MetricLine::percent("Weekly", 92.0, None),
            ],
        )
        .with_plan(Some("Max 20x".into()))];
        let j = waybar_no_activity(&outputs);
        assert_eq!(j["text"], "claude 92%");
        assert_eq!(j["class"], "warning");
    }

    #[test]
    fn waybar_excludes_free_plan_from_bar_but_keeps_tooltip() {
        // Codex on Free with a high session must not drive the bar; when it's
        // the only provider the bar is "no data" but the tooltip still lists it.
        let outputs = vec![ProviderOutput::new(
            "codex",
            "Codex",
            vec![MetricLine::percent("Session", 88.0, None)],
        )
        .with_plan(Some("Free".into()))];
        let j = waybar_no_activity(&outputs);
        assert_eq!(j["text"], "no data");
        assert!(j["tooltip"]
            .as_str()
            .unwrap()
            .contains("<b>Codex (Free)</b>"));
        assert!(j["tooltip"].as_str().unwrap().contains("Session: 88%"));
    }

    #[test]
    fn waybar_excludes_non_allowlisted_providers() {
        // Copilot is not in the allow-list and must never set the bar, even at
        // 99%; an eligible paid grok at 11% wins instead.
        let outputs = vec![
            ProviderOutput::new(
                "copilot",
                "Copilot",
                vec![MetricLine::percent("Premium", 99.0, None)],
            )
            .with_plan(Some("Individual".into())),
            ProviderOutput::new(
                "grok",
                "Grok",
                vec![MetricLine::percent("Weekly", 11.0, None)],
            )
            .with_plan(Some("X Premium+".into())),
        ];
        let j = waybar_no_activity(&outputs);
        assert_eq!(j["text"], "grok 11%");
        assert!(j["tooltip"].as_str().unwrap().contains("<b>Copilot"));
    }

    #[test]
    fn waybar_grok_uses_single_window_fallback() {
        let outputs = vec![ProviderOutput::new(
            "grok",
            "Grok",
            vec![MetricLine::percent("Weekly", 11.0, None)],
        )
        .with_plan(Some("SuperGrok Heavy".into()))];
        let j = waybar_no_activity(&outputs);
        assert_eq!(j["text"], "grok 11%");
    }

    #[test]
    fn waybar_picks_worst_among_eligible_providers() {
        // Two paid allow-listed providers: the higher Session-anchored value wins
        // when no activity signals are available.
        let outputs = vec![
            ProviderOutput::new(
                "claude",
                "Claude",
                vec![MetricLine::percent("Session", 20.0, None)],
            )
            .with_plan(Some("Max 20x".into())),
            ProviderOutput::new(
                "codex",
                "Codex",
                vec![MetricLine::percent("Session", 60.0, None)],
            )
            .with_plan(Some("Pro".into())),
        ];
        let j = waybar_no_activity(&outputs);
        assert_eq!(j["text"], "codex 60%");
    }

    #[test]
    fn waybar_prefers_last_used_over_worst_utilization() {
        let outputs = vec![
            ProviderOutput::new(
                "claude",
                "Claude",
                vec![
                    MetricLine::percent("Session", 10.0, None),
                    MetricLine::percent("Weekly", 85.0, None),
                ],
            )
            .with_plan(Some("Max 20x".into())),
            ProviderOutput::new(
                "grok",
                "Grok",
                vec![MetricLine::percent("Weekly", 1.0, None)],
            )
            .with_plan(Some("SuperGrok Heavy".into())),
        ];
        // Grok used more recently despite lower utilization.
        let j = waybar_with_activity(&outputs, |id| match id {
            "claude" => Some(1_000),
            "grok" => Some(9_000),
            _ => None,
        });
        assert_eq!(j["text"], "grok 1%");
        assert_eq!(j["percentage"], 1);
        assert_eq!(j["class"], "ok");
        // Tooltip still lists both.
        let tip = j["tooltip"].as_str().unwrap();
        assert!(tip.contains("Claude"));
        assert!(tip.contains("Grok"));
    }

    #[test]
    fn waybar_falls_back_to_worst_when_no_activity() {
        let outputs = vec![
            ProviderOutput::new(
                "claude",
                "Claude",
                vec![MetricLine::percent("Session", 20.0, None)],
            )
            .with_plan(Some("Max 20x".into())),
            ProviderOutput::new(
                "grok",
                "Grok",
                vec![MetricLine::percent("Weekly", 50.0, None)],
            )
            .with_plan(Some("SuperGrok Heavy".into())),
        ];
        let j = waybar_no_activity(&outputs);
        assert_eq!(j["text"], "grok 50%");
    }

    #[test]
    fn waybar_ignores_activity_for_ineligible_providers() {
        // Free codex is more "recent" but must not drive the bar.
        let outputs = vec![
            ProviderOutput::new(
                "codex",
                "Codex",
                vec![MetricLine::percent("Session", 99.0, None)],
            )
            .with_plan(Some("Free".into())),
            ProviderOutput::new(
                "claude",
                "Claude",
                vec![MetricLine::percent("Session", 15.0, None)],
            )
            .with_plan(Some("Max 20x".into())),
        ];
        let j = waybar_with_activity(&outputs, |id| match id {
            "codex" => Some(99_000),
            "claude" => Some(1_000),
            _ => None,
        });
        assert_eq!(j["text"], "claude 15%");
    }

    #[test]
    fn waybar_skips_errored_eligible_provider() {
        let outputs = vec![ProviderOutput::error("claude", "Claude", "boom")];
        let j = waybar_no_activity(&outputs);
        assert_eq!(j["text"], "no data");
    }

    #[test]
    fn plain_renders_lines() {
        let s = plain(&sample());
        assert!(s.contains("Claude (Max)"));
        assert!(s.contains("Session: 12%"));
        assert!(s.contains("Weekly: 81%"));
    }

    #[test]
    fn sparkline_scales_to_max() {
        let pts = vec![
            BarChartPoint {
                label: "a".into(),
                value: 0.0,
                value_label: None,
            },
            BarChartPoint {
                label: "b".into(),
                value: 10.0,
                value_label: None,
            },
        ];
        let s = sparkline(&pts);
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(chars.len(), 2);
        assert_eq!(chars[0], '▁'); // min
        assert_eq!(chars[1], '█'); // max
    }

    #[test]
    fn sparkline_empty_and_all_zero() {
        assert_eq!(sparkline(&[]), "");
        let pts = vec![
            BarChartPoint {
                label: "a".into(),
                value: 0.0,
                value_label: None,
            },
            BarChartPoint {
                label: "b".into(),
                value: 0.0,
                value_label: None,
            },
        ];
        assert_eq!(sparkline(&pts), "▁▁");
    }

    #[test]
    fn fmt_progress_variants() {
        assert_eq!(fmt_progress(42.4, 100.0, &ProgressFormat::Percent), "42%");
        assert_eq!(
            fmt_progress(1.0, 10.0, &ProgressFormat::Dollars),
            "$1.00 / $10.00"
        );
        assert_eq!(
            fmt_progress(
                3.0,
                5.0,
                &ProgressFormat::Count {
                    suffix: "reqs".into()
                }
            ),
            "3/5 reqs"
        );
    }

    #[test]
    fn reset_suffix_renders_for_future_only() {
        let future =
            time::OffsetDateTime::from_unix_timestamp((crate::util::now_ms() / 1000) + 7200)
                .unwrap()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap();
        assert!(reset_suffix(&Some(future)).contains("resets in"));
        assert_eq!(reset_suffix(&Some("2000-01-01T00:00:00Z".into())), "");
        assert_eq!(reset_suffix(&None), "");
    }

    #[test]
    fn waybar_tooltip_renders_barchart_as_sparkline_not_null() {
        let outputs = vec![ProviderOutput::new(
            "claude",
            "Claude",
            vec![
                MetricLine::text("Last 30 Days", "~$5.00 · 1M tokens"),
                MetricLine::bar_chart(
                    "Usage Trend",
                    vec![
                        BarChartPoint {
                            label: "a".into(),
                            value: 0.0,
                            value_label: None,
                        },
                        BarChartPoint {
                            label: "b".into(),
                            value: 4.0,
                            value_label: None,
                        },
                    ],
                    None,
                ),
            ],
        )];
        let j = waybar(&outputs);
        let tip = j["tooltip"].as_str().unwrap();
        assert!(tip.contains("Usage Trend: ▁"), "tooltip: {tip}");
        assert!(
            !tip.contains("null"),
            "tooltip must not contain null: {tip}"
        );
        let s = plain(&outputs);
        assert!(s.contains("Usage Trend: ▁"));
    }
}
