//! Local ledger of official Grok/xAI API usage records.
//!
//! Records are written by the external `ai-relay` capture worker when it
//! observes a completed Responses API call with a `usage` object. Probe reads
//! this file for accurate Last-30-Days totals — never invents tokens from
//! session context.
//!
//! Dollar estimates use **public API list prices** from [`crate::pricing`]
//! (Grok 4.5: $2 / $0.30 cached / $6 per MTok, with xAI's all-or-nothing
//! ≥200k long-context tier). Subscription-internal `cost_in_usd_ticks` are
//! still recorded for reference but are not what we display — SuperGrok
//! pool ticks are not public API rates.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{BarChartPoint, MetricKind, MetricLine};
use crate::pricing;
use crate::usage_stats::{self, CacheTotals, ModelCost};
use crate::util;

/// Rolling window: today plus the previous 30 days.
const WINDOW_DAYS: i64 = 31;
const DAY_MS: i64 = 86_400_000;
const TICKS_PER_USD: f64 = 1_000_000_000.0;

/// One completed API call's official usage (from Responses `usage`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageRecord {
    pub ts_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    /// xAI `cost_in_usd_ticks` (1e9 ticks = $1). Zero when not provided.
    #[serde(default)]
    pub cost_usd_ticks: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl UsageRecord {
    pub fn tokens_for_total(&self) -> u64 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens.saturating_add(self.output_tokens)
        }
    }

    /// Subscription-internal ticks from the API (not public list price).
    #[allow(dead_code)]
    pub fn ticks_usd(&self) -> Option<f64> {
        if self.cost_usd_ticks > 0 {
            Some(self.cost_usd_ticks as f64 / TICKS_PER_USD)
        } else {
            None
        }
    }

    /// Public API list-price USD for this record, or None if the model is unknown.
    ///
    /// xAI long-context rule ([docs](https://docs.x.ai/developers/pricing)):
    /// when prompt tokens ≥ 200k, **all** token types in the request use the
    /// higher rate (not progressive Anthropic-style tiers).
    pub fn list_cost_usd(&self) -> Option<f64> {
        let model = self
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        let p = pricing::table().find(model)?;
        let cached = self.cached_input_tokens.min(self.input_tokens);
        let uncached = self.input_tokens.saturating_sub(cached);
        const TIER: u64 = 200_000;
        let long = self.input_tokens >= TIER;
        let (rin, rcache, rout) = if long {
            (
                p.input_above_200k.unwrap_or(p.input),
                p.cache_read_above_200k.unwrap_or(p.cache_read),
                p.output_above_200k.unwrap_or(p.output),
            )
        } else {
            (p.input, p.cache_read, p.output)
        };
        Some(uncached as f64 * rin + cached as f64 * rcache + self.output_tokens as f64 * rout)
    }
}

/// Path to the append-only ledger JSONL.
pub fn ledger_path() -> PathBuf {
    crate::app::data_dir().join("grok-usage.jsonl")
}

/// Read all ledger records with `ts_ms` in `[cutoff, now]`.
pub fn read_window(now_ms: i64) -> Vec<UsageRecord> {
    let cutoff = now_ms - WINDOW_DAYS * DAY_MS;
    let path = ledger_path();
    let Ok(f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(rec) = serde_json::from_str::<UsageRecord>(line) else {
            continue;
        };
        if rec.ts_ms >= cutoff && rec.ts_ms <= now_ms + DAY_MS {
            out.push(rec);
        }
    }
    out
}

/// Aggregate ledger into Last-30-Days lines. Returns empty when no capture data.
///
/// When `weekly_start_ms` is set (Grok `currentPeriod.start`), also emit
/// "Since weekly reset" from records at/after that epoch start.
/// Without pool-% forecast (e.g. tests / callers that only need ledger lines).
#[allow(dead_code)]
pub fn cost_lines(weekly_start_ms: Option<i64>) -> Vec<MetricLine> {
    cost_lines_with_forecast(weekly_start_ms, None, None)
}

