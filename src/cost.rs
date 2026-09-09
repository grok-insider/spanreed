//! Cost presentation backed by the shared local consumption store.

use crate::pricing::{self, Usage};
use crate::usage_stats::{self, CacheTotals, ModelCost};
use crate::util;
use serde::{Deserialize, Serialize};

/// Rolling window: today plus the previous 30 days.
const WINDOW_DAYS: i64 = 31;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Copy)]
pub enum Source {
    Claude,
    Codex,
}

impl Source {
    fn id(self) -> &'static str {
        match self {
            Source::Claude => "claude",
            Source::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayCost {
    pub date: String,
    pub cost: f64,
    pub tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSummary {
    pub total_cost: f64,
    pub total_tokens: u64,
    /// True when at least one token-bearing entry had no resolvable price, so
    /// `total_cost` is a lower bound.
    pub partial: bool,
    /// Per-day totals, ascending by date.
    pub daily: Vec<DayCost>,
    /// Per-model totals, tokens descending.
    #[serde(default)]
    pub by_model: Vec<ModelCost>,
    /// Prompt/output/cache token totals (for Cache line).
    #[serde(default)]
    pub cache: CacheTotals,
}

/// A single priced usage record extracted from a log line.
struct Entry {
    ts_ms: i64,
    model: Option<String>,
    usage: Usage,
    /// Cost already recorded in the log (Claude "costUSD"); preferred when set.
    cost_usd: Option<f64>,
    /// Dedup key hash (Claude message+request id); None disables dedup.
    dedup: Option<u64>,
}

/// Build the cost display lines for a provider from its local logs:
/// `Last 30 Days`, optional since-weekly-reset, model/cache breakdown, sparkline.
///
/// `weekly_start_ms` is the current rate-limit epoch start (API-derived). When
/// set, only log entries with `ts >= weekly_start_ms` count toward
/// "Since weekly reset" (force-resets move this forward).
/// Returns an empty vec when there is no local usage data.
pub fn cost_lines(source: Source, weekly_start_ms: Option<i64>) -> Vec<crate::model::MetricLine> {
    use crate::model::{BarChartPoint, MetricKind, MetricLine};

    let summary = match estimate(source) {
        Some(s) if s.total_tokens > 0 => s,
        _ => return Vec::new(),
    };

    let mut lines = Vec::new();
    let tokens = util::fmt_tokens(summary.total_tokens);
    // Local-log list-price estimate (not the subscription invoice).
    let value = if summary.total_cost > 0.0 {
        let suffix = if summary.partial {
            " (partial, estimated)"
        } else {
            " (estimated)"
        };
        format!("~${:.2} · {} tokens{}", summary.total_cost, tokens, suffix)
    } else {
        format!("{tokens} tokens")
    };
    lines.push(MetricLine::text(MetricKind::Cost, "Last 30 Days", value));

    if let Some(start) = weekly_start_ms {
        if let Some(win) = estimate_since(source, start) {
            if let Some(l) =
                usage_stats::since_weekly_reset_line(win.total_tokens, win.total_cost, win.partial)
            {
                lines.push(l);
            }
        }
    }

    lines.extend(usage_stats::breakdown_lines(
        &summary.by_model,
        summary.cache,
    ));

    if summary.daily.len() >= 2 {
        let points = summary
            .daily
            .iter()
            .map(|d| BarChartPoint {
                label: d.date.clone(),
                value: d.cost,
                value_label: Some(format!("${:.2}", d.cost)),
            })
            .collect();
        lines.push(MetricLine::bar_chart("Usage Trend", points, None));
    }

    lines
}

/// Estimate the rolling window after importing changed local sources.
pub fn estimate(source: Source) -> Option<CostSummary> {
    compute_from(source, util::now_ms() - WINDOW_DAYS * DAY_MS)
}

pub fn estimate_since(source: Source, cutoff_ms: i64) -> Option<CostSummary> {
    compute_from(source, cutoff_ms)
}

fn compute_from(source: Source, cutoff: i64) -> Option<CostSummary> {
    pricing::ensure_fresh();
    let statuses = crate::usage::refresh(Some(source.id()), false).ok()?;
    let records = crate::usage::records(&fabrials_core::usage::UsageFilter {
        client: Some(source.id().into()),
        since_ms: Some(cutoff),
        ..Default::default()
    })
    .ok()?;
    let entries = records
        .into_iter()
        .filter_map(|record| {
            let tokens = record.tokens?;
            Some(Entry {
                ts_ms: record.at_ms,
                model: record.model,
                usage: Usage {
                    input: tokens
                        .input
                        .saturating_sub(tokens.cache_read)
                        .saturating_sub(tokens.cache_write),
                    output: tokens.output,
                    cache_read: tokens.cache_read,
                    cache_create: tokens.cache_write,
                },
                cost_usd: record.cost.map(|c| c.usd),
                dedup: None,
            })
        })
        .collect();
    let mut summary = aggregate(entries)?;
    summary.partial |= statuses.iter().any(|s| {
        matches!(
            s.state,
            fabrials_core::usage::ImportState::Error | fabrials_core::usage::ImportState::Partial
        )
    });
    Some(summary)
}

fn aggregate(mut entries: Vec<Entry>) -> Option<CostSummary> {
    // Dedup: keep the first occurrence of each (message,request) key.
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| match e.dedup {
        Some(key) => seen.insert(key),
        None => true,
    });

    let mut by_day: std::collections::BTreeMap<String, (f64, u64)> =
        std::collections::BTreeMap::new();
    let mut by_model: std::collections::HashMap<String, ModelCost> =
        std::collections::HashMap::new();
    let mut total_cost = 0.0;
    let mut total_tokens = 0u64;
    let mut partial = false;
    let mut cache = CacheTotals::default();

    for e in &entries {
        let tokens = e.usage.total();
        if tokens == 0 {
            continue;
        }
        let cost = match e.cost_usd {
            Some(c) => c,
            None => {
                partial = true;
                0.0
            }
        };
        let date = util::local_date_ymd(e.ts_ms);
        let slot = by_day.entry(date).or_insert((0.0, 0));
        slot.0 += cost;
        slot.1 += tokens;
        total_cost += cost;
        total_tokens += tokens;

        cache.input = cache.input.saturating_add(e.usage.input);
        cache.output = cache.output.saturating_add(e.usage.output);
        cache.cache_read = cache.cache_read.saturating_add(e.usage.cache_read);
        cache.cache_create = cache.cache_create.saturating_add(e.usage.cache_create);

        let model_name = e
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown")
            .to_string();
        let row = by_model.entry(model_name.clone()).or_insert(ModelCost {
            model: model_name,
            tokens: 0,
            cost: 0.0,
            input: 0,
            output: 0,
            cache_read: 0,
            cache_create: 0,
        });
        row.tokens = row.tokens.saturating_add(tokens);
        row.cost += cost;
        row.input = row.input.saturating_add(e.usage.input);
        row.output = row.output.saturating_add(e.usage.output);
        row.cache_read = row.cache_read.saturating_add(e.usage.cache_read);
        row.cache_create = row.cache_create.saturating_add(e.usage.cache_create);
    }

    let daily = by_day
        .into_iter()
        .map(|(date, (cost, tokens))| DayCost { date, cost, tokens })
        .collect();

    let mut by_model: Vec<ModelCost> = by_model.into_values().collect();
    usage_stats::sort_models_by_tokens(&mut by_model);

    Some(CostSummary {
        total_cost,
        total_tokens,
        partial,
        daily,
        by_model,
        cache,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_dedups_and_sums() {
        let mk = |cost: f64, tokens: u64, dedup: Option<u64>| Entry {
            ts_ms: util::now_ms(),
            model: None,
            usage: Usage {
                input: tokens,
                ..Default::default()
            },
            cost_usd: Some(cost),
            dedup,
        };
        let entries = vec![
            mk(1.0, 100, Some(1)),
            mk(1.0, 100, Some(1)), // duplicate, dropped
            mk(2.0, 200, Some(2)),
        ];
        let s = aggregate(entries).unwrap();
        assert!((s.total_cost - 3.0).abs() < 1e-9);
        assert_eq!(s.total_tokens, 300);
        assert!(!s.partial);
        assert_eq!(s.by_model.len(), 1);
        assert_eq!(s.by_model[0].model, "unknown");
        assert_eq!(s.cache.input, 300);
    }

    #[test]
    fn aggregate_flags_partial_when_unpriced() {
        let e = Entry {
            ts_ms: util::now_ms(),
            model: Some("totally-unknown-model".into()),
            usage: Usage {
                input: 1000,
                ..Default::default()
            },
            cost_usd: None,
            dedup: None,
        };
        let s = aggregate(vec![e]).unwrap();
        assert!(s.partial);
        assert_eq!(s.total_tokens, 1000);
        assert_eq!(s.total_cost, 0.0);
    }

    #[test]
    fn aggregate_by_model_and_cache() {
        let entries = vec![
            Entry {
                ts_ms: util::now_ms(),
                model: Some("claude-opus-4".into()),
                usage: Usage {
                    input: 100,
                    output: 50,
                    cache_create: 10,
                    cache_read: 200,
                },
                cost_usd: Some(1.5),
                dedup: None,
            },
            Entry {
                ts_ms: util::now_ms(),
                model: Some("claude-sonnet-4".into()),
                usage: Usage {
                    input: 80,
                    output: 20,
                    cache_create: 0,
                    cache_read: 40,
                },
                cost_usd: Some(0.5),
                dedup: None,
            },
            Entry {
                ts_ms: util::now_ms(),
                model: Some("claude-opus-4".into()),
                usage: Usage {
                    input: 10,
                    output: 5,
                    cache_create: 0,
                    cache_read: 0,
                },
                cost_usd: Some(0.1),
                dedup: None,
            },
        ];
        let s = aggregate(entries).unwrap();
        assert_eq!(s.by_model.len(), 2);
        assert_eq!(s.by_model[0].model, "claude-opus-4");
        assert_eq!(s.by_model[0].tokens, 100 + 50 + 10 + 200 + 10 + 5);
        assert_eq!(s.by_model[1].model, "claude-sonnet-4");
        assert_eq!(s.cache.input, 190);
        assert_eq!(s.cache.cache_read, 240);
        assert_eq!(s.cache.cache_create, 10);
        let hit = s.cache.cache_hit_pct().unwrap();
        assert!((hit - (240.0 / 430.0 * 100.0)).abs() < 1e-9);

        let lines = usage_stats::breakdown_lines(&s.by_model, s.cache);
        assert_eq!(lines.len(), 2);
        match &lines[0] {
            crate::model::MetricLine::Text { label, value, .. } => {
                assert_eq!(label, "Models");
                assert!(value.contains("claude-opus-4"));
                assert!(value.contains("claude-sonnet-4"));
            }
            _ => panic!("models line"),
        }
        match &lines[1] {
            crate::model::MetricLine::Text { label, value, .. } => {
                assert_eq!(label, "Cache");
                assert!(value.contains("create"), "value={value}");
                assert!(
                    value.contains(')'),
                    "create should be inside parens: {value}"
                );
            }
            _ => panic!("cache line"),
        }
    }
}
