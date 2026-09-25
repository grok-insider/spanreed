//! Local usage import: session readers for local clients, remote usage
//! protocols and normalization to `ConsumptionRecord`. No credential discovery
//! or network calls belong here.
pub mod catalog;
pub mod codex;
pub mod files;
pub mod remote;

pub use files::{SessionReader, READERS};
pub use formats::sessions::{CostSource, UnifiedMessage};
pub use formats::TokenBreakdown;

/// The Grok Build session reader, public so other hosts (the agent host) can
/// read Grok Build transcripts without going through the import pipeline.
pub mod grok_build {
    pub use crate::files::GrokReader as Reader;
    pub use crate::formats::sessions::grok::{
        parse_grok_file, parse_grok_unified_log_file, parse_grok_updates_file,
    };
}

use fabrials_types::consumption::{ConsumptionRecord, Granularity, Tokens, UsageOrigin};
use serde_json::Value;

pub(crate) fn timestamp(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(if number < 100_000_000_000 {
            number.checked_mul(1000)?
        } else {
            number
        });
    }
    formats::sessions::utils::rfc3339_ms(value.as_str()?)
}

pub(crate) fn number(value: &Value, name: &str) -> u64 {
    value.get(name).and_then(Value::as_u64).unwrap_or(0)
}

pub(crate) fn text(value: &Value, name: &str) -> Option<String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub(crate) fn record(client: &str, id: String, at_ms: i64, tokens: Tokens) -> ConsumptionRecord {
    ConsumptionRecord {
        id,
        client: client.into(),
        provider: None,
        account: None,
        session: None,
        project: None,
        model: None,
        service_tier: None,
        at_ms,
        timestamp_inferred: false,
        period_end_ms: None,
        origin: UsageOrigin::LocalLog,
        granularity: Granularity::Request,
        tokens: Some(tokens),
        cost: None,
        request_id: None,
    }
}

pub(crate) fn complete_lines(bytes: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    let end = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let mut offset = 0;
    bytes[..end]
        .split_inclusive(|b| *b == b'\n')
        .map(move |line| {
            let start = offset;
            offset += line.len();
            (start, line)
        })
}

mod formats;