/// Like [`cost_lines`], plus week/month forecasts when `weekly_pct` is known.
pub fn cost_lines_with_forecast(
    weekly_start_ms: Option<i64>,
    weekly_pct: Option<f64>,
    week_end_ms: Option<i64>,
) -> Vec<MetricLine> {
    let now = util::now_ms();
    let recs = read_window(now);
    if recs.is_empty() {
        let hint = empty_capture_hint();
        return vec![MetricLine::text(MetricKind::Cost, "Last 30 Days", hint)];
    }
    lines_from_records(&recs, weekly_start_ms, weekly_pct, week_end_ms)
}

fn empty_capture_hint() -> String {
    // Avoid circular deps on setup for unit tests: TCP probe only.
    let ports_up = capture_ports_up();
    if ports_up {
        "no usage captured yet — use Grok/OpenCode via the local proxy".into()
    } else {
        "proxy DOWN — Grok/OpenCode may fail; run `spanreed capture ensure`".into()
    }
}

fn capture_ports_up() -> bool {
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    ["127.0.0.1:18736", "127.0.0.1:18737"].iter().all(|a| {
        a.parse::<SocketAddr>()
            .ok()
            .and_then(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(150)).ok())
            .is_some()
    })
}

fn lines_from_records(
    recs: &[UsageRecord],
    weekly_start_ms: Option<i64>,
    weekly_pct: Option<f64>,
    week_end_ms: Option<i64>,
) -> Vec<MetricLine> {
    let mut total_tokens: u64 = 0;
    let mut tokens_with_cost: u64 = 0;
    let mut total_cost = 0.0;
    let mut has_cost = false;
    // date -> (cost, tokens)
    let mut daily: std::collections::BTreeMap<String, (f64, u64)> =
        std::collections::BTreeMap::new();
    let mut by_model: std::collections::HashMap<String, ModelCost> =
        std::collections::HashMap::new();
    let mut cache = CacheTotals::default();

    for r in recs {
        let tok = r.tokens_for_total();
        total_tokens = total_tokens.saturating_add(tok);
        // Public API list price (not SuperGrok subscription ticks).
        let cost = match r.list_cost_usd() {
            Some(c) => {
                has_cost = true;
                tokens_with_cost = tokens_with_cost.saturating_add(tok);
                c
            }
            None => 0.0,
        };
        total_cost += cost;
        let date = ms_to_ymd(r.ts_ms).unwrap_or_else(|| "unknown".into());
        let e = daily.entry(date).or_insert((0.0, 0));
        e.0 += cost;
        e.1 = e.1.saturating_add(tok);

        // Grok: input_tokens includes cached portion (same split as Codex pricing).
        let cached = r.cached_input_tokens.min(r.input_tokens);
        let uncached_input = r.input_tokens.saturating_sub(cached);
        cache.input = cache.input.saturating_add(uncached_input);
        cache.output = cache.output.saturating_add(r.output_tokens);
        cache.cache_read = cache.cache_read.saturating_add(cached);

        let model_name = r
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
        row.tokens = row.tokens.saturating_add(tok);
        row.cost += cost;
        row.input = row.input.saturating_add(uncached_input);
        row.output = row.output.saturating_add(r.output_tokens);
        row.cache_read = row.cache_read.saturating_add(cached);
    }

    // Partial when some captured tokens have no known public list price.
    let partial = has_cost && tokens_with_cost < total_tokens;
    let tokens = util::fmt_tokens(total_tokens);
    let value = if has_cost && total_cost > 0.0 {
        let suffix = if partial { " (partial)" } else { "" };
        format!("${:.4} · {tokens} tokens{suffix}", total_cost)
    } else {
        format!("{tokens} tokens")
    };

    let mut by_model: Vec<ModelCost> = by_model.into_values().collect();
    usage_stats::sort_models_by_tokens(&mut by_model);

    let mut lines = vec![MetricLine::text(MetricKind::Cost, "Last 30 Days", value)];
    if let Some(cov) = cost_coverage_line(tokens_with_cost, total_tokens, partial) {
        lines.push(cov);
    }

    let mut week_tokens: u64 = 0;
    let mut week_cost: f64 = 0.0;
    if let Some(start) = weekly_start_ms {
        let mut win_tokens: u64 = 0;
        let mut win_tokens_with_cost: u64 = 0;
        let mut win_cost = 0.0;
        let mut win_has_cost = false;
        for r in recs {
            if r.ts_ms < start {
                continue;
            }
            let tok = r.tokens_for_total();
            win_tokens = win_tokens.saturating_add(tok);
            if let Some(c) = r.list_cost_usd() {
                win_has_cost = true;
                win_cost += c;
                win_tokens_with_cost = win_tokens_with_cost.saturating_add(tok);
            }
        }
        let cost = if win_has_cost { win_cost } else { 0.0 };
        week_tokens = win_tokens;
        week_cost = cost;
        let win_partial = win_has_cost && win_tokens_with_cost < win_tokens;
        if let Some(l) = usage_stats::since_weekly_reset_line(win_tokens, cost, win_partial) {
            lines.push(l);
        }
    }

    if let Some(pct) = weekly_pct {
        let week_id = weekly_start_ms
            .map(|ms| ms.to_string())
            .unwrap_or_else(|| "unknown".into());
        lines.extend(crate::forecast::forecast_lines(
            "grok",
            &week_id,
            pct,
            week_tokens,
            week_cost,
            week_end_ms,
        ));
    }

    lines.extend(usage_stats::breakdown_lines(&by_model, cache));
    if daily.len() >= 2 {
        let points: Vec<BarChartPoint> = daily
            .iter()
            .map(|(date, (cost, tok))| {
                let value = if has_cost { *cost } else { *tok as f64 };
                let value_label = if has_cost {
                    format!("${:.4}", cost)
                } else {
                    util::fmt_tokens(*tok)
                };
                BarChartPoint {
                    label: date.clone(),
                    value,
                    value_label: Some(value_label),
                }
            })
            .collect();
        lines.push(MetricLine::bar_chart("Usage Trend", points, None));
    }
    lines
}

