//! First-seen weekly pool % per provider and billing week.
//!
//! Local $ only cover `pct_start → pct_now`. Storing the start % on the first
//! probe (even with $0) lets share/forecast scale by that span instead of
//! pretending the week began at 0%.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::creds;
use crate::model::{MetricKind, MetricLine, ProgressFormat, ProviderOutput};

const FILE: &str = "pool-baselines.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PoolBaseline {
    pub week_id: String,
    pub pct: f64,
    pub ts_ms: i64,
}

pub type BaselineMap = HashMap<String, PoolBaseline>;

fn path() -> PathBuf {
    crate::app::data_dir().join(FILE)
}

pub fn load() -> BaselineMap {
    let Some(raw) = creds::read_file(&path()) else {
        return HashMap::new();
    };
    serde_json::from_str(raw.trim()).unwrap_or_default()
}

pub fn save(map: &BaselineMap) -> Result<(), String> {
    let p = path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
    std::fs::write(p, format!("{body}\n")).map_err(|e| e.to_string())
}

/// Insert only if this provider has no row for `week_id`. New week replaces.
pub fn upsert(map: &mut BaselineMap, provider: &str, week_id: &str, pct: f64, ts_ms: i64) -> bool {
    if !pct.is_finite() || !(0.0..=100.0).contains(&pct) {
        return false;
    }
    match map.get(provider) {
        Some(b) if b.week_id == week_id => false,
        _ => {
            map.insert(
                provider.to_string(),
                PoolBaseline {
                    week_id: week_id.to_string(),
                    pct,
                    ts_ms,
                },
            );
            true
        }
    }
}

pub fn pct_for(map: &BaselineMap, provider: &str, week_id: &str) -> Option<f64> {
    map.get(provider)
        .filter(|b| b.week_id == week_id)
        .map(|b| b.pct)
}

/// Record first Weekly % this week after a probe (best-effort disk).
pub fn note_from_output(o: &ProviderOutput) {
    let Some((week_id, pct)) = week_and_pct(o) else {
        return;
    };
    let mut map = load();
    if upsert(
        &mut map,
        &o.provider_id,
        &week_id,
        pct,
        crate::util::now_ms(),
    ) {
        let _ = save(&map);
    }
}

pub fn baseline_pct(provider: &str, week_id: &str) -> Option<f64> {
    pct_for(&load(), provider, week_id)
}

pub fn week_and_pct(o: &ProviderOutput) -> Option<(String, f64)> {
    let (week, pct) = primary_weekly(o)?;
    Some((week, pct))
}

fn primary_weekly(o: &ProviderOutput) -> Option<(String, f64)> {
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
            if *kind != MetricKind::Quota {
                continue;
            }
            if label.eq_ignore_ascii_case("Weekly") {
                let week = resets_at
                    .as_deref()
                    .map(|s| format!("reset:{s}"))
                    .unwrap_or_else(|| format!("iso:{}", iso_week_fallback()));
                return Some((week, *used));
            }
        }
    }
    None
}

fn iso_week_fallback() -> String {
    use time::OffsetDateTime;
    let now = OffsetDateTime::now_utc();
    let (y, w, _) = now.to_iso_week_date();
    format!("{y}-W{w:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_write_kept_same_week() {
        let mut m = HashMap::new();
        assert!(upsert(&mut m, "grok", "w1", 10.0, 1));
        assert!(!upsert(&mut m, "grok", "w1", 40.0, 2));
        assert_eq!(pct_for(&m, "grok", "w1"), Some(10.0));
    }

    #[test]
    fn new_week_replaces() {
        let mut m = HashMap::new();
        upsert(&mut m, "grok", "w1", 10.0, 1);
        assert!(upsert(&mut m, "grok", "w2", 3.0, 2));
        assert_eq!(pct_for(&m, "grok", "w2"), Some(3.0));
        assert_eq!(pct_for(&m, "grok", "w1"), None);
    }

    #[test]
    fn providers_independent() {
        let mut m = HashMap::new();
        upsert(&mut m, "grok", "w1", 10.0, 1);
        upsert(&mut m, "codex", "w1", 5.0, 2);
        assert_eq!(pct_for(&m, "grok", "w1"), Some(10.0));
        assert_eq!(pct_for(&m, "codex", "w1"), Some(5.0));
    }
}
