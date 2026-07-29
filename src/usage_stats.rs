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
}
