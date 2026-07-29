//! Local-log cost estimation for Claude and Codex.
//!
//! Parses the CLIs' local session logs, prices token usage with the embedded
//! pricing table, and aggregates per-day cost over a rolling window. Designed
//! to be cheap on repeated runs:
//! - **mtime pre-filter**: append-only session files older than the window are
//!   skipped without reading them.
//! - **memchr pre-filter**: only lines containing the usage marker are JSON
//!   parsed.
//! - **parallel reads** across worker threads.
//! - **TTL cache** of the aggregate so back-to-back probes don't rescan.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use memchr::memmem;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::creds;
use crate::pricing::{self, Usage};
use crate::usage_stats::{self, CacheTotals, ModelCost};
use crate::util;

/// Rolling window: today plus the previous 30 days.
const WINDOW_DAYS: i64 = 31;
const DAY_MS: i64 = 86_400_000;
/// Reuse a cached aggregate for this long before recomputing.
const CACHE_TTL_SECS: u64 = 300;

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
/// `Last 30 Days`, model/cache breakdown, and a `Usage Trend` sparkline.
/// Returns an empty vec when there is no local usage data.
pub fn cost_lines(source: Source) -> Vec<crate::model::MetricLine> {
    use crate::model::{BarChartPoint, MetricLine};

    let summary = match estimate(source) {
        Some(s) if s.total_tokens > 0 => s,
        _ => return Vec::new(),
    };

    let mut lines = Vec::new();
    let tokens = util::fmt_tokens(summary.total_tokens);
    let value = if summary.total_cost > 0.0 {
        let suffix = if summary.partial { " (partial)" } else { "" };
        format!("~${:.2} · {} tokens{}", summary.total_cost, tokens, suffix)
    } else {
        format!("{tokens} tokens")
    };
    lines.push(MetricLine::text("Last 30 Days", value));
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

/// Estimate cost from local logs (TTL-cached).
pub fn estimate(source: Source) -> Option<CostSummary> {
    if let Some(cached) = read_cache(source) {
        return Some(cached);
    }
    let summary = compute(source)?;
    write_cache(source, &summary);
    Some(summary)
}

fn compute(source: Source) -> Option<CostSummary> {
    // Pull fresh model prices (TTL-cached, silent on failure) before the
    // pricing table is first built, so new models are priced without a
    // new binary.
    pricing::ensure_fresh();
    let cutoff = util::now_ms() - WINDOW_DAYS * DAY_MS;
    let files = collect_files(source, cutoff);
    if files.is_empty() {
        return None;
    }

    let entries = read_files_parallel(source, &files, cutoff);
    if entries.is_empty() {
        return Some(CostSummary {
            total_cost: 0.0,
            total_tokens: 0,
            partial: false,
            daily: Vec::new(),
            by_model: Vec::new(),
            cache: CacheTotals::default(),
        });
    }

    aggregate(entries)
}

fn aggregate(mut entries: Vec<Entry>) -> Option<CostSummary> {
    let pricing = pricing::table();

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
            None => match e.model.as_deref().and_then(|m| pricing.cost(m, e.usage)) {
                Some(c) => c,
                None => {
                    partial = true;
                    0.0
                }
            },
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

fn read_files_parallel(source: Source, files: &[PathBuf], cutoff: i64) -> Vec<Entry> {
    let workers = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(files.len())
        .max(1);

    if workers <= 1 {
        return files
            .iter()
            .flat_map(|f| read_file(source, f, cutoff))
            .collect();
    }

    let chunk_size = files.len().div_ceil(workers);
    thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .flat_map(|f| read_file(source, f, cutoff))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

fn read_file(source: Source, path: &Path, cutoff: i64) -> Vec<Entry> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    match source {
        Source::Claude => parse_claude_lines(&bytes, cutoff),
        Source::Codex => parse_codex_lines(&bytes, cutoff),
    }
}

// --- Claude ---

fn claude_finder() -> &'static memmem::Finder<'static> {
    static F: OnceLock<memmem::Finder<'static>> = OnceLock::new();
    F.get_or_init(|| memmem::Finder::new(br#""usage":{"#))
}

fn parse_claude_lines(bytes: &[u8], cutoff: i64) -> Vec<Entry> {
    let finder = claude_finder();
    let mut out = Vec::new();
    for line in bytes.split(|b| *b == b'\n') {
        if line.is_empty() || finder.find(line).is_none() {
            continue;
        }
        if let Some(entry) = parse_claude_value(line, cutoff) {
            out.push(entry);
        }
    }
    out
}

fn parse_claude_value(line: &[u8], cutoff: i64) -> Option<Entry> {
    let d: serde_json::Value = serde_json::from_slice(line).ok()?;
    let message = d.get("message")?;
    let usage_obj = message.get("usage")?;
    let ts_ms = d
        .get("timestamp")
        .and_then(|v| v.as_str())
        .and_then(parse_iso_ms)?;
    if ts_ms < cutoff {
        return None;
    }
    let usage = Usage {
        input: u64_of(usage_obj.get("input_tokens")),
        output: u64_of(usage_obj.get("output_tokens")),
        cache_create: u64_of(usage_obj.get("cache_creation_input_tokens")),
        cache_read: u64_of(usage_obj.get("cache_read_input_tokens")),
    };
    if usage.total() == 0 {
        return None;
    }
    let model = message
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let cost_usd = d.get("costUSD").and_then(|v| v.as_f64());
    let message_id = message.get("id").and_then(|v| v.as_str());
    let request_id = d.get("requestId").and_then(|v| v.as_str());
    let dedup = message_id.map(|mid| dedup_hash(mid, request_id));

    Some(Entry {
        ts_ms,
        model,
        usage,
        cost_usd,
        dedup,
    })
}

fn dedup_hash(message_id: &str, request_id: Option<&str>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    message_id.hash(&mut h);
    request_id.hash(&mut h);
    h.finish()
}

// --- Codex ---

fn codex_finders() -> &'static (memmem::Finder<'static>, memmem::Finder<'static>) {
    static F: OnceLock<(memmem::Finder<'static>, memmem::Finder<'static>)> = OnceLock::new();
    F.get_or_init(|| {
        (
            memmem::Finder::new(br#""turn_context""#),
            memmem::Finder::new(br#""token_count""#),
        )
    })
}

fn parse_codex_lines(bytes: &[u8], cutoff: i64) -> Vec<Entry> {
    let (turn_finder, token_finder) = codex_finders();
    let mut out = Vec::new();
    let mut current_model: Option<String> = None;

    for line in bytes.split(|b| *b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let is_turn = turn_finder.find(line).is_some();
        let is_token = token_finder.find(line).is_some();
        if !is_turn && !is_token {
            continue;
        }
        let d: serde_json::Value = match serde_json::from_slice(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let payload = d.get("payload");

        // Track the active model from turn_context events.
        if d.get("type").and_then(|v| v.as_str()) == Some("turn_context") {
            if let Some(m) = payload
                .and_then(|p| p.get("model"))
                .and_then(|v| v.as_str())
            {
                current_model = Some(m.to_string());
            }
            continue;
        }

        // token_count events carry the per-turn delta in last_token_usage.
        let ptype = payload.and_then(|p| p.get("type")).and_then(|v| v.as_str());
        if ptype != Some("token_count") {
            continue;
        }
        let ts_ms = match d
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(parse_iso_ms)
        {
            Some(t) => t,
            None => continue,
        };
        if ts_ms < cutoff {
            continue;
        }
        let last = payload
            .and_then(|p| p.get("info"))
            .and_then(|i| i.get("last_token_usage"));
        let last = match last {
            Some(l) => l,
            None => continue,
        };
        let input = u64_of(last.get("input_tokens"));
        let cached = u64_of(last.get("cached_input_tokens")).min(input);
        let output = u64_of(last.get("output_tokens"));
        // Codex: input_tokens includes the cached portion; split for pricing
        // (uncached input at input rate, cached at cache-read rate).
        let usage = Usage {
            input: input - cached,
            output,
            cache_create: 0,
            cache_read: cached,
        };
        if usage.total() == 0 {
            continue;
        }
        out.push(Entry {
            ts_ms,
            model: current_model.clone(),
            usage,
            cost_usd: None,
            dedup: None,
        });
    }
    out
}

// --- shared helpers ---

fn u64_of(v: Option<&serde_json::Value>) -> u64 {
    v.and_then(|x| x.as_u64()).unwrap_or(0)
}

fn parse_iso_ms(s: &str) -> Option<i64> {
    OffsetDateTime::parse(s.trim(), &Rfc3339)
        .ok()
        .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
}

fn collect_files(source: Source, cutoff: i64) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    match source {
        Source::Claude => {
            if let Some(dirs) = creds::env("CLAUDE_CONFIG_DIR") {
                for raw in dirs.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    roots.push(creds::expand(raw).join("projects"));
                }
            }
            if roots.is_empty() {
                roots.push(creds::config_home().join("claude").join("projects"));
                roots.push(creds::expand("~/.claude").join("projects"));
            }
        }
        Source::Codex => {
            roots.push(creds::expand("~/.codex").join("sessions"));
        }
    }

    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        if root.is_dir() && seen.insert(root.clone()) {
            collect_jsonl(&root, cutoff, &mut files);
        }
    }
    files
}

/// Recursively collect `*.jsonl` files whose mtime is within the window.
/// Append-only session logs older than the cutoff can't hold in-window entries.
fn collect_jsonl(dir: &Path, cutoff: i64, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() {
            collect_jsonl(&path, cutoff, out);
        } else if ft.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("jsonl")
            && file_mtime_ms(&entry).map(|m| m >= cutoff).unwrap_or(true)
        {
            out.push(path);
        }
    }
}

fn file_mtime_ms(entry: &std::fs::DirEntry) -> Option<i64> {
    let modified = entry.metadata().ok()?.modified().ok()?;
    let dur = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as i64)
}

// --- TTL cache ---

fn cache_path(source: Source) -> PathBuf {
    // v2: includes by_model + cache totals (old blobs without fields still
    // deserialize via #[serde(default)] but we bump the filename so probes
    // recompute once after upgrade instead of serving empty breakdowns).
    creds::cache_home()
        .join("spanreed")
        .join(format!("{}-cost-v2.json", source.id()))
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    saved_at: u64,
    summary: CostSummary,
}

fn read_cache(source: Source) -> Option<CostSummary> {
    let text = creds::read_file(&cache_path(source))?;
    let cache: CacheFile = serde_json::from_str(&text).ok()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    if now.saturating_sub(cache.saved_at) <= CACHE_TTL_SECS {
        Some(cache.summary)
    } else {
        None
    }
}

fn write_cache(source: Source, summary: &CostSummary) {
    let path = cache_path(source);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let cache = CacheFile {
        saved_at: now,
        summary: summary.clone(),
    };
    if let Ok(text) = serde_json::to_string(&cache) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_line_parses_usage_and_dedup() {
        let line = br#"{"timestamp":"2099-01-01T00:00:00.000Z","requestId":"req_1","type":"assistant","message":{"model":"claude-opus-4-8","id":"msg_1","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":5}}}"#;
        let entries = parse_claude_lines(line, 0);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.usage.input, 100);
        assert_eq!(e.usage.total(), 165);
        assert_eq!(e.model.as_deref(), Some("claude-opus-4-8"));
        assert!(e.dedup.is_some());
    }

    #[test]
    fn claude_line_below_cutoff_is_skipped() {
        let line = br#"{"timestamp":"2000-01-01T00:00:00.000Z","message":{"model":"m","id":"x","usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
        // cutoff far in the future -> skipped
        let entries = parse_claude_lines(line, util::now_ms());
        assert!(entries.is_empty());
    }

    #[test]
    fn claude_non_usage_line_skipped_by_memchr() {
        let line = br#"{"type":"user","message":{"role":"user","content":"hello"}}"#;
        assert!(parse_claude_lines(line, 0).is_empty());
    }

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

    #[test]
    fn codex_token_count_uses_last_usage_and_model() {
        let lines = [
            br#"{"timestamp":"2099-01-01T00:00:00Z","type":"turn_context","payload":{"model":"gpt-5-codex"}}"#.to_vec(),
            br#"{"timestamp":"2099-01-01T00:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1000,"cached_input_tokens":600,"output_tokens":200,"total_tokens":1200}}}}"#.to_vec(),
        ]
        .join(&b'\n');
        let entries = parse_codex_lines(&lines, 0);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.model.as_deref(), Some("gpt-5-codex"));
        assert_eq!(e.usage.input, 400); // 1000 - 600 cached
        assert_eq!(e.usage.cache_read, 600);
        assert_eq!(e.usage.output, 200);

        let s = aggregate(entries).unwrap();
        assert_eq!(s.by_model[0].model, "gpt-5-codex");
        assert_eq!(s.cache.cache_read, 600);
        assert_eq!(s.cache.input, 400);
    }
}
