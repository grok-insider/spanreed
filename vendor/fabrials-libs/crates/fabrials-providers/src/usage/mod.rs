//! Local usage adapters. No credential discovery or network calls belong here.
pub mod catalog;
pub mod claude;
pub mod codex;
pub mod files;
pub mod remote;

use fabrials_core::usage::{Granularity, Tokens, UsageOrigin, UsageRecord};
use serde_json::Value;

pub(crate) fn timestamp(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(if number < 100_000_000_000 {
            number.checked_mul(1000)?
        } else {
            number
        });
    }
    time::OffsetDateTime::parse(
        value.as_str()?,
        &time::format_description::well_known::Rfc3339,
    )
    .ok()
    .and_then(|t| i64::try_from(t.unix_timestamp_nanos() / 1_000_000).ok())
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

pub(crate) fn record(client: &str, id: String, at_ms: i64, tokens: Tokens) -> UsageRecord {
    UsageRecord {
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
