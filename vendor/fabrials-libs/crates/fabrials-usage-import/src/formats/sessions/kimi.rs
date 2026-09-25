// Adapted from Tokscale 0d621cae343af9b057fb4a38c84d089524ac4378.
// Copyright (c) 2025 Junho Yeo. MIT; see TOKSCALE-LICENSE.
//! Kimi CLI / Kimi Code session parser
//!
//! Parses wire.jsonl from both `kimi-cli` and `kimi-code`.
//!
//! `~/.kimi/sessions/[GROUP_ID]/[SESSION_UUID]/wire.jsonl`
//!   Token data comes from StatusUpdate messages.
//!
//! `~/.kimi-code/sessions/[WORKSPACE]/[SESSION]/agents/[AGENT]/wire.jsonl`
//!   Token data comes from usage.record lines.

use super::utils::{file_modified_timestamp_ms, for_each_json_line};
use super::UnifiedMessage;
use crate::formats::TokenBreakdown;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Top-level wire.jsonl line: either metadata or a timestamped message
#[derive(Debug, Deserialize)]
struct WireLine {
    timestamp: Option<f64>,
    message: Option<WireMessage>,
    #[serde(rename = "type")]
    line_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireMessage {
    #[serde(rename = "type")]
    msg_type: String,
    payload: Option<StatusPayload>,
}

#[derive(Debug, Deserialize)]
struct StatusPayload {
    token_usage: Option<TokenUsage>,
    #[allow(dead_code)]
    message_id: Option<String>,
}

/// Token usage counts shared by both wire formats.
///
/// Legacy kimi-cli StatusUpdate payloads use snake_case field names;
/// kimi-code usage.record lines use the camelCase aliases.
#[derive(Debug, Deserialize)]
struct TokenUsage {
    #[serde(alias = "inputOther")]
    input_other: Option<i64>,
    output: Option<i64>,
    #[serde(alias = "inputCacheRead")]
    input_cache_read: Option<i64>,
    #[serde(alias = "inputCacheCreation")]
    input_cache_creation: Option<i64>,
}

impl TokenUsage {
    /// Clamp negative counts to zero and build a breakdown.
    /// Returns `None` when every count is zero so callers can skip the entry.
    fn to_breakdown(&self) -> Option<TokenBreakdown> {
        let input = self.input_other.unwrap_or(0).max(0);
        let output = self.output.unwrap_or(0).max(0);
        let cache_read = self.input_cache_read.unwrap_or(0).max(0);
        let cache_write = self.input_cache_creation.unwrap_or(0).max(0);

        if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
            return None;
        }

        Some(TokenBreakdown {
            input,
            output,
            cache_read,
            cache_write,
            // Kimi wire protocols do not expose reasoning tokens; all reasoning included in output
            reasoning: 0,
        })
    }
}

/// Default model name when config.json is not available
const DEFAULT_MODEL: &str = "kimi-for-coding";
const DEFAULT_PROVIDER: &str = "moonshot";

/// Locate the legacy Kimi CLI config consumed by `parse_kimi_file`. Kimi Code
/// embeds model information in each wire record and does not use this file.
pub(crate) fn kimi_config_path(wire_path: &Path) -> Option<PathBuf> {
    let sessions_dir = wire_path.parent()?.parent()?.parent()?;
    Some(sessions_dir.parent()?.join("config.json"))
}

/// Read model name from ~/.kimi/config.json if available
fn read_model_from_config(wire_path: &Path) -> String {
    if let Some(config_path) = kimi_config_path(wire_path) {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(bytes) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(model) = bytes.get("model").and_then(|v| v.as_str()) {
                    if !model.is_empty() {
                        return model.to_string();
                    }
                }
            }
        }
    }
    DEFAULT_MODEL.to_string()
}

