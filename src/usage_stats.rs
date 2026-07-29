//! Shared formatting for local usage breakdowns (models + prompt cache).
//!
//! Used by Claude/Codex log costing (`cost`) and the Grok capture ledger
//! (`grok_ledger`) so probe lines stay consistent.

use serde::{Deserialize, Serialize};

use crate::model::MetricLine;
use crate::util;

/// Max model names shown on the Models line before `· (+N)`.
pub const MODEL_TOP_N: usize = 3;

/// Soft cap for displayed model id length.
const MODEL_NAME_MAX: usize = 28;

/// Known product window length when Claude's usage JSON omits an explicit duration.
pub const CLAUDE_WEEKLY_SECS: i64 = 7 * 24 * 60 * 60;

/// One rate-limit / credits epoch reported by a provider API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateWindow {
    /// Display label for the pool (e.g. "Weekly").
    pub label: &'static str,
    pub used_percent: f64,
    /// Epoch end (when this window resets), unix ms.
    pub resets_at_ms: i64,
    /// Epoch start (when the current window began), unix ms.
    pub window_start_ms: i64,
    /// Window length in seconds when known (Codex `limit_window_seconds`).
    pub limit_window_secs: Option<i64>,
}

impl RateWindow {
    /// Codex-style epoch: `window_start = effective_reset - limit_window_seconds`.
    ///
    /// `effective_reset` caps `api_reset_at` at `now + limit_window_seconds` so a
    /// force-reset (used≈0, reset_at ≈ now+W) yields `window_start ≈ now`.
    pub fn from_codex_fields(
        label: &'static str,
        used_percent: f64,
        api_reset_at_secs: Option<i64>,
        limit_window_secs: Option<i64>,
        now_sec: i64,
    ) -> Option<Self> {
        let limit = limit_window_secs.filter(|s| *s > 0)?;
        let window_end = now_sec + limit;
        let effective_reset = match api_reset_at_secs {
            Some(a) => a.min(window_end),
            None => window_end,
        };
        let window_start_ms = (effective_reset - limit).saturating_mul(1000);
        let resets_at_ms = effective_reset.saturating_mul(1000);
        Some(Self {
            label,
            used_percent,
            resets_at_ms,
            window_start_ms,
            limit_window_secs: Some(limit),
        })
    }

    /// Claude-style: only `resets_at` is known; start = resets_at − known duration.
    pub fn from_resets_at_iso(
        label: &'static str,
        used_percent: f64,
        resets_at_iso: &str,
        limit_window_secs: i64,
    ) -> Option<Self> {
        if limit_window_secs <= 0 {
            return None;
        }
        let resets_at_ms = iso_to_ms(resets_at_iso)?;
        let window_start_ms = resets_at_ms.saturating_sub(limit_window_secs.saturating_mul(1000));
        Some(Self {
            label,
            used_percent,
            resets_at_ms,
            window_start_ms,
            limit_window_secs: Some(limit_window_secs),
        })
    }

    /// Grok credits period with explicit start/end ISO timestamps.
    pub fn from_period_bounds(
        label: &'static str,
        used_percent: f64,
        start_iso: &str,
        end_iso: &str,
    ) -> Option<Self> {
        let window_start_ms = iso_to_ms(start_iso)?;
        let resets_at_ms = iso_to_ms(end_iso)?;
        if resets_at_ms < window_start_ms {
            return None;
        }
        let limit_window_secs = Some((resets_at_ms - window_start_ms) / 1000);
        Some(Self {
            label,
            used_percent,
            resets_at_ms,
            window_start_ms,
            limit_window_secs,
        })
    }
}

fn iso_to_ms(iso: &str) -> Option<i64> {
    util::parse_iso_dt(iso.trim()).map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
}

/// Metric line for local tokens/cost observed since the current weekly epoch.
pub fn since_weekly_reset_line(tokens: u64, cost: f64, partial: bool) -> Option<MetricLine> {
    if tokens == 0 {
        return None;
    }
    let tok = util::fmt_tokens(tokens);
    let value = if cost > 0.0 {
        let suffix = if partial { " (partial)" } else { "" };
        format!("{tok} tokens · ~${cost:.2}{suffix}")
    } else {
        format!("{tok} tokens")
    };
    Some(MetricLine::text("Since weekly reset", value))
}

/// Per-model totals over a rolling window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelCost {
    pub model: String,
    pub tokens: u64,
    pub cost: f64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_create: u64,
}

/// Cache + prompt token totals for a window.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct CacheTotals {
    /// Uncached input tokens (provider-dependent split; see `cache_hit_pct`).
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_create: u64,
}

