//! Render provider outputs in the formats the CLI exposes.

use crate::model::{MetricLine, ProgressFormat, ProviderOutput};

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