/// Extract session ID from the wire.jsonl path
/// Path format: ~/.kimi/sessions/GROUP_ID/SESSION_UUID/wire.jsonl
fn extract_session_id(path: &Path) -> String {
    path.parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Parse a Kimi CLI wire.jsonl file
pub fn parse_kimi_file(path: &Path) -> Vec<UnifiedMessage> {
    let model = read_model_from_config(path);
    let session_id = extract_session_id(path);
    let fallback_timestamp = file_modified_timestamp_ms(path);

    let mut messages: Vec<UnifiedMessage> = Vec::new();
    let mut timestamp_sources: Vec<TimestampSource> = Vec::new();
    let mut keyed_indices: HashMap<String, usize> = HashMap::new();

    for_each_json_line(path, &mut |_index, trimmed| {
        let mut bytes = trimmed.as_bytes().to_vec();
        let wire_line = match simd_json::from_slice::<WireLine>(&mut bytes) {
            Ok(wl) => wl,
            Err(_) => return,
        };

        // Skip metadata lines (first line: {"type": "metadata", ...})
        if wire_line.line_type.as_deref() == Some("metadata") {
            return;
        }

        let message = match wire_line.message {
            Some(m) => m,
            None => return,
        };

        // Only process StatusUpdate messages
        if message.msg_type != "StatusUpdate" {
            return;
        }

        let payload = match message.payload {
            Some(p) => p,
            None => return,
        };

        let token_usage = match payload.token_usage {
            Some(u) => u,
            None => return,
        };

        // Convert Unix seconds (float) to milliseconds, falling back to file
        // mtime when the wire value is missing or does not convert to a
        // positive instant. A corrupt `{"timestamp": -1.5}` would otherwise
        // anchor the message in a pre-epoch daily bucket; the float->int cast
        // also collapses NaN to 0, so the same check catches that.
        let (timestamp_ms, timestamp_source) = match wire_line
            .timestamp
            .map(|ts| (ts * 1000.0) as i64)
            .filter(|ms| *ms > 0)
        {
            Some(ms) => (ms, TimestampSource::Wire),
            None => (fallback_timestamp, TimestampSource::FileMtime),
        };

        // Skip entries with zero tokens
        let Some(tokens) = token_usage.to_breakdown() else {
            return;
        };

        let dedup_key = payload.message_id;

        let message = UnifiedMessage::new_with_dedup(
            "kimi",
            model.clone(),
            DEFAULT_PROVIDER,
            session_id.clone(),
            timestamp_ms,
            tokens,
            0.0,
            dedup_key,
        );
        push_or_replace_status_update(
            &mut messages,
            &mut timestamp_sources,
            &mut keyed_indices,
            message,
            timestamp_source,
        );
    });

    messages
}

fn exact_token_total(tokens: &TokenBreakdown) -> i128 {
    i128::from(tokens.input)
        + i128::from(tokens.output)
        + i128::from(tokens.cache_read)
        + i128::from(tokens.cache_write)
        + i128::from(tokens.reasoning)
}

/// Where a StatusUpdate's anchor came from. The mtime fallback is a guess for a
/// line whose own timestamp was unusable, so it ranks below a real wire value
/// when duplicates are compared.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TimestampSource {
    Wire,
    FileMtime,
}

fn should_replace_status_update(
    existing: (&UnifiedMessage, TimestampSource),
    candidate: (&UnifiedMessage, TimestampSource),
) -> bool {
    let (existing, existing_source) = existing;
    let (candidate, candidate_source) = candidate;
    let existing_total = exact_token_total(&existing.tokens);
    let candidate_total = exact_token_total(&candidate.tokens);

    if candidate_total != existing_total {
        return candidate_total > existing_total;
    }

    // Totals tie, so the anchor decides. Compare provenance before the value:
    // mtime is >= every real timestamp in a file still being written, so a
    // corrupt duplicate that fell back to it would otherwise outrank the good
    // line it duplicates and move the message off its true day.
    if existing_source != candidate_source {
        return candidate_source == TimestampSource::Wire;
    }

    candidate.timestamp >= existing.timestamp
}

