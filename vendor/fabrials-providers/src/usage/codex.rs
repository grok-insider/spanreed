use super::{complete_lines, number, record, text, timestamp};
use fabrials_core::usage::{ImportCheckpoint, ParsedUsage, Tokens, UsageParser};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct Codex;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Counters {
    input: u64,
    output: u64,
    cached: u64,
    reasoning: u64,
}
impl Counters {
    fn read(value: &Value) -> Option<Self> {
        value.is_object().then(|| Self {
            input: number(value, "input_tokens"),
            output: number(value, "output_tokens"),
            cached: number(value, "cached_input_tokens"),
            reasoning: number(value, "reasoning_output_tokens"),
        })
    }
    fn delta(&self, previous: &Self) -> Option<Self> {
        Some(Self {
            input: self.input.checked_sub(previous.input)?,
            output: self.output.checked_sub(previous.output)?,
            cached: self.cached.checked_sub(previous.cached)?,
            reasoning: self.reasoning.checked_sub(previous.reasoning)?,
        })
    }
    fn tokens(&self) -> Tokens {
        Tokens {
            input: self.input,
            output: self.output,
            cache_read: self.cached.min(self.input),
            cache_write: 0,
            reasoning: self.reasoning.min(self.output),
        }
    }
    fn within(&self, other: &Self) -> bool {
        self.input <= other.input
            && self.output <= other.output
            && self.cached <= other.cached
            && self.reasoning <= other.reasoning
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    fallback_at_ms: Option<i64>,
    session: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    project: Option<String>,
    tier: Option<String>,
    previous: Option<Counters>,
    inherited: Option<Counters>,
    replay: bool,
    parent_meta: bool,
    user_fork: bool,
    child_started_ms: Option<i64>,
    live_turns: std::collections::BTreeSet<String>,
}

fn uuid_ms(id: &str) -> Option<i64> {
    let parts: Vec<_> = id.split('-').collect();
    if parts.len() != 5 || parts[0].len() != 8 || parts[1].len() != 4 || !parts[2].starts_with('7')
    {
        return None;
    }
    i64::from_str_radix(&format!("{}{}", parts[0], parts[1]), 16).ok()
}

impl UsageParser for Codex {
    fn client(&self) -> &str {
        "codex"
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
        let mut state: State = if previous.parser_state.is_null() {
            State::default()
        } else {
            serde_json::from_value(previous.parser_state.clone())
                .map_err(|_| "Invalid Codex parser checkpoint")?
        };
        let mut result = ParsedUsage {
            checkpoint: previous.clone(),
            ..ParsedUsage::default()
        };
        for (offset, line) in complete_lines(bytes) {
            result.checkpoint.offset = previous.offset + (offset + line.len()) as u64;
            // Rollouts contain prompts and responses; only these metadata rows are decoded.
            if ![
                b"session_meta".as_slice(),
                b"turn_context",
                b"token_count",
                b"task_started",
                b"thread_settings_applied",
                b"turn.completed",
                b"thread.started",
                b"\"usage\"",
            ]
            .iter()
            .any(|needle| memchr::memmem::find(line, needle).is_some())
            {
                continue;
            }
            let value: Value = match serde_json::from_slice(line) {
                Ok(v) => v,
                Err(_) => {
                    result.rejected_records += 1;
                    continue;
                }
            };
            let payload = &value["payload"];
            let kind = value["type"].as_str().unwrap_or("");
            let event = payload["type"].as_str().unwrap_or("");
            if kind == "thread.started" {
                state.session = text(&value, "thread_id").or(state.session);
                continue;
            }
            if let Some(usage) = value
                .get("usage")
                .or_else(|| value.pointer("/data/usage"))
                .or_else(|| value.pointer("/result/usage"))
                .or_else(|| value.pointer("/response/usage"))
            {
                result.recognized_records += 1;
                let Some(usage) = Counters::read(usage) else {
                    result.rejected_records += 1;
                    continue;
                };
                if usage.tokens().total() == 0 {
                    continue;
                }
                let explicit = timestamp(&value["timestamp"]);
                let Some(at) = explicit.or(state.fallback_at_ms) else {
                    result.rejected_records += 1;
                    continue;
                };
                let session = state.session.clone().unwrap_or_else(|| source.into());
                let mut entry = record(
                    "codex",
                    format!("{session}:{}", previous.offset + offset as u64),
                    at,
                    usage.tokens(),
                );
                entry.session = Some(session);
                entry.model = text(&value, "model")
                    .or_else(|| text(&value["data"], "model"))
                    .or_else(|| state.model.clone());
                entry.provider = state.provider.clone();
                entry.service_tier = state.tier.clone();
                entry.timestamp_inferred = explicit.is_none();
                entry.validate()?;
                result.records.push(entry);
                continue;
            }
            if kind == "session_meta" {
                if state.session.is_some() {
                    if text(payload, "id") != state.session {
                        state.parent_meta = true;
                    }
                    continue;
                }
                state.session = text(payload, "id");
                state.project = text(payload, "cwd");
                state.provider = text(payload, "model_provider");
                state.child_started_ms = state
                    .session
                    .as_deref()
                    .and_then(uuid_ms)
                    .or_else(|| timestamp(&payload["timestamp"]));
                state.replay = text(payload, "forked_from_id").is_some()
                    || payload
                        .pointer("/source/subagent/thread_spawn/parent_thread_id")
                        .and_then(Value::as_str)
                        .is_some();
                state.user_fork = payload["thread_source"] == "user";
                continue;
            }
            if state.replay {
                if event == "task_started" {
                    if let Some(turn) = text(payload, "turn_id") {
                        let at = uuid_ms(&turn).or_else(|| timestamp(&payload["started_at"]));
                        if state.child_started_ms.is_none()
                            || at.zip(state.child_started_ms).is_some_and(|(a, b)| a >= b)
                        {
                            state.live_turns.insert(turn);
                        }
                    }
                }
                let own_turn = kind == "turn_context"
                    && (!state.parent_meta
                        || text(payload, "turn_id").is_some_and(|turn| {
                            state.live_turns.contains(&turn)
                                || uuid_ms(&turn)
                                    .zip(state.child_started_ms)
                                    .is_some_and(|(a, b)| a > b || (a == b && state.user_fork))
                        }));
                if !own_turn {
                    if event == "token_count" {
                        if let Some(total) = Counters::read(&payload["info"]["total_token_usage"]) {
                            state.previous = Some(total.clone());
                            state.inherited = Some(total);
                        }
                    }
                    continue;
                }
                state.replay = false;
                state.live_turns.clear();
            }
            if kind == "turn_context" {
                state.model = text(payload, "model");
                state.project = text(payload, "cwd").or(state.project);
                if payload.get("service_tier").is_some() {
                    state.tier = text(payload, "service_tier");
                }
                continue;
            }
            if event == "thread_settings_applied" {
                state.tier = text(&payload["thread_settings"], "service_tier");
                continue;
            }
            if kind != "event_msg" || event != "token_count" {
                continue;
            }
            let info = &payload["info"];
            if !info.is_object() {
                continue;
            }
            result.recognized_records += 1;
            let total = Counters::read(&info["total_token_usage"]);
            let last = Counters::read(&info["last_token_usage"]);
            if total
                .as_ref()
                .zip(state.inherited.as_ref())
                .is_some_and(|(n, b)| n.within(b))
            {
                continue;
            }
            let usage = match (&total, &last, &state.previous) {
                (Some(n), _, Some(p)) if n == p => continue,
                (Some(n), Some(l), Some(p)) if n.delta(p).is_none() => {
                    // Mutable snapshots may briefly regress; retain the watermark
                    // for small regressions rather than counting a replayed delta.
                    let now = n.tokens().total();
                    let before = p.tokens().total();
                    if now.saturating_add(l.tokens().total().saturating_mul(2)) >= before
                        || (now as u128) * 100 >= (before as u128) * 98
                    {
                        continue;
                    }
                    l.clone()
                }
                (_, Some(l), _) => l.clone(),
                (Some(n), None, Some(p)) => match n.delta(p) {
                    Some(delta) => delta,
                    None => {
                        state.previous = total;
                        continue;
                    }
                },
                (Some(n), None, None) => n.clone(),
                _ => continue,
            };
            if usage.tokens().total() == 0 {
                continue;
            }
            if let Some(total) = total {
                state.previous = Some(total);
            } else if let Some(previous) = state.previous.as_mut() {
                previous.input = previous.input.saturating_add(usage.input);
                previous.output = previous.output.saturating_add(usage.output);
                previous.cached = previous.cached.saturating_add(usage.cached);
                previous.reasoning = previous.reasoning.saturating_add(usage.reasoning);
            }
            state.inherited = None;
            let Some(at) = timestamp(&value["timestamp"]) else {
                result.rejected_records += 1;
                continue;
            };
            let session = state.session.clone().unwrap_or_else(|| source.into());
            let mut entry = record(
                "codex",
                format!("{session}:{}", previous.offset + offset as u64),
                at,
                usage.tokens(),
            );
            entry.session = Some(session);
            entry.project = state.project.clone();
            entry.provider = state.provider.clone();
            entry.model = text(payload, "model")
                .or_else(|| text(info, "model"))
                .or_else(|| state.model.clone());
            entry.service_tier = state.tier.clone();
            entry.request_id = text(payload, "request_id");
            if entry.validate().is_err() {
                result.rejected_records += 1;
                continue;
            }
            result.records.push(entry);
        }
        result.checkpoint.parser_state = serde_json::to_value(state).map_err(|e| e.to_string())?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(last: Option<u64>, total: u64) -> Value {
        serde_json::json!({"type":"event_msg","timestamp":"2026-09-01T00:00:00Z","payload":{"type":"token_count","info":{
            "last_token_usage":last.map(|n|serde_json::json!({"input_tokens":n,"cached_input_tokens":n/2})),"total_token_usage":{"input_tokens":total,"cached_input_tokens":total/2}}}})
    }
    fn bytes(rows: &[Value]) -> Vec<u8> {
        rows.iter()
            .flat_map(|v| {
                let mut b = serde_json::to_vec(v).unwrap();
                b.push(b'\n');
                b
            })
            .collect()
    }
    #[test]
    fn duplicate_snapshots_do_not_count_and_checkpoint_survives_restart() {
        let first = Codex
            .parse(
                &bytes(&[row(Some(10), 10), row(Some(10), 10)]),
                "s",
                &ImportCheckpoint::default(),
            )
            .unwrap();
        assert_eq!(first.records.len(), 1);
        let next = Codex
            .parse(
                &bytes(&[row(Some(10), 10), row(None, 30)]),
                "s",
                &first.checkpoint,
            )
            .unwrap();
        assert_eq!(next.records.len(), 1);
        assert_eq!(next.records[0].tokens.as_ref().unwrap().total(), 20);
    }
    #[test]
    fn inherited_history_is_not_child_usage() {
        let rows = bytes(&[
            serde_json::json!({"type":"session_meta","payload":{"id":"child","forked_from_id":"parent"}}),
            row(Some(100), 100),
            row(Some(20), 120),
            serde_json::json!({"type":"turn_context","payload":{"model":"gpt-test"}}),
            row(Some(20), 120),
            row(Some(10), 130),
        ]);
        let parsed = Codex
            .parse(&rows, "source", &ImportCheckpoint::default())
            .unwrap();
        assert_eq!(parsed.records.len(), 1);
        assert_eq!(parsed.records[0].tokens.as_ref().unwrap().total(), 10);
        assert_eq!(parsed.records[0].model.as_deref(), Some("gpt-test"));
    }
    #[test]
    fn partial_line_is_not_consumed() {
        let all = bytes(&[row(Some(10), 10)]);
        let end = all.len() - 5;
        let parsed = Codex
            .parse(&all[..end], "source", &ImportCheckpoint::default())
            .unwrap();
        assert_eq!(parsed.checkpoint.offset, 0);
        assert!(parsed.records.is_empty());
    }
    #[test]
    fn compaction_resets_and_temporary_regressions_are_distinct() {
        let parsed = Codex
            .parse(
                &bytes(&[
                    row(Some(100), 1000),
                    row(Some(10), 990),
                    row(Some(10), 1010),
                    row(Some(10), 100),
                    row(Some(10), 100),
                ]),
                "s",
                &ImportCheckpoint::default(),
            )
            .unwrap();
        assert_eq!(
            parsed
                .records
                .iter()
                .map(|r| r.tokens.as_ref().unwrap().total())
                .sum::<u64>(),
            120
        );
    }
}

#[cfg(test)]
mod headless_tests {
    use super::*;
    #[test]
    fn headless_usage_marks_file_dates_as_inferred_and_does_not_double_reasoning() {
        let bytes=b"{\"type\":\"thread.started\",\"thread_id\":\"t1\"}\n{\"type\":\"turn.completed\",\"model\":\"gpt-test\",\"usage\":{\"input_tokens\":10,\"cached_input_tokens\":3,\"output_tokens\":4,\"reasoning_output_tokens\":2}}\n";
        let parsed = Codex
            .parse(
                bytes,
                "file",
                &ImportCheckpoint {
                    offset: 0,
                    parser_state: serde_json::json!({"fallback_at_ms":1789000000000i64}),
                },
            )
            .unwrap();
        assert_eq!(parsed.records.len(), 1);
        assert!(parsed.records[0].timestamp_inferred);
        assert_eq!(parsed.records[0].tokens.as_ref().unwrap().total(), 14);
        assert_eq!(parsed.records[0].session.as_deref(), Some("t1"));
    }
}
