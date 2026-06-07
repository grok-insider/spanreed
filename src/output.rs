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
                    ..
                } => {
                    s.push_str(&format!(
                        "  {label}: {}\n",
                        fmt_progress(*used, *limit, format)
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
/// `text` is the single highest-utilization primary metric across providers
/// (e.g. "claude 42%"). The tooltip lists every provider/line.
pub fn waybar(outputs: &[ProviderOutput]) -> serde_json::Value {
    let mut worst: Option<(String, f64)> = None;
    let mut tooltip = String::new();

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
                    ..
                } => {
                    let pct = line_percent(line).unwrap_or(0.0);
                    tooltip.push_str(&format!(
                        "  {label}: {}\n",
                        fmt_progress(*used, *limit, format)
                    ));
                    let candidate = (format!("{} {:.0}%", out.provider_id, pct), pct);
                    if worst.as_ref().map(|(_, p)| pct > *p).unwrap_or(true) {
                        worst = Some(candidate);
                    }
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

    let (text, pct) = worst.unwrap_or_else(|| ("no data".to_string(), 0.0));
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

    #[test]
    fn waybar_picks_worst_metric_and_class() {
        let j = waybar(&sample());
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
        let j = waybar(&[]);
        assert_eq!(j["text"], "no data");
        assert_eq!(j["class"], "ok");
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
}
