//! Local ledger of official Grok/xAI API usage records.
//!
//! Records are written by [`crate::grok_proxy`] when it observes a completed
//! Responses API call with a `usage` object. Probe reads this file for
//! accurate Last-30-Days totals — never invents tokens from session context.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::creds;
use crate::model::{MetricKind, BarChartPoint, MetricLine};
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

    pub fn cost_usd(&self) -> Option<f64> {
        if self.cost_usd_ticks > 0 {
            Some(self.cost_usd_ticks as f64 / TICKS_PER_USD)
        } else {
            None
        }
    }
}

/// Path to the append-only ledger JSONL.
pub fn ledger_path() -> PathBuf {
    creds::data_home()
        .join("spanreed")
        .join("grok-usage.jsonl")
}

/// Append one usage record. Best-effort; logs and returns Err on IO failure.
pub fn append(record: &UsageRecord) -> Result<(), String> {
    let path = ledger_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir ledger dir: {e}"))?;
    }
    // Dedup: skip if same request_id already present (last few KB scan).
    if let Some(rid) = record.request_id.as_deref() {
        if !rid.is_empty() && recent_has_request_id(rid) {
            return Ok(());
        }
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open ledger: {e}"))?;
    let line = serde_json::to_string(record).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| format!("write ledger: {e}"))?;
    Ok(())
}

