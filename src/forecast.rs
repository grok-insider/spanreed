//! Week/month usage forecasts from pool-% density (preferred) or wall-clock pace.
//!
//! When local cost data is incomplete (e.g. mid-week wipe), we calibrate
//! tokens/$ per point of Weekly pool usage: after Weekly % rises by ≥Δ, use
//! that band to extrapolate to 100% of the pool.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::creds;
use crate::model::{MetricKind, MetricLine};
use crate::util;

/// Minimum Weekly % increase between samples before we recompute density.
pub const MIN_PCT_DELTA: f64 = 3.0;
/// Minimum Weekly % for one-shot density (tokens/pct_now) when no band yet.
pub const MIN_PCT_ONESHOT: f64 = 5.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PctSample {
    pub provider: String,
    pub week_id: String,
    pub ts_ms: i64,
    pub weekly_pct: f64,
    pub tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeekSnapshot {
    pub provider: String,
    pub week_end_ms: i64,
    pub tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeekProjection {
    pub tokens: u64,
    pub cost_usd: f64,
    pub low_confidence: bool,
}

/// Pure: density from two samples on the same week (pct2 > pct1).
pub fn density_from_band(a: &PctSample, b: &PctSample) -> Option<(f64, f64)> {
    if a.week_id != b.week_id {
        return None;
    }
    let d_pct = b.weekly_pct - a.weekly_pct;
    if d_pct < MIN_PCT_DELTA - f64::EPSILON {
        return None;
    }
    if b.tokens < a.tokens {
        return None;
    }
    let d_tok = (b.tokens - a.tokens) as f64;
    let d_cost = (b.cost_usd - a.cost_usd).max(0.0);
    Some((d_tok / d_pct, d_cost / d_pct))
}

/// One-shot tokens/cost per % from origin (only when pct is high enough).
pub fn density_oneshot(tokens: u64, cost_usd: f64, weekly_pct: f64) -> Option<(f64, f64)> {
    if weekly_pct < MIN_PCT_ONESHOT {
        return None;
    }
    Some((tokens as f64 / weekly_pct, cost_usd / weekly_pct))
}

/// Project full week to 100% pool using density.
pub fn project_week_to_full(
    tokens_now: u64,
    cost_now: f64,
    weekly_pct: f64,
    tokens_per_pct: f64,
    cost_per_pct: f64,
    low_confidence: bool,
) -> WeekProjection {
    let remaining = (100.0 - weekly_pct).max(0.0);
    let add_tok = (tokens_per_pct * remaining).max(0.0).round() as u64;
    let add_cost = (cost_per_pct * remaining).max(0.0);
    WeekProjection {
        tokens: tokens_now.saturating_add(add_tok),
        cost_usd: cost_now + add_cost,
        low_confidence,
    }
}

/// Smart month: completed weeks + current projection + future weeks × median cost.
pub fn project_month(
    completed: &[(u64, f64)],
    current: &WeekProjection,
    remaining_full_weeks: u32,
) -> (u64, f64) {
    let mut tokens: u64 = completed.iter().map(|(t, _)| *t).sum();
    let mut cost: f64 = completed.iter().map(|(_, c)| *c).sum();
    tokens = tokens.saturating_add(current.tokens);
    cost += current.cost_usd;

    if remaining_full_weeks == 0 {
        return (tokens, cost);
    }

    let mut week_costs: Vec<f64> = completed.iter().map(|(_, c)| *c).collect();
    week_costs.push(current.cost_usd);
    week_costs.retain(|c| *c > 0.0);
    let per_week_cost = if week_costs.is_empty() {
        current.cost_usd
    } else {
        median_f64(&mut week_costs)
    };

    let mut week_tokens: Vec<u64> = completed.iter().map(|(t, _)| *t).collect();
    week_tokens.push(current.tokens);
    week_tokens.retain(|t| *t > 0);
    let per_week_tok = if week_tokens.is_empty() {
        current.tokens
    } else {
        median_u64(&mut week_tokens)
    };

    for _ in 0..remaining_full_weeks {
        tokens = tokens.saturating_add(per_week_tok);
        cost += per_week_cost;
    }
    (tokens, cost)
}

fn median_f64(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn median_u64(v: &mut [u64]) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2
    }
}

fn samples_path() -> PathBuf {
    creds::data_home()
        .join("spanreed")
        .join("pct-samples.jsonl")
}

fn weeks_path() -> PathBuf {
    creds::data_home()
        .join("spanreed")
        .join("cost-weeks.jsonl")
}

/// Append sample and return median density for this week if computable.
pub fn record_sample(sample: &PctSample) -> Option<(f64, f64, bool)> {
    let path = samples_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        if let Ok(line) = serde_json::to_string(sample) {
            let _ = writeln!(f, "{line}");
        }
    }

    let prior = load_samples_for_week(&sample.provider, &sample.week_id);
    // Densities from consecutive pairs with Δpct ≥ MIN
    let mut densities: Vec<(f64, f64)> = Vec::new();
    for w in prior.windows(2) {
        if let Some(d) = density_from_band(&w[0], &w[1]) {
            densities.push(d);
        }
        if let Some(d) = density_from_band(&w[0], sample) {
            densities.push(d);
        }
    }
    if let Some(last) = prior.last() {
        if let Some(d) = density_from_band(last, sample) {
            densities.push(d);
        }
    }

    if !densities.is_empty() {
        let tok = median_f64(&mut densities.iter().map(|(t, _)| *t).collect::<Vec<_>>());
        let cost = median_f64(&mut densities.iter().map(|(_, c)| *c).collect::<Vec<_>>());
        return Some((tok, cost, false));
    }

    density_oneshot(sample.tokens, sample.cost_usd, sample.weekly_pct).map(|(t, c)| (t, c, true))
}

