//! Pure formatting helpers for the system tray (no GUI deps).
//!
//! Always compiled so unit tests cover tooltip / severity without `feature = "tray"`.
//! Production callers live in `tray.rs` (feature-gated). Without that feature some
//! helpers are only hit by unit tests (and a few only by the tray UI), so clippy
//! would otherwise flag dead_code under default features / `-D warnings`.

#![cfg_attr(not(feature = "tray"), allow(dead_code))]

use crate::model::{MetricLine, ProgressFormat, ProviderOutput};
use crate::output;

/// Tray icon / notification band from a utilization percentage (used 0–100).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraySeverity {
    Ok,
    Warning,
    Critical,
    /// Capture/proxy ports down (overrides quota severity for icon color).
    ProxyDown,
}

impl TraySeverity {
    /// RGBA tint applied to the white Behelit master icon.
    pub fn tint_rgba(self) -> [u8; 4] {
        match self {
            TraySeverity::Ok => [180, 255, 200, 255],
            TraySeverity::Warning => [255, 200, 80, 255],
            TraySeverity::Critical => [255, 90, 90, 255],
            TraySeverity::ProxyDown => [255, 60, 60, 255],
        }
    }
}

/// Remaining percent for a 0–100 "used" quota line.
pub fn remaining_pct(used: f64) -> f64 {
    (100.0 - used).clamp(0.0, 100.0)
}

/// Highest utilization among percent progress lines (None if none).
pub fn max_used_pct(outputs: &[ProviderOutput]) -> Option<f64> {
    outputs
        .iter()
        .filter(|o| !o.has_error())
        .flat_map(|o| o.lines.iter())
        .filter_map(output::line_percent_public)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

/// Map capture health + max used% → tray severity.
pub fn severity(capture_up: bool, max_used: Option<f64>) -> TraySeverity {
    if !capture_up {
        return TraySeverity::ProxyDown;
    }
    match max_used {
        Some(p) if p >= 95.0 => TraySeverity::Critical,
        Some(p) if p >= 80.0 => TraySeverity::Warning,
        _ => TraySeverity::Ok,
    }
}

/// Multi-line tooltip for the tray icon.
pub fn format_tooltip(
    outputs: &[ProviderOutput],
    capture_up: bool,
    update_note: Option<&str>,
) -> String {
    let mut lines = Vec::new();
    let cap = if capture_up {
        "Capture: UP · Grok :18736 · xAI :18737"
    } else {
        "Capture: DOWN · run spanreed capture ensure"
    };
    lines.push(cap.to_string());

    for out in outputs {
        if out.has_error() {
            lines.push(format!("{}: error", out.display_name));
            continue;
        }
        let plan = out
            .plan
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .map(|p| format!(" ({p})"))
            .unwrap_or_default();
        let mut bits = Vec::new();
        for line in &out.lines {
            if let Some(s) = format_quota_bit(line) {
                bits.push(s);
            }
        }
        if bits.is_empty() {
            lines.push(format!("{}{plan}", out.display_name));
        } else {
            lines.push(format!("{}{plan}  {}", out.display_name, bits.join(" · ")));
        }
    }

    if let Some(note) = update_note {
        if !note.is_empty() {
            lines.push(note.to_string());
        }
    }
    lines.join("\n")
}

fn format_quota_bit(line: &MetricLine) -> Option<String> {
    match line {
        MetricLine::Progress {
            label,
            used,
            format: ProgressFormat::Percent,
            ..
        } => {
            let left = remaining_pct(*used);
            Some(format!("{label} {left:.0}% left"))
        }
        MetricLine::Progress {
            label,
            used,
            limit,
            format: ProgressFormat::Dollars,
            ..
        } => Some(format!("{label} ${used:.2}/${limit:.2}")),
        MetricLine::Progress {
            label,
            used,
            limit,
            format: ProgressFormat::Count { suffix },
            ..
        } => Some(format!("{label} {used:.0}/{limit:.0} {suffix}")),
        _ => None,
    }
}

/// Whether any provider crossed into a higher severity band vs previous max used%.
pub fn crossed_threshold(prev: Option<f64>, next: Option<f64>) -> Option<&'static str> {
    let n = next?;
    let p = prev.unwrap_or(0.0);
    if n >= 95.0 && p < 95.0 {
        Some("critical")
    } else if n >= 80.0 && p < 80.0 {
        Some("warning")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MetricLine;

    #[test]
    fn remaining() {
        assert_eq!(remaining_pct(52.0), 48.0);
        assert_eq!(remaining_pct(0.0), 100.0);
        assert_eq!(remaining_pct(100.0), 0.0);
    }

    #[test]
    fn tooltip_remaining() {
        let out = ProviderOutput::new(
            "grok",
            "Grok",
            vec![MetricLine::percent("Weekly", 52.0, None)],
        )
        .with_plan(Some("Heavy".into()));
        let t = format_tooltip(&[out], true, Some("Update: 0.0.3 available"));
        assert!(t.contains("Capture: UP"));
        assert!(t.contains("Weekly 48% left"));
        assert!(t.contains("Update: 0.0.3"));
    }

    #[test]
    fn severity_proxy() {
        assert_eq!(severity(false, Some(10.0)), TraySeverity::ProxyDown);
        assert_eq!(severity(true, Some(90.0)), TraySeverity::Warning);
        assert_eq!(severity(true, Some(96.0)), TraySeverity::Critical);
        assert_eq!(severity(true, Some(10.0)), TraySeverity::Ok);
    }

    #[test]
    fn threshold_cross() {
        assert_eq!(crossed_threshold(Some(70.0), Some(82.0)), Some("warning"));
        assert_eq!(crossed_threshold(Some(90.0), Some(96.0)), Some("critical"));
        assert_eq!(crossed_threshold(Some(85.0), Some(90.0)), None);
    }

    #[test]
    fn menu_labels_documented_in_tooltip_contract() {
        // Menu actions are owned by tray.rs (feature-gated); tooltip contract
        // always includes capture line + remaining quotas for percent lines.
        let out = ProviderOutput::new(
            "claude",
            "Claude",
            vec![
                MetricLine::percent("Session", 4.0, None),
                MetricLine::percent("Weekly", 1.0, None),
            ],
        )
        .with_plan(Some("Max 20x".into()));
        let t = format_tooltip(&[out], false, None);
        assert!(t.contains("Capture: DOWN"));
        assert!(t.contains("Session 96% left"));
        assert!(t.contains("Weekly 99% left"));
        assert!(t.contains("Max 20x"));
    }

    #[test]
    fn max_used_and_tint_used_by_tray_contract() {
        let out = ProviderOutput::new(
            "grok",
            "Grok",
            vec![MetricLine::percent("Weekly", 90.0, None)],
        );
        assert_eq!(max_used_pct(&[out]), Some(90.0));
        let rgba = TraySeverity::Warning.tint_rgba();
        assert_eq!(rgba[3], 255);
        assert!(rgba[0] > 0);
    }
}