impl CacheTotals {
    /// Prompt-side total used for the cache hit ratio denominator:
    /// `input (uncached) + cache_read` (cache_create is a write, not a hit).
    pub fn prompt_for_hit_ratio(self) -> u64 {
        self.input.saturating_add(self.cache_read)
    }

    /// `cache_read / (input + cache_read) * 100`, or None when no prompt tokens.
    pub fn cache_hit_pct(self) -> Option<f64> {
        let denom = self.prompt_for_hit_ratio();
        if denom == 0 {
            return None;
        }
        Some((self.cache_read as f64 / denom as f64) * 100.0)
    }

    pub fn has_cache_signal(self) -> bool {
        self.cache_read > 0 || self.cache_create > 0
    }
}

/// Build `Models` and `Cache` metric lines from aggregates.
pub fn breakdown_lines(by_model: &[ModelCost], cache: CacheTotals) -> Vec<MetricLine> {
    let mut lines = Vec::new();
    if let Some(l) = models_line(by_model) {
        lines.push(l);
    }
    if let Some(l) = cache_line(cache) {
        lines.push(l);
    }
    lines
}

fn models_line(by_model: &[ModelCost]) -> Option<MetricLine> {
    let non_empty: Vec<&ModelCost> = by_model.iter().filter(|m| m.tokens > 0).collect();
    if non_empty.is_empty() {
        return None;
    }
    let top = non_empty.len().min(MODEL_TOP_N);
    let mut parts: Vec<String> = non_empty[..top]
        .iter()
        .map(|m| {
            format!(
                "{} {}",
                short_model_name(&m.model),
                util::fmt_tokens(m.tokens)
            )
        })
        .collect();
    let extra = non_empty.len().saturating_sub(top);
    if extra > 0 {
        parts.push(format!("(+{extra})"));
    }
    Some(MetricLine::text("Models", parts.join(" · ")))
}

fn cache_line(cache: CacheTotals) -> Option<MetricLine> {
    if !cache.has_cache_signal() {
        return None;
    }
    let pct = cache.cache_hit_pct()?;
    let value = if cache.cache_create > 0 {
        format!(
            "{:.0}% of input (read {} · create {})",
            pct,
            util::fmt_tokens(cache.cache_read),
            util::fmt_tokens(cache.cache_create)
        )
    } else {
        format!(
            "{:.0}% of input (read {})",
            pct,
            util::fmt_tokens(cache.cache_read)
        )
    };
    Some(MetricLine::text("Cache", value))
}

/// Shorten long model ids for the terminal while keeping discriminants.
pub fn short_model_name(model: &str) -> String {
    let m = model.trim();
    if m.is_empty() {
        return "unknown".into();
    }
    // Drop common provider routing prefixes.
    let m = m
        .strip_prefix("anthropic/")
        .or_else(|| m.strip_prefix("openai/"))
        .or_else(|| m.strip_prefix("xai/"))
        .unwrap_or(m);
    // Prefer dropping a trailing date-like segment (-20250301).
    let m = strip_trailing_date_suffix(m).unwrap_or(m);
    if m.len() <= MODEL_NAME_MAX {
        return m.to_string();
    }
    truncate_ellipsis(m, MODEL_NAME_MAX)
}

fn strip_trailing_date_suffix(m: &str) -> Option<&str> {
    // ...-YYYYMMDD at the end only.
    let idx = m.rfind('-')?;
    let seg = &m[idx + 1..];
    if seg.len() == 8 && seg.chars().all(|c| c.is_ascii_digit()) {
        Some(&m[..idx])
    } else {
        None
    }
}