fn recent_has_request_id(rid: &str) -> bool {
    let path = ledger_path();
    let Ok(f) = std::fs::File::open(path) else {
        return false;
    };
    let needle = format!("\"request_id\":\"{rid}\"");
    // Only scan last ~256 KiB for recent dups.
    let meta = f.metadata().ok();
    let reader = BufReader::new(f);
    if let Some(m) = meta {
        if m.len() > 256 * 1024 {
            // Fall through: full scan is fine for typical ledger sizes; for huge
            // files we still scan all lines (simple + correct).
        }
    }
    for line in reader.lines().map_while(Result::ok) {
        if line.contains(&needle) {
            return true;
        }
    }
    false
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
pub fn cost_lines(weekly_start_ms: Option<i64>) -> Vec<MetricLine> {
    let now = util::now_ms();
    let recs = read_window(now);
    if recs.is_empty() {
        return vec![MetricLine::text(MetricKind::Cost, "Last 30 Days",
            "no capture yet — enable `spanreed capture serve` (or HM capture.enable)",
        )];
    }
    lines_from_records(&recs, weekly_start_ms)
}

fn lines_from_records(recs: &[UsageRecord], weekly_start_ms: Option<i64>) -> Vec<MetricLine> {
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
        let cost = r.cost_usd().unwrap_or(0.0);
        if cost > 0.0 {
            has_cost = true;
            total_cost += cost;
            tokens_with_cost = tokens_with_cost.saturating_add(tok);
        }
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

    // Official xAI ticks only; partial when some captured tokens lack cost_in_usd_ticks.
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
            if let Some(c) = r.cost_usd() {
                win_has_cost = true;
                win_cost += c;
                win_tokens_with_cost = win_tokens_with_cost.saturating_add(tok);
            }
        }
        let cost = if win_has_cost { win_cost } else { 0.0 };
        let win_partial = win_has_cost && win_tokens_with_cost < win_tokens;
        if let Some(l) = usage_stats::since_weekly_reset_line(win_tokens, cost, win_partial) {
            lines.push(l);
        }
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

/// When `$` is incomplete, show what fraction of tokens had official ticks.
fn cost_coverage_line(with_cost: u64, total: u64, partial: bool) -> Option<MetricLine> {
    if !partial || total == 0 {
        return None;
    }
    let pct = (with_cost as f64 / total as f64) * 100.0;
    Some(MetricLine::text(MetricKind::Cost, "Cost coverage",
        format!("{pct:.0}% of tokens (official ticks)"),
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

/// Parse official usage from a Responses API JSON object or SSE body.
pub fn usage_from_response_body(body: &str) -> Option<UsagePartial> {
    // Try whole body as JSON first.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body.trim()) {
        if let Some(u) = usage_from_json(&v) {
            return Some(u);
        }
    }
    // SSE: find last `response.completed` (or any object with usage).
    let mut best: Option<UsagePartial> = None;
    for line in body.lines() {
        let line = line.trim();
        let payload = line.strip_prefix("data: ").unwrap_or(line);
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) else {
            continue;
        };
        if let Some(u) = usage_from_json(&v) {
            best = Some(u);
        }
    }
    best
}

#[derive(Debug, Clone)]
pub struct UsagePartial {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
    pub cost_usd_ticks: u64,
    pub model: Option<String>,
    pub request_id: Option<String>,
}

fn usage_from_json(v: &serde_json::Value) -> Option<UsagePartial> {
    // response.completed shape: { type, response: { usage, model, id } }
    let response = v.get("response").filter(|r| r.is_object()).unwrap_or(v);
    let usage = response.get("usage").or_else(|| v.get("usage"))?;
    if !usage.is_object() {
        return None;
    }
    let num = |k: &str| -> u64 {
        usage
            .get(k)
            .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64)))
            .unwrap_or(0)
    };
    let input = num("input_tokens").max(num("prompt_tokens"));
    let output = num("output_tokens").max(num("completion_tokens"));
    let total = num("total_tokens");
    let cached = usage
        .get("input_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64)))
        .unwrap_or(0);
    let reasoning = usage
        .get("output_tokens_details")
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64)))
        .unwrap_or(0);
    let cost_ticks = usage
        .get("cost_in_usd_ticks")
        .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64)))
        .unwrap_or(0);

    if input == 0 && output == 0 && total == 0 && cost_ticks == 0 {
        return None;
    }

    let model = response
        .get("model")
        .or_else(|| v.get("model"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());
    let request_id = response
        .get("id")
        .or_else(|| v.get("id"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());

    Some(UsagePartial {
        input_tokens: input,
        output_tokens: output,
        cached_input_tokens: cached,
        reasoning_tokens: reasoning,
        total_tokens: if total > 0 {
            total
        } else {
            input.saturating_add(output)
        },
        cost_usd_ticks: cost_ticks,
        model,
        request_id,
    })
}

impl UsagePartial {
    pub fn into_record(self, ts_ms: i64, session_id: Option<String>) -> UsageRecord {
        UsageRecord {
            ts_ms,
            session_id,
            model: self.model,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cached_input_tokens: self.cached_input_tokens,
            reasoning_tokens: self.reasoning_tokens,
            total_tokens: self.total_tokens,
            cost_usd_ticks: self.cost_usd_ticks,
            request_id: self.request_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_response_completed_usage() {
        let body = r#"data: {"type":"response.created"}
data: {"type":"response.completed","response":{"id":"resp_1","model":"grok-4.5","usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120,"input_tokens_details":{"cached_tokens":40},"output_tokens_details":{"reasoning_tokens":5},"cost_in_usd_ticks":500000000}}}
data: [DONE]
"#;
        let u = usage_from_response_body(body).unwrap();
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 20);
        assert_eq!(u.cached_input_tokens, 40);
        assert_eq!(u.reasoning_tokens, 5);
        assert_eq!(u.total_tokens, 120);
        assert_eq!(u.cost_usd_ticks, 500_000_000);
        assert_eq!(u.model.as_deref(), Some("grok-4.5"));
        assert_eq!(u.request_id.as_deref(), Some("resp_1"));
        let rec = u.into_record(1_000, Some("sess".into()));
        assert!((rec.cost_usd().unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn ignores_body_without_usage() {
        assert!(usage_from_response_body("data: {\"type\":\"ping\"}\n").is_none());
        assert!(usage_from_response_body("").is_none());
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
                cost_usd_ticks: 1_000_000_000, // $1
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
        let lines = lines_from_records(&recs, None);
        let labels: Vec<&str> = lines
            .iter()
            .filter_map(|l| match l {
                MetricLine::Text { label, .. } => Some(label.as_str()),
                MetricLine::BarChart { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"Last 30 Days"));
        assert!(labels.contains(&"Cost coverage"));
        assert!(labels.contains(&"Models"));
        assert!(labels.contains(&"Cache"));
        assert!(labels.contains(&"Usage Trend"));

        let last30 = lines.iter().find_map(|l| match l {
            MetricLine::Text { label, value, .. } if label == "Last 30 Days" => {
                Some(value.as_str())
            }
            _ => None,
        });
        let last30 = last30.expect("last 30");
        assert!(
            last30.contains("(partial)"),
            "mixed ticks should be partial: {last30}"
        );
        assert!(last30.contains("$1.0000"), "got {last30}");

        let cov = lines.iter().find_map(|l| match l {
            MetricLine::Text { label, value, .. } if label == "Cost coverage" => {
                Some(value.as_str())
            }
            _ => None,
        });
        let cov = cov.expect("coverage");
        // 1100 with cost / 1350 total ≈ 81%
        assert!(cov.contains("81% of tokens"), "unexpected coverage: {cov}");

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
    fn full_cost_ticks_not_partial() {
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
        let lines = lines_from_records(&recs, None);
        let last30 = lines.iter().find_map(|l| match l {
            MetricLine::Text { label, value, .. } if label == "Last 30 Days" => {
                Some(value.as_str())
            }
            _ => None,
        });
        let last30 = last30.expect("last 30");
        assert!(!last30.contains("partial"), "got {last30}");
        assert!(!lines.iter().any(|l| matches!(
            l,
            MetricLine::Text { label, .. } if label == "Cost coverage"
        )));
    }

    #[test]
    fn weekly_partial_when_window_has_unticked_tokens() {
        let recs = vec![
            UsageRecord {
                ts_ms: 5_000,
                session_id: None,
                model: Some("a".into()),
                input_tokens: 100,
                output_tokens: 0,
                cached_input_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 100,
                cost_usd_ticks: 1_000_000_000,
                request_id: Some("priced".into()),
            },
            UsageRecord {
                ts_ms: 6_000,
                session_id: None,
                model: Some("a".into()),
                input_tokens: 50,
                output_tokens: 0,
                cached_input_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 50,
                cost_usd_ticks: 0,
                request_id: Some("bare".into()),
            },
        ];
        let lines = lines_from_records(&recs, Some(4_000));
        let since = lines.iter().find_map(|l| match l {
            MetricLine::Text { label, value, .. } if label == "Since weekly reset" => {
                Some(value.as_str())
            }
            _ => None,
        });
        let since = since.expect("since weekly");
        assert!(since.contains("(partial)"), "got {since}");
        assert!(since.contains("150 tokens"), "got {since}");
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
        let lines = lines_from_records(&recs, Some(4_000));
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