fn load_samples_for_week(provider: &str, week_id: &str) -> Vec<PctSample> {
    let path = samples_path();
    let Ok(f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        let Ok(s) = serde_json::from_str::<PctSample>(&line) else {
            continue;
        };
        if s.provider == provider && s.week_id == week_id {
            out.push(s);
        }
    }
    out.sort_by_key(|a| a.ts_ms);
    out
}

/// Build Cost metric lines for expected week/month when projection available.
pub fn forecast_lines(
    provider: &str,
    week_id: &str,
    weekly_pct: f64,
    tokens_week: u64,
    cost_week: f64,
    week_end_ms: Option<i64>,
) -> Vec<MetricLine> {
    if tokens_week == 0 && cost_week <= 0.0 {
        return Vec::new();
    }
    let sample = PctSample {
        provider: provider.to_string(),
        week_id: week_id.to_string(),
        ts_ms: util::now_ms(),
        weekly_pct: weekly_pct.clamp(0.0, 100.0),
        tokens: tokens_week,
        cost_usd: cost_week,
    };

    let Some((tok_per, cost_per, low)) = record_sample(&sample) else {
        return Vec::new();
    };

    let week = project_week_to_full(
        tokens_week,
        cost_week,
        sample.weekly_pct,
        tok_per,
        cost_per,
        low,
    );

    let mut lines = vec![format_expected("Expected this week", &week)];

    // Smart month from completed week snapshots + current projection.
    let completed = load_week_snapshots(provider);
    let remaining = remaining_full_weeks_in_month(util::now_ms(), week_end_ms);
    let (m_tok, m_cost) = project_month(&completed, &week, remaining);
    lines.push(MetricLine::text(
        MetricKind::Cost,
        "Expected this month",
        format_tokens_cost(m_tok, m_cost, week.low_confidence),
    ));

    // Persist completed weeks when week_end has passed (best-effort).
    if let Some(end) = week_end_ms {
        if util::now_ms() >= end {
            append_week_snapshot(&WeekSnapshot {
                provider: provider.to_string(),
                week_end_ms: end,
                tokens: tokens_week,
                cost_usd: cost_week,
            });
        }
    }

    lines
}

fn format_expected(label: &str, w: &WeekProjection) -> MetricLine {
    MetricLine::text(
        MetricKind::Cost,
        label,
        format_tokens_cost(w.tokens, w.cost_usd, w.low_confidence),
    )
}

fn format_tokens_cost(tokens: u64, cost: f64, low: bool) -> String {
    let tok = util::fmt_tokens(tokens);
    let conf = if low { " (low confidence)" } else { "" };
    if cost > 0.0 {
        format!("~{tok} · ~${cost:.2}{conf}")
    } else {
        format!("~{tok} tokens{conf}")
    }
}

fn load_week_snapshots(provider: &str) -> Vec<(u64, f64)> {
    let path = weeks_path();
    let Ok(f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let now = util::now_ms();
    let month_start = month_start_ms(now);
    let mut out = Vec::new();
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        let Ok(s) = serde_json::from_str::<WeekSnapshot>(&line) else {
            continue;
        };
        if s.provider == provider && s.week_end_ms >= month_start && s.week_end_ms <= now {
            out.push((s.tokens, s.cost_usd));
        }
    }
    out
}

