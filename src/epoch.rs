//! Detect unscheduled weekly pool resets (gift / outage / user-held).
//!
//! A scheduled rollover at `resets_at` is a new epoch but **not** early.
//! An early reset is: clock still before the old end, used collapsed, and
//! `resets_at` moved. That must not be stitched into the previous week's
//! at 100% density.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::creds;
use crate::util;

/// Slack: if we are within this of the old end, treat as scheduled rollover.
pub const SCHEDULED_SLACK_MS: i64 = 60 * 60 * 1000;
/// Used must have been at least this before we call a collapse a reset.
pub const MIN_USED_BEFORE: f64 = 15.0;
/// Used at or below this after the jump counts as collapsed.
pub const MAX_USED_AFTER: f64 = 5.0;

#[derive(Debug, Clone, PartialEq)]
pub struct WindowSnap {
    pub resets_at: String,
    pub used: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetKind {
    /// Unscheduled: now still before the previous end.
    Early,
    /// Clock ≈ previous end — normal weekly rollover.
    Scheduled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResetEvent {
    pub ts_ms: i64,
    pub provider: String,
    /// Always `"early"` locally; API may relabel broadcast vs user.
    pub kind: String,
    pub prev_end: String,
    pub new_end: String,
    pub used_before: f64,
    pub used_after: f64,
}

/// Classify a Weekly window change.
pub fn classify_reset(prev: &WindowSnap, curr: &WindowSnap, now_ms: i64) -> Option<ResetKind> {
    let old_end = util::parse_iso_dt(&prev.resets_at)
        .map(|d| d.unix_timestamp() * 1000)
        .or_else(|| parse_ms_loose(&prev.resets_at))?;
    let new_end = util::parse_iso_dt(&curr.resets_at)
        .map(|d| d.unix_timestamp() * 1000)
        .or_else(|| parse_ms_loose(&curr.resets_at))?;
    if prev.resets_at == curr.resets_at || old_end == new_end {
        return None;
    }
    let used_collapsed = prev.used >= MIN_USED_BEFORE && curr.used <= MAX_USED_AFTER;
    let used_zeroed = curr.used <= MAX_USED_AFTER && (prev.used - curr.used) >= 10.0;
    if !(used_collapsed || used_zeroed) && new_end <= old_end {
        return None;
    }
    if now_ms + SCHEDULED_SLACK_MS < old_end && (used_collapsed || used_zeroed) {
        return Some(ResetKind::Early);
    }
    if (now_ms - old_end).abs() <= SCHEDULED_SLACK_MS * 6 {
        return Some(ResetKind::Scheduled);
    }
    if used_collapsed || used_zeroed {
        Some(ResetKind::Early)
    } else {
        Some(ResetKind::Scheduled)
    }
}

fn parse_ms_loose(s: &str) -> Option<i64> {
    s.parse::<i64>().ok().filter(|n| *n > 1_000_000_000_000)
}

pub fn reset_events_path() -> PathBuf {
    creds::data_home()
        .join("spanreed")
        .join("reset-events.jsonl")
}

pub fn append_event(ev: &ResetEvent) -> Result<(), String> {
    let path = reset_events_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let line = serde_json::to_string(ev).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
}

/// Events in the last `since_ms` (for share payload).
/// Compare this probe to last history Weekly sample; persist early resets.
pub fn note_jumps_from_outputs(outputs: &[crate::model::ProviderOutput]) {
    let hist = crate::history::read_samples(&crate::history::history_path(), None);
    let mut last: std::collections::HashMap<String, crate::history::HistorySample> =
        std::collections::HashMap::new();
    for s in hist {
        if s.label == "Weekly" {
            last.insert(s.provider.clone(), s);
        }
    }
    let now = util::now_ms();
    for o in outputs {
        let Some(curr) = weekly_snap(o) else {
            continue;
        };
        let Some(prev_h) = last.get(&o.provider_id) else {
            continue;
        };
        let Some(prev_end) = prev_h.resets_at.clone() else {
            continue;
        };
        let prev = WindowSnap {
            resets_at: prev_end,
            used: prev_h.used,
        };
        if classify_reset(&prev, &curr, now) != Some(ResetKind::Early) {
            continue;
        }
        let ev = ResetEvent {
            ts_ms: now,
            provider: o.provider_id.clone(),
            kind: "early".into(),
            prev_end: prev.resets_at,
            new_end: curr.resets_at,
            used_before: prev.used,
            used_after: curr.used,
        };
        let _ = append_event(&ev);
    }
}

fn weekly_snap(o: &crate::model::ProviderOutput) -> Option<WindowSnap> {
    use crate::model::{MetricKind, MetricLine, ProgressFormat};
    for line in &o.lines {
        if let MetricLine::Progress {
            kind,
            label,
            used,
            resets_at,
            format: ProgressFormat::Percent,
            ..
        } = line
        {
            if *kind == MetricKind::Quota && label.eq_ignore_ascii_case("Weekly") {
                let end = resets_at.clone()?;
                return Some(WindowSnap {
                    resets_at: end,
                    used: *used,
                });
            }
        }
    }
    None
}

pub fn recent_events(since_ms: i64) -> Vec<ResetEvent> {
    let Ok(raw) = std::fs::read_to_string(reset_events_path()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in raw.lines() {
        let Ok(ev) = serde_json::from_str::<ResetEvent>(line) else {
            continue;
        };
        if ev.ts_ms >= since_ms {
            out.push(ev);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iso(day: u8) -> String {
        format!("2026-08-{day:02}T00:00:00Z")
    }

    fn ms_on(day: u8) -> i64 {
        util::parse_iso_dt(&iso(day)).unwrap().unix_timestamp() * 1000
    }

    #[test]
    fn generous_reset_on_the_15th_is_early() {
        let prev = WindowSnap {
            resets_at: iso(17),
            used: 70.0,
        };
        let curr = WindowSnap {
            resets_at: iso(22),
            used: 0.0,
        };
        let now = ms_on(15);
        assert_eq!(classify_reset(&prev, &curr, now), Some(ResetKind::Early));
    }

    #[test]
    fn scheduled_rollover_on_the_17th_is_not_early() {
        let prev = WindowSnap {
            resets_at: iso(17),
            used: 90.0,
        };
        let curr = WindowSnap {
            resets_at: iso(24),
            used: 0.0,
        };
        let now = ms_on(17);
        assert_eq!(
            classify_reset(&prev, &curr, now),
            Some(ResetKind::Scheduled)
        );
    }

    #[test]
    fn no_change_is_none() {
        let w = WindowSnap {
            resets_at: iso(17),
            used: 40.0,
        };
        assert_eq!(classify_reset(&w, &w, ms_on(15)), None);
    }
}
