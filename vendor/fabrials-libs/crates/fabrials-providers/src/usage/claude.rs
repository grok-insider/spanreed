use super::{complete_lines, number, record, text, timestamp};
use fabrials_core::usage::{
    CostOrigin, ImportCheckpoint, ParsedUsage, Tokens, UsageCost, UsageParser,
};
use serde_json::Value;

pub struct Claude;

impl UsageParser for Claude {
    fn client(&self) -> &str {
        "claude"
    }
    fn version(&self) -> u32 {
        1
    }
    fn incremental(&self) -> bool {
        true
    }
    fn parse(
        &self,
        bytes: &[u8],
        source: &str,
        previous: &ImportCheckpoint,
    ) -> Result<ParsedUsage, String> {
        let mut result = ParsedUsage {
            checkpoint: previous.clone(),
            ..ParsedUsage::default()
        };
        let mut records = std::collections::BTreeMap::new();
        for (offset, line) in complete_lines(bytes) {
            result.checkpoint.offset = previous.offset + (offset + line.len()) as u64;
            let value: Value = match serde_json::from_slice(line) {
                Ok(value) => value,
                Err(_) => {
                    result.rejected_records += 1;
                    continue;
                }
            };
            if value["type"] != "assistant" {
                continue;
            }
            let message = &value["message"];
            let usage = &message["usage"];
            if !usage.is_object() {
                continue;
            }
            result.recognized_records += 1;
            let Some(at) = timestamp(&value["timestamp"]) else {
                result.rejected_records += 1;
                continue;
            };
            let read = number(usage, "cache_read_input_tokens");
            let write = number(usage, "cache_creation_input_tokens");
            let Some(input) = number(usage, "input_tokens")
                .checked_add(read)
                .and_then(|n| n.checked_add(write))
            else {
                result.rejected_records += 1;
                continue;
            };
            let tokens = Tokens {
                input,
                output: number(usage, "output_tokens"),
                cache_read: read,
                cache_write: write,
                reasoning: 0,
            };
            let session = text(&value, "sessionId").unwrap_or_else(|| source.into());
            let identity = text(message, "id")
                .unwrap_or_else(|| format!("offset:{}", previous.offset + offset as u64));
            let request = text(&value, "requestId");
            let mut entry = record(
                "claude",
                format!("{session}:{identity}:{}", request.as_deref().unwrap_or("")),
                at,
                tokens,
            );
            entry.session = Some(session);
            entry.provider = Some("anthropic".into());
            entry.model = text(message, "model");
            entry.project = text(&value, "cwd");
            entry.request_id = request;
            entry.cost = value["costUSD"].as_f64().map(|usd| UsageCost {
                usd,
                origin: CostOrigin::ClientReported,
                pricing_revision: None,
            });
            if entry.validate().is_err() {
                result.rejected_records += 1;
                continue;
            }
            // Streaming assistant messages can revise the same request's usage.
            records.insert(entry.id.clone(), entry);
        }
        result.records = records.into_values().collect();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streaming_updates_replace_and_cache_is_counted_once() {
        let mut bytes = Vec::new();
        for output in [2, 5] {
            let row = serde_json::json!({"type":"assistant","sessionId":"s","requestId":"r","timestamp":"2026-09-01T00:00:00Z","message":{"id":"m","model":"claude","usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":30,"output_tokens":output}}});
            bytes.extend(serde_json::to_vec(&row).unwrap());
            bytes.push(b'\n');
        }
        bytes.extend(b"{unfinished");
        let parsed = Claude
            .parse(&bytes, "path", &ImportCheckpoint::default())
            .unwrap();
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.records[0].tokens.as_ref().unwrap().total(), 65);
        assert_eq!(parsed.checkpoint.offset, bytes.len() as u64 - 11);
    }
}