fn append_week_snapshot(s: &WeekSnapshot) {
    let path = weeks_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Skip duplicate week_end
    for (t, _) in load_week_snapshots(&s.provider) {
        let _ = t;
    }
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing.lines().any(|l| {
            serde_json::from_str::<WeekSnapshot>(l)
                .map(|e| e.provider == s.provider && e.week_end_ms == s.week_end_ms)
                .unwrap_or(false)
        }) {
            return;
        }
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        if let Ok(line) = serde_json::to_string(s) {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn month_start_ms(now_ms: i64) -> i64 {
    let sec = now_ms / 1000;
    let Ok(dt) = time::OffsetDateTime::from_unix_timestamp(sec) else {
        return now_ms - 30 * 86_400_000;
    };
    let date = dt.date();
    let first = time::Date::from_calendar_date(date.year(), date.month(), 1).unwrap_or(date);
    let midnight = first
        .with_hms(0, 0, 0)
        .map(|t| t.assume_utc())
        .unwrap_or(dt);
    midnight.unix_timestamp() * 1000
}

/// Full weeks still left in the calendar month after the current week ends.
fn remaining_full_weeks_in_month(now_ms: i64, week_end_ms: Option<i64>) -> u32 {
    let sec = now_ms / 1000;
    let Ok(now) = time::OffsetDateTime::from_unix_timestamp(sec) else {
        return 0;
    };
    let date = now.date();
    let (ny, nm) = if u8::from(date.month()) == 12 {
        (date.year() + 1, time::Month::January)
    } else {
        (
            date.year(),
            time::Month::try_from(u8::from(date.month()) + 1).unwrap_or(date.month()),
        )
    };
    let next_month = time::Date::from_calendar_date(ny, nm, 1)
        .ok()
        .and_then(|d| d.with_hms(0, 0, 0).ok())
        .map(|t| t.assume_utc().unix_timestamp() * 1000)
        .unwrap_or(now_ms + 30 * 86_400_000);

    let after_current = week_end_ms.unwrap_or(now_ms + 7 * 86_400_000).max(now_ms);
    if after_current >= next_month {
        return 0;
    }
    let ms_left = next_month - after_current;
    (ms_left / (7 * 86_400_000)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(pct: f64, tokens: u64, cost: f64) -> PctSample {
        PctSample {
            provider: "grok".into(),
            week_id: "w1".into(),
            ts_ms: 1,
            weekly_pct: pct,
            tokens,
            cost_usd: cost,
        }
    }

    #[test]
    fn density_requires_min_delta() {
        let a = sample(50.0, 80_000_000, 500.0);
        let b = sample(52.0, 83_000_000, 520.0);
        assert!(density_from_band(&a, &b).is_none());
        let c = sample(55.0, 88_000_000, 550.0);
        let (tp, cp) = density_from_band(&a, &c).unwrap();
        assert!((tp - 1_600_000.0).abs() < 1.0); // 8M / 5
        assert!((cp - 10.0).abs() < 0.01); // 50 / 5
    }

    #[test]
    fn project_week_extrapolates_to_full_pool() {
        // 50% used, 50M tokens, density 1M/%
        let w = project_week_to_full(50_000_000, 100.0, 50.0, 1_000_000.0, 2.0, false);
        assert_eq!(w.tokens, 100_000_000);
        assert!((w.cost_usd - 200.0).abs() < 0.01);
        assert!(!w.low_confidence);
    }

    #[test]
    fn smart_month_uses_completed_not_4x() {
        // Finished weeks $100, $80, $80 + current proj $80, no remaining full weeks
        let current = WeekProjection {
            tokens: 10,
            cost_usd: 80.0,
            low_confidence: false,
        };
        let (tok, cost) = project_month(&[(1, 100.0), (1, 80.0), (1, 80.0)], &current, 0);
        assert_eq!(tok, 13);
        assert!((cost - 340.0).abs() < 0.01);

        // One remaining full week: median of [100,80,80,80] = 80
        let (_, cost2) = project_month(&[(1, 100.0), (1, 80.0), (1, 80.0)], &current, 1);
        assert!((cost2 - 420.0).abs() < 0.01); // 340 + 80
    }

    #[test]
    fn oneshot_requires_min_pct() {
        assert!(density_oneshot(1_000_000, 10.0, 2.0).is_none());
        let (t, c) = density_oneshot(10_000_000, 50.0, 10.0).unwrap();
        assert!((t - 1_000_000.0).abs() < 1.0);
        assert!((c - 5.0).abs() < 0.01);
    }
}
