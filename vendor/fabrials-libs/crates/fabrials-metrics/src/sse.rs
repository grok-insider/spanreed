//! Parse official `usage` objects out of Responses API / SSE bodies.

use fabrials_model::UsageRecord;
use serde::Deserialize;

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

impl UsagePartial {
    pub fn into_record(
        self,
        ts_ms: i64,
        session_id: Option<String>,
        account_id: Option<String>,
        route: Option<String>,
    ) -> UsageRecord {
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
            account_id,
            route,
            provider: None,
            key_hash: None,
            kind: None,
            duration_ms: None,
            status: None,
            unit: None,
            quantity: None,
            hosted_tools: None,
        }
    }
}

pub fn usage_from_response_body(body: &str) -> Option<UsagePartial> {
    if let Ok(v) = serde_json::from_str::<UsageEnvelope>(body) {
        if let Some(u) = usage_from_json(v) {
            return Some(u);
        }
    }
    let mut best: Option<UsagePartial> = None;
    for line in body.lines() {
        let line = line.trim();
        let payload = line.strip_prefix("data: ").unwrap_or(line);
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<UsageEnvelope>(payload) else {
            continue;
        };
        if let Some(u) = usage_from_json(v) {
            best = Some(u);
        }
    }
    best
}

/// Anthropic Messages streams split usage: `message_start` carries the input
/// tokens under `message.usage`, `message_delta` the cumulative output count.
/// Merge every observed partial by field maximum so both survive.
/// Anthropic reports cache reads and writes beside `input_tokens`; they are
/// folded into the input total with reads as `cached_input_tokens`.
pub fn usage_from_messages_body(body: &str) -> Option<UsagePartial> {
    let mut merged: Option<UsagePartial> = None;
    for line in body.lines() {
        let line = line.trim();
        let payload = line.strip_prefix("data: ").unwrap_or(line);
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<UsageEnvelope>(payload) else {
            continue;
        };
        if let Some(u) = usage_from_json(v) {
            merged = Some(match merged {
                Some(prev) => merge_partials(prev, u),
                None => u,
            });
        }
    }
    merged
}

fn merge_partials(prev: UsagePartial, next: UsagePartial) -> UsagePartial {
    let input_tokens = prev.input_tokens.max(next.input_tokens);
    let output_tokens = prev.output_tokens.max(next.output_tokens);
    UsagePartial {
        input_tokens,
        output_tokens,
        cached_input_tokens: prev.cached_input_tokens.max(next.cached_input_tokens),
        reasoning_tokens: prev.reasoning_tokens.max(next.reasoning_tokens),
        total_tokens: prev
            .total_tokens
            .max(next.total_tokens)
            .max(input_tokens.saturating_add(output_tokens)),
        cost_usd_ticks: prev.cost_usd_ticks.max(next.cost_usd_ticks),
        model: next.model.or(prev.model),
        request_id: prev.request_id.or(next.request_id),
    }
}

#[derive(Deserialize, Default)]
struct UsageEnvelope {
    response: Option<ResponseFields>,
    message: Option<ResponseFields>,
    usage: Option<UsageFields>,
    model: Option<String>,
    id: Option<String>,
}

#[derive(Deserialize, Default)]
struct ResponseFields {
    usage: Option<UsageFields>,
    model: Option<String>,
    id: Option<String>,
}

#[derive(Deserialize, Default)]
struct UsageFields {
    input_tokens: Option<UsageNumber>,
    prompt_tokens: Option<UsageNumber>,
    output_tokens: Option<UsageNumber>,
    completion_tokens: Option<UsageNumber>,
    total_tokens: Option<UsageNumber>,
    input_tokens_details: Option<InputDetails>,
    prompt_tokens_details: Option<InputDetails>,
    output_tokens_details: Option<OutputDetails>,
    cost_in_usd_ticks: Option<UsageNumber>,
    cache_read_input_tokens: Option<UsageNumber>,
    cache_creation_input_tokens: Option<UsageNumber>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum UsageNumber {
    Integer(u64),
    Float(f64),
}

impl UsageNumber {
    fn value(&self) -> u64 {
        match self {
            Self::Integer(value) => *value,
            Self::Float(value) => *value as u64,
        }
    }
}

#[derive(Deserialize, Default)]
struct InputDetails {
    cached_tokens: Option<UsageNumber>,
}

#[derive(Deserialize, Default)]
struct OutputDetails {
    reasoning_tokens: Option<UsageNumber>,
}

fn usage_from_json(v: UsageEnvelope) -> Option<UsagePartial> {
    let response = v.response.or(v.message).unwrap_or_default();
    let usage = response.usage.or(v.usage)?;
    let num = |value: Option<&UsageNumber>| value.map(UsageNumber::value).unwrap_or(0);
    let cache_read = num(usage.cache_read_input_tokens.as_ref());
    let cache_creation = num(usage.cache_creation_input_tokens.as_ref());
    let input = num(usage.input_tokens.as_ref())
        .saturating_add(cache_read)
        .saturating_add(cache_creation)
        .max(num(usage.prompt_tokens.as_ref()));
    let output = num(usage.output_tokens.as_ref()).max(num(usage.completion_tokens.as_ref()));
    let total = num(usage.total_tokens.as_ref());
    let cached = [&usage.input_tokens_details, &usage.prompt_tokens_details]
        .into_iter()
        .flatten()
        .map(|details| num(details.cached_tokens.as_ref()))
        .max()
        .unwrap_or(0)
        .max(cache_read);
    let reasoning = usage
        .output_tokens_details
        .as_ref()
        .map(|details| num(details.reasoning_tokens.as_ref()))
        .unwrap_or(0);
    let cost_ticks = num(usage.cost_in_usd_ticks.as_ref());

    if input == 0 && output == 0 && total == 0 && cost_ticks == 0 {
        return None;
    }

    let model = response.model.or(v.model).and_then(bounded_identifier);
    let request_id = response.id.or(v.id).and_then(bounded_identifier);

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

fn bounded_identifier(mut value: String) -> Option<String> {
    let start = value.len().saturating_sub(value.trim_start().len());
    let end = value.trim_end().len();
    value.truncate(end);
    if start > 0 {
        value.drain(..start);
    }
    (!value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control))
        .then_some(value)
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
        let rec = u.into_record(1_000, Some("sess".into()), None, Some("grok".into()));
        assert!((rec.ticks_usd().unwrap() - 0.5).abs() < 1e-9);
        let j = serde_json::to_string(&rec).unwrap();
        assert!(!j.contains("access_token"));
        assert!(!j.contains("refresh_token"));
    }