fn push_or_replace_status_update(
    messages: &mut Vec<UnifiedMessage>,
    timestamp_sources: &mut Vec<TimestampSource>,
    keyed_indices: &mut HashMap<String, usize>,
    message: UnifiedMessage,
    timestamp_source: TimestampSource,
) {
    let dedup_key = message
        .dedup_key
        .as_ref()
        .filter(|key| !key.is_empty())
        .cloned();

    let Some(dedup_key) = dedup_key else {
        messages.push(message);
        timestamp_sources.push(timestamp_source);
        return;
    };

    if let Some(index) = keyed_indices.get(&dedup_key).copied() {
        if should_replace_status_update(
            (&messages[index], timestamp_sources[index]),
            (&message, timestamp_source),
        ) {
            messages[index] = message;
            timestamp_sources[index] = timestamp_source;
        }
        return;
    }

    let index = messages.len();
    messages.push(message);
    timestamp_sources.push(timestamp_source);
    keyed_indices.insert(dedup_key, index);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn test_parse_kimi_valid_status_update() {
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": 1770983426.420942, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 1562, "output": 2463, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "chatcmpl-xxx"}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].client, "kimi");
        assert_eq!(messages[0].model_id, "kimi-for-coding");
        assert_eq!(messages[0].provider_id, "moonshot");
        assert_eq!(messages[0].tokens.input, 1562);
        assert_eq!(messages[0].tokens.output, 2463);
        assert_eq!(messages[0].tokens.cache_read, 0);
        assert_eq!(messages[0].tokens.cache_write, 0);
        // Timestamp: 1770983426.420942 * 1000 = 1770983426420
        assert_eq!(messages[0].timestamp, 1770983426420);
    }

    #[test]
    fn test_parse_kimi_multi_turn() {
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": 1770983400.0, "message": {"type": "TurnBegin", "payload": {"user_input": "hello"}}}
{"timestamp": 1770983410.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 100, "output": 200, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-1"}}}
{"timestamp": 1770983420.0, "message": {"type": "TurnBegin", "payload": {"user_input": "world"}}}
{"timestamp": 1770983430.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 300, "output": 400, "input_cache_read": 50, "input_cache_creation": 0}, "message_id": "msg-2"}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].tokens.input, 100);
        assert_eq!(messages[0].tokens.output, 200);
        assert_eq!(messages[1].tokens.input, 300);
        assert_eq!(messages[1].tokens.output, 400);
        assert_eq!(messages[1].tokens.cache_read, 50);
    }

    #[test]
    fn test_parse_kimi_skip_non_status_update() {
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": 1770983400.0, "message": {"type": "TurnBegin", "payload": {"user_input": "hello"}}}
{"timestamp": 1770983410.0, "message": {"type": "ContentPart", "payload": {"type": "text", "text": "response"}}}
{"timestamp": 1770983420.0, "message": {"type": "ToolCall", "payload": {"type": "function", "id": "tool_1", "function": {"name": "ReadFile", "arguments": "{}"}}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert!(messages.is_empty());
    }

    #[test]
    fn test_parse_kimi_empty_file() {
        let file = create_test_file("");

        let messages = parse_kimi_file(file.path());

        assert!(messages.is_empty());
    }

    #[test]
    fn test_parse_kimi_tool_call_multi_step() {
        // Simulates a tool-call scenario with multiple StatusUpdate messages in one turn
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": 1770983400.0, "message": {"type": "TurnBegin", "payload": {"user_input": "read file"}}}
{"timestamp": 1770983405.0, "message": {"type": "StepBegin", "payload": {"n": 1}}}
{"timestamp": 1770983410.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 500, "output": 100, "input_cache_read": 200, "input_cache_creation": 0}, "message_id": "msg-step1"}}}
{"timestamp": 1770983415.0, "message": {"type": "ToolCall", "payload": {"type": "function", "id": "tool_1", "function": {"name": "ReadFile", "arguments": "{}"}}}}
{"timestamp": 1770983420.0, "message": {"type": "StepBegin", "payload": {"n": 2}}}
{"timestamp": 1770983425.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 800, "output": 300, "input_cache_read": 400, "input_cache_creation": 100}, "message_id": "msg-step2"}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 2);
        // Step 1
        assert_eq!(messages[0].tokens.input, 500);
        assert_eq!(messages[0].tokens.output, 100);
        assert_eq!(messages[0].tokens.cache_read, 200);
        assert_eq!(messages[0].tokens.cache_write, 0);
        // Step 2
        assert_eq!(messages[1].tokens.input, 800);
        assert_eq!(messages[1].tokens.output, 300);
        assert_eq!(messages[1].tokens.cache_read, 400);
        assert_eq!(messages[1].tokens.cache_write, 100);
    }

    #[test]
    fn test_parse_kimi_with_cache_tokens() {
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": 1771123711.615454, "message": {"type": "StatusUpdate", "payload": {"context_usage": 0.024, "token_usage": {"input_other": 1508, "output": 205, "input_cache_read": 4864, "input_cache_creation": 0}, "message_id": "chatcmpl-2tNw2mhUNfdPMP0Jyie7gDhD"}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].tokens.input, 1508);
        assert_eq!(messages[0].tokens.output, 205);
        assert_eq!(messages[0].tokens.cache_read, 4864);
        assert_eq!(messages[0].tokens.cache_write, 0);
    }

    #[test]
    fn test_parse_kimi_deduplicates_repeated_status_updates_by_message_id() {
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": 1770983410.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 100, "output": 10, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-progressive"}}}
{"timestamp": 1770983420.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 120, "output": 30, "input_cache_read": 5, "input_cache_creation": 0}, "message_id": "msg-progressive"}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].dedup_key.as_deref(), Some("msg-progressive"));
        assert_eq!(messages[0].tokens.input, 120);
        assert_eq!(messages[0].tokens.output, 30);
        assert_eq!(messages[0].tokens.cache_read, 5);
        assert_eq!(messages[0].timestamp, 1770983420000);
    }

    #[test]
    fn test_parse_kimi_keeps_larger_extreme_status_update() {
        // Both saturating totals equal i64::MAX, but the first exact total is larger.
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": 1770983410.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 9223372036854775807, "output": 9223372036854775807, "input_cache_read": 2, "input_cache_creation": 0}, "message_id": "msg-extreme"}}}
{"timestamp": 1770983420.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 9223372036854775807, "output": 0, "input_cache_read": 1, "input_cache_creation": 0}, "message_id": "msg-extreme"}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].dedup_key.as_deref(), Some("msg-extreme"));
        assert_eq!(messages[0].tokens.input, i64::MAX);
        assert_eq!(messages[0].tokens.output, i64::MAX);
        assert_eq!(messages[0].tokens.cache_read, 2);
        assert_eq!(messages[0].tokens.cache_write, 0);
        assert_eq!(messages[0].timestamp, 1770983410000);
    }

    #[test]
    fn test_parse_kimi_keeps_distinct_and_missing_message_ids_separate() {
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": 1770983410.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 10, "output": 1, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-1"}}}
{"timestamp": 1770983420.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 20, "output": 2, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-2"}}}
{"timestamp": 1770983430.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 30, "output": 3, "input_cache_read": 0, "input_cache_creation": 0}}}}
{"timestamp": 1770983440.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 40, "output": 4, "input_cache_read": 0, "input_cache_creation": 0}}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].dedup_key.as_deref(), Some("msg-1"));
        assert_eq!(messages[1].dedup_key.as_deref(), Some("msg-2"));
        assert!(messages[2].dedup_key.is_none());
        assert!(messages[3].dedup_key.is_none());
    }

    #[test]
    fn test_parse_kimi_skips_zero_token_entries() {
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": 1770983410.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 0, "output": 0, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-empty"}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert!(messages.is_empty());
    }

    #[test]
    fn test_parse_kimi_keeps_extreme_buckets_and_skips_only_all_zero() {
        // MAX + MAX + 2 panics in debug and wraps to zero in release.
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": 1770983410.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 9223372036854775807, "output": 9223372036854775807, "input_cache_read": 2, "input_cache_creation": 0}, "message_id": "msg-extreme"}}}
{"timestamp": 1770983420.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 0, "output": 0, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-zero"}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].tokens.input, i64::MAX);
        assert_eq!(messages[0].tokens.output, i64::MAX);
        assert_eq!(messages[0].tokens.cache_read, 2);
        assert_eq!(messages[0].tokens.cache_write, 0);
    }

    #[test]
    fn test_parse_kimi_non_positive_timestamps_fall_back_to_mtime() {
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": -1.5, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 10, "output": 1, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-negative"}}}
{"timestamp": 0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 20, "output": 2, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-zero"}}}
{"timestamp": 1770983426.420942, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 30, "output": 3, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-valid"}}}"#;
        let file = create_test_file(content);
        let mtime = file_modified_timestamp_ms(file.path());

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 3);
        // -1.5s would otherwise become -1500ms and bucket into 1969-12-31 (UTC;
        // the exact pre-epoch day depends on the local zone, the mis-dating
        // does not).
        assert_eq!(messages[0].tokens.input, 10);
        assert_eq!(messages[0].timestamp, mtime);
        assert_eq!(messages[1].tokens.input, 20);
        assert_eq!(messages[1].timestamp, mtime);
        assert_eq!(messages[2].tokens.input, 30);
        assert_eq!(messages[2].timestamp, 1770983426420);
    }

    #[test]
    fn test_parse_kimi_mtime_fallback_does_not_outrank_a_real_timestamp() {
        // Same message_id, same totals, second line's timestamp unusable. The
        // fallback lands on mtime, which is newer than every real timestamp in
        // a live session file, so an untied comparison would let the corrupt
        // line's anchor replace the good one.
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": 1770983426.420942, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 100, "output": 10, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-dup"}}}
{"timestamp": -1, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 100, "output": 10, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-dup"}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].timestamp, 1770983426420);
    }

    #[test]
    fn test_parse_kimi_real_timestamp_still_wins_a_tie_over_mtime_fallback() {
        // Mirror of the above with the corrupt line first, so the good anchor
        // arrives as the candidate rather than the incumbent.
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
{"timestamp": -1, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 100, "output": 10, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-dup"}}}
{"timestamp": 1770983426.420942, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 100, "output": 10, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-dup"}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].timestamp, 1770983426420);
    }

    #[test]
    fn test_parse_kimi_malformed_lines() {
        let content = r#"{"type": "metadata", "protocol_version": "1.3"}
not valid json at all
{"timestamp": 1770983410.0, "message": {"type": "StatusUpdate", "payload": {"token_usage": {"input_other": 100, "output": 200, "input_cache_read": 0, "input_cache_creation": 0}, "message_id": "msg-1"}}}"#;
        let file = create_test_file(content);

        let messages = parse_kimi_file(file.path());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].tokens.input, 100);
    }

    // -------------------------------------------------------------------------
    // Kimi Code tests
    // -------------------------------------------------------------------------

    // -------------------------------------------------------------------------
    // Kimi Code workspace resolution tests
    // -------------------------------------------------------------------------
}