fn truncate_ellipsis(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    if max <= 1 {
        return s.chars().take(max).collect();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// Sort model rows by tokens descending (stable for ties by name).
pub fn sort_models_by_tokens(models: &mut [ModelCost]) {
    models.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.model.cmp(&b.model)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_hit_pct_is_read_over_uncached_plus_read() {
        let c = CacheTotals {
            input: 400,
            output: 50,
            cache_read: 600,
            cache_create: 10,
        };
        // 600 / (400+600) = 60%
        assert!((c.cache_hit_pct().unwrap() - 60.0).abs() < 1e-9);
    }

    #[test]
    fn cache_line_omits_create_when_zero() {
        let lines = breakdown_lines(
            &[],
            CacheTotals {
                input: 100,
                output: 0,
                cache_read: 100,
                cache_create: 0,
            },
        );
        assert_eq!(lines.len(), 1);
        match &lines[0] {
            MetricLine::Text { label, value, .. } => {
                assert_eq!(label, "Cache");
                assert!(value.contains("% of input"));
                assert!(value.contains("read"));
                assert!(!value.contains("create"));
            }
            _ => panic!("expected text line"),
        }
    }

    #[test]
    fn models_line_top_n_and_extra() {
        let models = vec![
            ModelCost {
                model: "a".into(),
                tokens: 3000,
                cost: 0.0,
                input: 0,
                output: 0,
                cache_read: 0,
                cache_create: 0,
            },
            ModelCost {
                model: "b".into(),
                tokens: 2000,
                cost: 0.0,
                input: 0,
                output: 0,
                cache_read: 0,
                cache_create: 0,
            },
            ModelCost {
                model: "c".into(),
                tokens: 1000,
                cost: 0.0,
                input: 0,
                output: 0,
                cache_read: 0,
                cache_create: 0,
            },
            ModelCost {
                model: "d".into(),
                tokens: 500,
                cost: 0.0,
                input: 0,
                output: 0,
                cache_read: 0,
                cache_create: 0,
            },
        ];
        let lines = breakdown_lines(&models, CacheTotals::default());
        assert_eq!(lines.len(), 1);
        match &lines[0] {
            MetricLine::Text { label, value, .. } => {
                assert_eq!(label, "Models");
                assert!(value.contains("a "));
                assert!(value.contains("b "));
                assert!(value.contains("c "));
                assert!(value.contains("(+1)"));
                assert!(!value.contains(" d "));
            }
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn short_model_strips_date_suffix() {
        let long = "claude-opus-4-20250514";
        assert_eq!(short_model_name(long), "claude-opus-4");
    }

    #[test]
    fn no_models_or_cache_yields_empty() {
        assert!(breakdown_lines(&[], CacheTotals::default()).is_empty());
    }

    #[test]
    fn codex_window_start_is_reset_minus_limit() {
        let now = 1_700_000_000_i64;
        let limit = 604_800_i64; // 7d
        let reset = now + limit; // force-reset: next reset in 7d
        let w = RateWindow::from_codex_fields("Weekly", 0.0, Some(reset), Some(limit), now)
            .expect("window");
        assert_eq!(w.window_start_ms, now * 1000);
        assert_eq!(w.resets_at_ms, reset * 1000);
        assert_eq!(w.limit_window_secs, Some(limit));
    }

    #[test]
    fn codex_force_reset_moves_epoch_start() {
        let now = 1_700_000_000_i64;
        let limit = 604_800_i64;
        let old_reset = now + 2 * 24 * 3600; // mid-cycle, 2d left
        let old = RateWindow::from_codex_fields("Weekly", 80.0, Some(old_reset), Some(limit), now)
            .unwrap();
        // Force-reset: used→0, reset jumps to now+7d
        let new_reset = now + limit;
        let new = RateWindow::from_codex_fields("Weekly", 0.0, Some(new_reset), Some(limit), now)
            .unwrap();
        assert!(new.window_start_ms > old.window_start_ms);
        assert_eq!(new.window_start_ms, now * 1000);
        // Pre-force tokens would be before new.window_start_ms
        let pre_force_ts = old.window_start_ms + 1000;
        assert!(pre_force_ts < new.window_start_ms);
    }

    #[test]
    fn codex_caps_outrange_reset_at() {
        let now = 1_700_000_000_i64;
        let limit = 18_000_i64; // 5h session
        let far = now + 30 * 24 * 3600; // API returned monthly-ish stamp
        let w = RateWindow::from_codex_fields("Session", 1.0, Some(far), Some(limit), now).unwrap();
        assert_eq!(w.resets_at_ms, (now + limit) * 1000);
        assert_eq!(w.window_start_ms, now * 1000);
    }

    #[test]
    fn claude_weekly_start_from_resets_at() {
        let end = "2026-08-06T12:00:00Z";
        let w = RateWindow::from_resets_at_iso("Weekly", 49.0, end, CLAUDE_WEEKLY_SECS).unwrap();
        let end_ms = iso_to_ms(end).unwrap();
        assert_eq!(w.resets_at_ms, end_ms);
        assert_eq!(w.window_start_ms, end_ms - CLAUDE_WEEKLY_SECS * 1000);
    }

    #[test]
    fn since_weekly_line_formats() {
        let l = since_weekly_reset_line(1_500_000, 12.5, false).unwrap();
        match l {
            MetricLine::Text { label, value, .. } => {
                assert_eq!(label, "Since weekly reset");
                assert!(value.contains("1.5M tokens"));
                assert!(value.contains("~$12.50"));
            }
            _ => panic!("text"),
        }
        assert!(since_weekly_reset_line(0, 1.0, false).is_none());
    }
}
