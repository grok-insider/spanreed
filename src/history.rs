//! Append-only usage history for rate-limit progress metrics.
//!
//! Written on `spanreed serve` refresh (and optionally when
//! `SPANREED_HISTORY=1` is set). Segmented by epoch via `resets_at`: a material
//! change is recorded as `event: "reset"` so force-resets are not mixed into one
//! continuous series.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::creds;
use crate::model::{MetricLine, ProgressFormat, ProviderOutput};
use crate::util;

/// Max age of samples retained when rotating (days).
const RETAIN_DAYS: i64 = 90;
const DAY_MS: i64 = 86_400_000;
/// Soft size cap before rewrite (bytes).
const MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistorySample {
    pub ts_ms: i64,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub label: String,
    pub used: f64,
    pub limit: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_start_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_window_secs: Option<i64>,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
}

impl HistorySample {
    fn key(&self) -> (String, String) {
        (self.provider.clone(), self.label.clone())
    }
}

/// Default history path under XDG data.
pub fn history_path() -> PathBuf {
    creds::data_home()
        .join("spanreed")
        .join("usage-history.jsonl")
}

/// Extract percent progress samples from probe outputs.
pub fn samples_from_outputs(outputs: &[ProviderOutput], ts_ms: i64) -> Vec<HistorySample> {
    let mut out = Vec::new();
    for p in outputs {
        for line in &p.lines {
            let MetricLine::Progress {
                label,
                used,
                limit,
                format,
                resets_at,
                ..
            } = line
            else {
                continue;
            };
            if !matches!(format, ProgressFormat::Percent) {
                continue;
            }
            // Session + Weekly pools for limit / force-reset tracking.
            if label != "Weekly" && label != "Session" {
                continue;
            }
            out.push(HistorySample {
                ts_ms,
                provider: p.provider_id.clone(),
                plan: p.plan.clone(),
                label: label.clone(),
                used: *used,
                limit: *limit,
                resets_at: resets_at.clone(),
                window_start_ms: None,
                limit_window_secs: None,
                kind: "percent".into(),
                event: None,
            });
        }
    }
    out
}

/// Whether `curr` should be treated as a new rate-limit epoch vs `prev`.
pub fn is_reset_event(prev: &HistorySample, curr: &HistorySample) -> bool {
    matches!(
        (&prev.resets_at, &curr.resets_at),
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() && a != b
    )
}

/// Whether we should skip writing `curr` because nothing material changed.
pub fn is_duplicate(prev: &HistorySample, curr: &HistorySample) -> bool {
    if is_reset_event(prev, curr) {
        return false;
    }
    same_used(prev.used, curr.used) && prev.resets_at == curr.resets_at
}

fn same_used(a: f64, b: f64) -> bool {
    (a * 10.0).round() == (b * 10.0).round()
}

/// Apply reset event flag and decide write eligibility against last known sample.
pub fn prepare_sample(
    prev: Option<&HistorySample>,
    mut curr: HistorySample,
) -> Option<HistorySample> {
    if let Some(p) = prev {
        if is_duplicate(p, &curr) {
            return None;
        }
        if is_reset_event(p, &curr) {
            curr.event = Some("reset".into());
        }
    }
    Some(curr)
}

/// Append samples to `path`, deduping against the last line per (provider,label).
pub fn append_samples(path: &Path, samples: &[HistorySample]) -> Result<usize, String> {
    if samples.is_empty() {
        return Ok(0);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir history: {e}"))?;
    }
    let last_by_key = last_samples_map(path);
    let mut written = 0usize;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open history: {e}"))?;
    for s in samples {
        let prev = last_by_key.get(&s.key());
        let Some(to_write) = prepare_sample(prev, s.clone()) else {
            continue;
        };
        let line = serde_json::to_string(&to_write).map_err(|e| e.to_string())?;
        writeln!(f, "{line}").map_err(|e| format!("write history: {e}"))?;
        written += 1;
    }
    drop(f);
    maybe_rotate(path)?;
    Ok(written)
}

/// Record progress metrics from a full probe into the default history file.
pub fn record(outputs: &[ProviderOutput]) {
    let samples = samples_from_outputs(outputs, util::now_ms());
    if samples.is_empty() {
        return;
    }
    if let Err(e) = append_samples(&history_path(), &samples) {
        log::debug!("history record: {e}");
    }
}