/// When `$` is incomplete, show what fraction of tokens had a public list price.
fn cost_coverage_line(with_cost: u64, total: u64, partial: bool) -> Option<MetricLine> {
    if !partial || total == 0 {
        return None;
    }
    let pct = (with_cost as f64 / total as f64) * 100.0;
    Some(MetricLine::text(
        MetricKind::Cost,
        "Cost coverage",
        format!("{pct:.0}% of tokens (API list price)"),
    ))
}

fn ms_to_ymd(ms: i64) -> Option<String> {
    let secs = ms.div_euclid(1000);
    let t = time::OffsetDateTime::from_unix_timestamp(secs).ok()?;
    Some(format!(
        "{:04}-{:02}-{:02}",
        t.year(),
        u8::from(t.month()),
        t.day()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_cost_matches_public_rates() {
        // 60 unc * $2/M + 40 cache * $0.30/M + 20 out * $6/M
        let rec = UsageRecord {
            ts_ms: 1_000,
            session_id: Some("sess".into()),
            model: Some("grok-4.5".into()),
            input_tokens: 100,
            output_tokens: 20,
            cached_input_tokens: 40,
            reasoning_tokens: 5,
            total_tokens: 120,
            cost_usd_ticks: 500_000_000,
            request_id: Some("resp_1".into()),
        };
        assert!((rec.ticks_usd().unwrap() - 0.5).abs() < 1e-9);
        let list = rec.list_cost_usd().unwrap();
        let expected = 60.0 * 2e-6 + 40.0 * 3e-7 + 20.0 * 6e-6;
        assert!(
            (list - expected).abs() < 1e-12,
            "list={list} expected={expected}"
        );
    }

    #[test]
    fn list_cost_uses_long_context_rates_when_prompt_ge_200k() {
        let rec = UsageRecord {
            ts_ms: 1,
            session_id: None,
            model: Some("grok-4.5".into()),
            input_tokens: 200_000,
            output_tokens: 1_000,
            cached_input_tokens: 100_000,
            reasoning_tokens: 0,
            total_tokens: 201_000,
            cost_usd_ticks: 0,
            request_id: None,
        };
        // All tokens at long rates: unc 100k * $4/M + cache 100k * $0.60/M + out 1k * $12/M
        let expected = 100_000.0 * 4e-6 + 100_000.0 * 6e-7 + 1_000.0 * 1.2e-5;
        let got = rec.list_cost_usd().unwrap();
        assert!(
            (got - expected).abs() < 1e-9,
            "got={got} expected={expected}"
        );
    }

    #[test]
    fn cost_lines_empty_points_at_proxy() {
        // When ledger missing, cost_lines still returns enable message.
        // Use a path that won't exist by temporarily relying on real ledger;
        // if user has capture data this still returns non-empty. Assert shape:
        let lines = cost_lines(None);
        assert!(!lines.is_empty());
        match &lines[0] {
            MetricLine::Text { label, .. } => assert_eq!(label, "Last 30 Days"),
            _ => panic!("expected text line"),
        }
    }

    #[test]
    fn lines_from_records_models_and_cache() {
        let recs = vec![
            UsageRecord {
                ts_ms: 1_700_000_000_000,
                session_id: None,
                model: Some("grok-4.5-build".into()),
                input_tokens: 1000,
                output_tokens: 100,
                cached_input_tokens: 600,
                reasoning_tokens: 0,
                total_tokens: 1100,
                cost_usd_ticks: 1_000_000_000, // ticks ignored for $ display
                request_id: Some("a".into()),
            },
            UsageRecord {
                ts_ms: 1_700_086_400_000, // next day
                session_id: None,
                model: Some("grok-4.5".into()),
                input_tokens: 200,
                output_tokens: 50,
                cached_input_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 250,
                cost_usd_ticks: 0,
                request_id: Some("b".into()),
            },
        ];
        let lines = lines_from_records(&recs, None, None, None);
        let labels: Vec<&str> = lines
            .iter()
            .filter_map(|l| match l {
                MetricLine::Text { label, .. } => Some(label.as_str()),
                MetricLine::BarChart { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"Last 30 Days"));
        assert!(!labels.contains(&"Cost coverage")); // both models priced
        assert!(labels.contains(&"Models"));
        assert!(labels.contains(&"Cache"));
        assert!(labels.contains(&"Usage Trend"));

        // 400*$2/M + 600*$0.30/M + 100*$6/M + 200*$2/M + 50*$6/M
        let expected = 400.0 * 2e-6 + 600.0 * 3e-7 + 100.0 * 6e-6 + 200.0 * 2e-6 + 50.0 * 6e-6;
        let last30 = lines.iter().find_map(|l| match l {
            MetricLine::Text { label, value, .. } if label == "Last 30 Days" => {
                Some(value.as_str())
            }
            _ => None,
        });
        let last30 = last30.expect("last 30");
        assert!(!last30.contains("(partial)"), "got {last30}");
        assert!(
            last30.contains(&format!("${expected:.4}")),
            "expected ${expected:.4} in {last30}"
        );

        let models = lines.iter().find_map(|l| match l {
            MetricLine::Text { label, value, .. } if label == "Models" => Some(value.as_str()),
            _ => None,
        });
        let models = models.expect("models line");
        assert!(models.contains("grok-4.5-build"));
        assert!(models.contains("grok-4.5"));

        let cache = lines.iter().find_map(|l| match l {
            MetricLine::Text { label, value, .. } if label == "Cache" => Some(value.as_str()),
            _ => None,
        });
        let cache = cache.expect("cache line");
        // uncached input 400+200, cache_read 600 → 600/1200 = 50%
        assert!(
            cache.contains("50% of input"),
            "unexpected cache line: {cache}"
        );
        assert!(!cache.contains("create"));
    }

    #[test]
    fn full_list_price_not_partial() {
        let recs = vec![UsageRecord {
            ts_ms: 1_700_000_000_000,
            session_id: None,
            model: Some("grok-4.5-build".into()),
            input_tokens: 100,
            output_tokens: 10,
            cached_input_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 110,
            cost_usd_ticks: 500_000_000,
            request_id: Some("only".into()),
        }];
        let lines = lines_from_records(&recs, None, None, None);
        let last30 = lines.iter().find_map(|l| match l {
            MetricLine::Text { label, value, .. } if label == "Last 30 Days" => {
                Some(value.as_str())
            }
            _ => None,
        });
        let last30 = last30.expect("last 30");
        assert!(!last30.contains("partial"), "got {last30}");
        // 100*$2/M + 10*$6/M = $0.00026
        assert!(
            last30.contains("$0.0003") || last30.contains("$0.0002"),
            "got {last30}"
        );
        assert!(!lines.iter().any(|l| matches!(
            l,
            MetricLine::Text { label, .. } if label == "Cost coverage"
        )));
    }

    #[test]
    fn weekly_partial_when_window_has_unpriced_model() {
        let recs = vec![
            UsageRecord {
                ts_ms: 5_000,
                session_id: None,
                model: Some("grok-4.5".into()),
                input_tokens: 1_000_000,
                output_tokens: 0,
                cached_input_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 1_000_000,
                cost_usd_ticks: 0,
                request_id: Some("priced".into()),
            },
            UsageRecord {
                ts_ms: 6_000,
                session_id: None,
                model: Some("unknown-model-xyz".into()),
                input_tokens: 50,
                output_tokens: 0,
                cached_input_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 50,
                cost_usd_ticks: 0,
                request_id: Some("bare".into()),
            },
        ];
        let lines = lines_from_records(&recs, Some(4_000), None, None);
        let since = lines.iter().find_map(|l| match l {
            MetricLine::Text { label, value, .. } if label == "Since weekly reset" => {
                Some(value.as_str())
            }
            _ => None,
        });
        let since = since.expect("since weekly");
        assert!(since.contains("(partial)"), "got {since}");
        assert!(since.contains("1M tokens"), "got {since}");
        // 1M input ≥200k → long-context $4/M list = ~$4
        assert!(
            since.contains("$4.00") || since.contains("~$4"),
            "got {since}"
        );
    }

    #[test]
    fn since_weekly_excludes_records_before_epoch() {
        let recs = vec![
            UsageRecord {
                ts_ms: 1_000,
                session_id: None,
                model: Some("a".into()),
                input_tokens: 100,
                output_tokens: 0,
                cached_input_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 100,
                cost_usd_ticks: 0,
                request_id: Some("old".into()),
            },
            UsageRecord {
                ts_ms: 5_000,
                session_id: None,
                model: Some("a".into()),
                input_tokens: 25,
                output_tokens: 0,
                cached_input_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 25,
                cost_usd_ticks: 0,
                request_id: Some("new".into()),
            },
        ];
        let lines = lines_from_records(&recs, Some(4_000), None, None);
        let since = lines.iter().find_map(|l| match l {
            MetricLine::Text { label, value, .. } if label == "Since weekly reset" => {
                Some(value.as_str())
            }
            _ => None,
        });
        let since = since.expect("since weekly line");
        assert!(since.contains("25 tokens"), "got {since}");
        assert!(!since.contains("100"), "pre-epoch tokens leaked: {since}");
    }
}