    #[test]
    fn ignores_body_without_usage() {
        assert!(usage_from_response_body("data: {\"type\":\"ping\"}\n").is_none());
        assert!(usage_from_response_body("").is_none());
    }

    #[test]
    fn merges_anthropic_message_start_and_delta() {
        let body = "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"union-alpha\",\"usage\":{\"input_tokens\":1200,\"output_tokens\":3,\"cache_read_input_tokens\":900}}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"OK\"}}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":57}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n";
        let u = usage_from_messages_body(body).unwrap();
        assert_eq!(u.input_tokens, 2100);
        assert_eq!(u.cached_input_tokens, 900);
        assert_eq!(u.output_tokens, 57);
        assert_eq!(u.total_tokens, 2157);
        assert_eq!(u.model.as_deref(), Some("union-alpha"));
        assert_eq!(u.request_id.as_deref(), Some("msg_1"));
    }

    #[test]
    fn messages_body_ignores_usage_less_events_and_keeps_plain_json() {
        assert!(usage_from_messages_body("event: ping\ndata: {\"type\":\"ping\"}\n").is_none());
        assert!(usage_from_messages_body("").is_none());
        let plain =
            r#"{"id":"msg_2","model":"union-alpha","usage":{"input_tokens":10,"output_tokens":4}}"#;
        let u = usage_from_messages_body(plain).unwrap();
        assert_eq!(u.input_tokens, 10);
        assert_eq!(u.output_tokens, 4);
        assert_eq!(u.model.as_deref(), Some("union-alpha"));
        assert_eq!(u.request_id.as_deref(), Some("msg_2"));
    }

    #[test]
    fn ignores_large_payload_fields_and_bounds_identifiers() {
        let payload = "x".repeat(1024 * 1024);
        let body = format!(
            "{{\"output\":\"{payload}\",\"model\":\"{}\",\"usage\":{{\"prompt_tokens\":2.0,\"completion_tokens\":3}}}}",
            "m".repeat(300)
        );
        let usage = usage_from_response_body(&body).expect("usage");
        assert_eq!(usage.input_tokens, 2);
        assert_eq!(usage.output_tokens, 3);
        assert_eq!(usage.total_tokens, 5);
        assert!(usage.model.is_none());
    }

    #[test]
    fn folds_anthropic_cache_reads_and_writes_into_input() {
        let body = "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_3\",\"model\":\"claude-opus-5-5\",\"usage\":{\"input_tokens\":2,\"cache_creation_input_tokens\":1500,\"cache_read_input_tokens\":48000,\"output_tokens\":1}}}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":2,\"cache_creation_input_tokens\":1500,\"cache_read_input_tokens\":48000,\"output_tokens\":580}}\n";
        let u = usage_from_messages_body(body).unwrap();
        assert_eq!(u.input_tokens, 49_502);
        assert_eq!(u.cached_input_tokens, 48_000);
        assert_eq!(u.output_tokens, 580);
        assert_eq!(u.total_tokens, 50_082);
    }

    #[test]
    fn reads_chat_completions_prompt_cache_details() {
        let body = r#"{"model":"m","usage":{"prompt_tokens":1000,"completion_tokens":10,"total_tokens":1010,"prompt_tokens_details":{"cached_tokens":800}}}"#;
        let u = usage_from_response_body(body).unwrap();
        assert_eq!(u.input_tokens, 1000);
        assert_eq!(u.cached_input_tokens, 800);
    }
}