/// True when serve should always record, or probe when SPANREED_HISTORY=1.
pub fn should_record_on_probe() -> bool {
    creds::env("SPANREED_HISTORY").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Read samples, optionally filtered by provider id, newest last.
pub fn read_samples(path: &Path, provider: Option<&str>) -> Vec<HistorySample> {
    let Ok(f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(s) = serde_json::from_str::<HistorySample>(line) else {
            continue;
        };
        if let Some(p) = provider {
            if s.provider != p {
                continue;
            }
        }
        out.push(s);
    }
    out
}

/// Human-readable table for `spanreed history`.
pub fn format_table(samples: &[HistorySample]) -> String {
    if samples.is_empty() {
        return "No usage history yet. Run `spanreed serve` (or SPANREED_HISTORY=1 spanreed probe).\n"
            .into();
    }
    let mut s = String::new();
    s.push_str("ts_ms\tprovider\tlabel\tused\tresets_at\tevent\n");
    for r in samples {
        let resets = r.resets_at.as_deref().unwrap_or("-");
        let event = r.event.as_deref().unwrap_or("-");
        s.push_str(&format!(
            "{}\t{}\t{}\t{:.1}\t{}\t{}\n",
            r.ts_ms, r.provider, r.label, r.used, resets, event
        ));
    }
    s
}

fn last_samples_map(path: &Path) -> std::collections::HashMap<(String, String), HistorySample> {
    let mut map = std::collections::HashMap::new();
    for s in read_samples(path, None) {
        map.insert(s.key(), s);
    }
    map
}

fn maybe_rotate(path: &Path) -> Result<(), String> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    // Only rewrite when the file is large; drop samples older than RETAIN_DAYS.
    if meta.len() <= MAX_BYTES {
        return Ok(());
    }
    let all = read_samples(path, None);
    let cutoff = util::now_ms() - RETAIN_DAYS * DAY_MS;
    let mut kept: Vec<_> = all.iter().filter(|s| s.ts_ms >= cutoff).cloned().collect();
    // Never wipe the file completely (clock skew / synthetic timestamps).
    if kept.is_empty() {
        kept = all.into_iter().rev().take(1000).collect();
        kept.reverse();
    }
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|e| format!("rotate open: {e}"))?;
        for s in &kept {
            let line = serde_json::to_string(s).map_err(|e| e.to_string())?;
            writeln!(f, "{line}").map_err(|e| format!("rotate write: {e}"))?;
        }
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("rotate rename: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MetricLine;

    fn sample(provider: &str, label: &str, used: f64, resets: &str, ts: i64) -> HistorySample {
        HistorySample {
            ts_ms: ts,
            provider: provider.into(),
            plan: Some("Pro".into()),
            label: label.into(),
            used,
            limit: 100.0,
            resets_at: Some(resets.into()),
            window_start_ms: None,
            limit_window_secs: Some(604800),
            kind: "percent".into(),
            event: None,
        }
    }

    #[test]
    fn samples_from_outputs_picks_percent_progress() {
        let out = ProviderOutput::new(
            "codex",
            "Codex",
            vec![
                MetricLine::percent("Session", 10.0, Some("2026-08-01T00:00:00Z".into())),
                MetricLine::percent("Weekly", 80.0, Some("2026-08-05T22:00:00Z".into())),
                MetricLine::text("Last 30 Days", "1B tokens"),
            ],
        )
        .with_plan(Some("Pro".into()));
        let samples = samples_from_outputs(&[out], 1000);
        assert_eq!(samples.len(), 2);
        assert!(samples
            .iter()
            .any(|s| s.label == "Weekly" && s.used == 80.0));
        assert!(samples.iter().any(|s| s.label == "Session"));
    }

    #[test]
    fn reset_event_when_resets_at_jumps() {
        let a = sample("codex", "Weekly", 80.0, "2026-08-01T00:00:00Z", 1);
        let b = sample("codex", "Weekly", 0.0, "2026-08-08T00:00:00Z", 2);
        assert!(is_reset_event(&a, &b));
        let prepared = prepare_sample(Some(&a), b).unwrap();
        assert_eq!(prepared.event.as_deref(), Some("reset"));
    }

    #[test]
    fn dedup_skips_same_used_and_resets() {
        let a = sample("grok", "Weekly", 44.1, "2026-08-01T00:00:00Z", 1);
        let b = sample("grok", "Weekly", 44.14, "2026-08-01T00:00:00Z", 2); // rounds to 44.1
        assert!(is_duplicate(&a, &b));
        assert!(prepare_sample(Some(&a), b).is_none());
    }

    #[test]
    fn used_drop_without_resets_change_is_not_reset() {
        let a = sample("codex", "Weekly", 80.0, "2026-08-01T00:00:00Z", 1);
        let b = sample("codex", "Weekly", 0.0, "2026-08-01T00:00:00Z", 2);
        assert!(!is_reset_event(&a, &b));
        let prepared = prepare_sample(Some(&a), b).unwrap();
        assert!(prepared.event.is_none());
    }

    #[test]
    fn append_writes_reset_and_skips_dup() {
        let dir = std::env::temp_dir().join(format!(
            "spanreed-history-test-{}-{}",
            std::process::id(),
            util::now_ms()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("usage-history.jsonl");

        let a = sample("codex", "Weekly", 80.0, "T1", 100);
        let n = append_samples(&path, std::slice::from_ref(&a)).unwrap();
        assert_eq!(n, 1);

        let dup = sample("codex", "Weekly", 80.0, "T1", 200);
        let n = append_samples(&path, &[dup]).unwrap();
        assert_eq!(n, 0);

        let forced = sample("codex", "Weekly", 0.0, "T2", 300);
        let n = append_samples(&path, &[forced]).unwrap();
        assert_eq!(n, 1);

        let all = read_samples(&path, Some("codex"));
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].event.as_deref(), Some("reset"));
        assert_eq!(all[1].used, 0.0);

        let table = format_table(&all);
        assert!(table.contains("reset"));
        assert!(table.contains("codex"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
