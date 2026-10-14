//! First-byte and first-content timing of a streamed upstream response.
//!
//! The relay forwards an upstream response while it reads it, so the instant a read
//! returns is the instant the relay first had those bytes. [`StreamProbe`] stamps the first
//! read that returned a body byte (`first_byte`) and the first read that completed an SSE
//! event carrying content (`first_content`, the TTFT).
//!
//! Content is decided from the protocol shape of the event, never from a provider name:
//! Responses `response.*.delta`, Anthropic Messages `content_block_delta` and Chat
//! Completions `choices[].delta`. The shapes were checked against real streams of every
//! provider the relay proxies (see `docs/streaming-event-shapes.md` in ai-relay).
//!
//! The scan runs inside the read loop because the response tail the relay keeps for usage
//! parsing is a rolling window that can evict the first events of a long stream. It stops at
//! the first content event, is bounded in memory and work, and gives up quietly (leaving the
//! TTFT unset) on anything it cannot read. It never logs or keeps response text.

use fabrials_types::hop::HopKind;
use serde_json::Value;
use std::fmt;
use std::time::Instant;

/// One SSE line longer than this is skipped (its event is dropped).
const MAX_LINE_BYTES: usize = 1024 * 1024;
/// Accumulated `data:` payload of one event; a larger event is dropped.
const MAX_EVENT_BYTES: usize = 1024 * 1024;
/// Total bytes scanned before giving up: a stream that has not produced content by now is
/// not going to produce a meaningful first token.
const MAX_SCAN_BYTES: usize = 8 * 1024 * 1024;
/// Parsed events before giving up.
const MAX_EVENTS: usize = 4096;

/// Whole milliseconds from `origin` to `at`, saturating at zero.
pub fn elapsed_ms(origin: Instant, at: Instant) -> u64 {
    u64::try_from(at.saturating_duration_since(origin).as_millis()).unwrap_or(u64::MAX)
}

/// Whether one parsed SSE `data:` payload carries content: a non-empty text, reasoning or
/// tool-call argument delta. Role-only chunks, empty deltas, usage-only chunks, lifecycle
/// events (`response.created`, `message_start`, `ping`) and signatures do not.
pub fn event_has_content(event: &Value) -> bool {
    let Some(object) = event.as_object() else {
        return false;
    };
    match object.get("type").and_then(Value::as_str) {
        // Responses: every `response.*.delta` is model output (`output_text`, `reasoning_text`,
        // `reasoning_summary_text`, `function_call_arguments`, `refusal`, ...).
        Some(kind) if kind.starts_with("response.") && kind.ends_with(".delta") => {
            non_empty(object.get("delta"))
        }
        Some("content_block_delta") => anthropic_delta_has_content(object.get("delta")),
        _ => chat_chunk_has_content(object),
    }
}

fn non_empty(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(|s| !s.is_empty())
}

fn anthropic_delta_has_content(delta: Option<&Value>) -> bool {
    let Some(delta) = delta.and_then(Value::as_object) else {
        return false;
    };
    // `signature_delta` and `citations_delta` are bookkeeping, not output.
    let field = match delta.get("type").and_then(Value::as_str) {
        Some("text_delta") => "text",
        Some("thinking_delta") => "thinking",
        Some("input_json_delta") => "partial_json",
        _ => return false,
    };
    non_empty(delta.get(field))
}

fn chat_chunk_has_content(object: &serde_json::Map<String, Value>) -> bool {
    let Some(choices) = object.get("choices").and_then(Value::as_array) else {
        return false;
    };
    choices.iter().any(|choice| {
        choice
            .get("delta")
            .and_then(Value::as_object)
            .is_some_and(chat_delta_has_content)
    })
}

fn chat_delta_has_content(delta: &serde_json::Map<String, Value>) -> bool {
    ["content", "reasoning_content", "reasoning", "refusal"]
        .iter()
        .any(|key| non_empty(delta.get(*key)))
        || delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| {
                calls
                    .iter()
                    .any(|call| non_empty(call.get("function").and_then(|f| f.get("arguments"))))
            })
}

/// Incremental SSE reader. Feed it response bytes as they arrive; it reports whether an
/// event with content has completed.
struct Scanner {
    line: Vec<u8>,
    /// The current line exceeded `MAX_LINE_BYTES`: drop bytes up to its newline.
    skipping: bool,
    event: Option<String>,
    data: Vec<u8>,
    has_data: bool,
    /// The current event's data exceeded `MAX_EVENT_BYTES`.
    oversized: bool,
    scanned: usize,
    events: usize,
    first_line: bool,
    gave_up: bool,
}

impl Scanner {
    fn new() -> Self {
        Self {
            line: Vec::new(),
            skipping: false,
            event: None,
            data: Vec::new(),
            has_data: false,
            oversized: false,
            scanned: 0,
            events: 0,
            first_line: true,
            gave_up: false,
        }
    }

    /// True when an event carrying content completed inside `chunk`.
    fn feed(&mut self, chunk: &[u8]) -> bool {
        self.scanned = self.scanned.saturating_add(chunk.len());
        if self.scanned > MAX_SCAN_BYTES {
            self.gave_up = true;
            return false;
        }
        for &byte in chunk {
            if byte == b'\n' {
                if self.skipping {
                    self.skipping = false;
                    self.line.clear();
                    continue;
                }
                let line = std::mem::take(&mut self.line);
                if self.line_done(&line) {
                    return true;
                }
                if self.gave_up {
                    return false;
                }
            } else if !self.skipping {
                if self.line.len() >= MAX_LINE_BYTES {
                    self.skipping = true;
                    self.oversized = true;
                    self.line.clear();
                } else {
                    self.line.push(byte);
                }
            }
        }
        false
    }

    fn line_done(&mut self, raw: &[u8]) -> bool {
        let mut line = raw;
        if self.first_line {
            self.first_line = false;
            line = line.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(line);
        }
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            return self.dispatch();
        }
        if line[0] == b':' {
            return false;
        }
        let (field, value) = match line.iter().position(|b| *b == b':') {
            Some(at) => {
                let value = &line[at + 1..];
                (&line[..at], value.strip_prefix(b" ").unwrap_or(value))
            }
            None => (line, &line[line.len()..]),
        };
        match field {
            b"event" => self.event = Some(String::from_utf8_lossy(value).into_owned()),
            b"data" => {
                if self.data.len().saturating_add(value.len() + 1) > MAX_EVENT_BYTES {
                    self.oversized = true;
                } else {
                    if self.has_data {
                        self.data.push(b'\n');
                    }
                    self.data.extend_from_slice(value);
                }
                self.has_data = true;
            }
            _ => {}
        }
        false
    }

    fn dispatch(&mut self) -> bool {
        let event = self.event.take();
        let data = std::mem::take(&mut self.data);
        let has_data = std::mem::take(&mut self.has_data);
        let oversized = std::mem::take(&mut self.oversized);
        if !has_data || oversized {
            return false;
        }
        self.events += 1;
        if self.events > MAX_EVENTS {
            self.gave_up = true;
            return false;
        }
        let Ok(mut payload) = serde_json::from_slice::<Value>(&data) else {
            return false;
        };
        // A stream may name its events with `event:` and leave `type` out of the payload.
        if let (Some(name), Value::Object(map)) = (event, &mut payload) {
            map.entry("type").or_insert(Value::String(name));
        }
        event_has_content(&payload)
    }

    #[cfg(test)]
    fn buffered_bytes(&self) -> usize {
        self.line.len() + self.data.len()
    }
}

/// Timings of one upstream response body, observed read by read.
pub struct StreamProbe {
    first_byte: Option<Instant>,
    first_content: Option<Instant>,
    scanner: Option<Scanner>,
}

impl StreamProbe {
    /// Stamp only the first body byte. For bodies that are not an event stream.
    pub fn first_byte_only() -> Self {
        Self {
            first_byte: None,
            first_content: None,
            scanner: None,
        }
    }

    /// Stamp the first body byte and the first event with content.
    pub fn sse_content() -> Self {
        Self {
            first_byte: None,
            first_content: None,
            scanner: Some(Scanner::new()),
        }
    }

    /// Pick the probe for a response: event content is looked for only in a successful
    /// chat response that is an uncompressed event stream.
    pub fn for_response(
        kind: HopKind,
        status: u16,
        content_type: Option<&str>,
        content_encoding: Option<&str>,
    ) -> Self {
        let event_stream =
            content_type.is_some_and(|value| value.to_ascii_lowercase().contains("event-stream"));
        let plain = content_encoding.is_none_or(|value| {
            let value = value.trim();
            value.is_empty() || value.eq_ignore_ascii_case("identity")
        });
        if kind == HopKind::Chat && (200..300).contains(&status) && event_stream && plain {
            Self::sse_content()
        } else {
            Self::first_byte_only()
        }
    }

    /// Record one read of the response body. `read_at` is when the read returned, taken
    /// before anything was forwarded so a slow client cannot inflate the timing.
    pub fn observe_chunk(&mut self, chunk: &[u8], read_at: Instant) {
        if chunk.is_empty() {
            return;
        }
        if self.first_byte.is_none() {
            self.first_byte = Some(read_at);
        }
        let Some(scanner) = self.scanner.as_mut() else {
            return;
        };
        if scanner.feed(chunk) {
            self.first_content = Some(read_at);
            self.scanner = None;
        } else if scanner.gave_up {
            self.scanner = None;
        }
    }

    /// Whether the probe can still learn something from more bytes.
    pub fn is_scanning(&self) -> bool {
        self.scanner.is_some()
    }

    pub fn first_byte_ms(&self, origin: Instant) -> Option<u64> {
        self.first_byte.map(|at| elapsed_ms(origin, at))
    }

    pub fn ttft_ms(&self, origin: Instant) -> Option<u64> {
        self.first_content.map(|at| elapsed_ms(origin, at))
    }
}

/// Flags and durations only: the scanner buffers response text and must never reach a log.
impl fmt::Debug for StreamProbe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamProbe")
            .field("first_byte", &self.first_byte.is_some())
            .field("first_content", &self.first_content.is_some())
            .field("scanning", &self.scanner.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    fn content(value: Value) -> bool {
        event_has_content(&value)
    }

    #[test]
    fn responses_deltas_count_when_they_carry_text() {
        for kind in [
            "response.output_text.delta",
            "response.reasoning_text.delta",
            "response.reasoning_summary_text.delta",
            "response.function_call_arguments.delta",
            "response.refusal.delta",
        ] {
            assert!(content(json!({"type": kind, "delta": "x"})), "{kind}");
            assert!(!content(json!({"type": kind, "delta": ""})), "{kind} empty");
        }
    }

    #[test]
    fn responses_lifecycle_and_done_events_do_not_count() {
        for kind in [
            "response.created",
            "response.in_progress",
            "response.output_item.added",
            "response.content_part.added",
            "response.output_text.done",
            "response.function_call_arguments.done",
            "response.completed",
        ] {
            assert!(
                !content(json!({"type": kind, "delta": "x", "text": "x"})),
                "{kind}"
            );
        }
        assert!(!content(json!({"type": "response.output_text.delta"})));
        assert!(!content(
            json!({"type": "response.output_text.delta", "delta": null})
        ));
        assert!(!content(
            json!({"type": "response.output_text.delta", "delta": {"a": 1}})
        ));
    }

    #[test]
    fn anthropic_deltas_count_when_they_carry_output() {
        let delta = |kind: &str, field: &str, value: &str| json!({"type": "content_block_delta", "delta": {"type": kind, field: value}});
        assert!(content(delta("text_delta", "text", "a")));
        assert!(content(delta("thinking_delta", "thinking", "a")));
        assert!(content(delta("input_json_delta", "partial_json", "{")));
        // The first partial_json of a tool call is routinely empty.
        assert!(!content(delta("input_json_delta", "partial_json", "")));
        assert!(!content(delta("text_delta", "text", "")));
        assert!(!content(delta("signature_delta", "signature", "abc")));
        assert!(!content(delta("citations_delta", "citation", "abc")));
        // A field of the wrong delta type is not content.
        assert!(!content(delta("text_delta", "thinking", "a")));
    }

    #[test]
    fn anthropic_lifecycle_events_do_not_count() {
        for kind in [
            "message_start",
            "content_block_start",
            "ping",
            "content_block_stop",
            "message_delta",
            "message_stop",
        ] {
            assert!(
                !content(json!({"type": kind, "delta": {"type": "text_delta", "text": "a"}})),
                "{kind}"
            );
        }
    }

    #[test]
    fn chat_chunks_count_when_a_delta_carries_output() {
        let chunk = |delta: Value| json!({"choices": [{"index": 0, "delta": delta}]});
        assert!(content(chunk(json!({"content": "a"}))));
        assert!(content(chunk(json!({"reasoning_content": "a"}))));
        assert!(content(chunk(json!({"reasoning": "a"}))));
        assert!(content(chunk(json!({"refusal": "a"}))));
        assert!(content(chunk(
            json!({"tool_calls": [{"index": 0, "function": {"arguments": "{"}}]})
        )));
        // Role-only and empty deltas are how real providers open a stream.
        assert!(!content(chunk(json!({"role": "assistant"}))));
        assert!(!content(chunk(json!({"role": "assistant", "content": ""}))));
        assert!(!content(chunk(json!({"content": null}))));
        assert!(!content(chunk(json!({}))));
        // A tool call announces its name before any arguments.
        assert!(!content(chunk(
            json!({"tool_calls": [{"index": 0, "function": {"name": "f", "arguments": ""}}]})
        )));
        // Usage-only and finish chunks.
        assert!(!content(
            json!({"choices": [], "usage": {"completion_tokens": 3}})
        ));
        assert!(!content(
            json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})
        ));
        // A second choice carrying output counts.
        assert!(content(
            json!({"choices": [{"delta": {}}, {"delta": {"content": "a"}}]})
        ));
    }

    #[test]
    fn payloads_that_are_not_objects_do_not_count() {
        for value in [json!("a"), json!(["a"]), json!(1), json!(null)] {
            assert!(!content(value));
        }
    }

    // ---- scanner over bytes ------------------------------------------------

    fn scan(bytes: &[u8]) -> bool {
        Scanner::new().feed(bytes)
    }

    const CHAT_STREAM: &[u8] =
        b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n\
data: [DONE]\n\n";

    #[test]
    fn scanner_skips_role_only_chunks_and_finds_the_first_text() {
        assert!(scan(CHAT_STREAM));
        let role_only =
            b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\ndata: [DONE]\n\n";
        assert!(!scan(role_only));
    }

    #[test]
    fn scanner_handles_event_names_comments_crlf_and_multiline_data() {
        let stream = b"\xEF\xBB\xBF: keepalive\r\n\r\n\
event: response.created\r\ndata: {\"type\":\"response.created\"}\r\n\r\n\
: ping\n\n\
event: response.output_text.delta\n\
data: {\"type\":\"response.output_text.delta\",\n\
data: \"delta\":\"hi\"}\n\n";
        assert!(scan(stream));
    }

    #[test]
    fn scanner_uses_the_event_name_when_the_payload_has_no_type() {
        assert!(scan(
            b"event: response.output_text.delta\ndata: {\"delta\":\"hi\"}\n\n"
        ));
        assert!(!scan(
            b"event: response.created\ndata: {\"delta\":\"hi\"}\n\n"
        ));
        // A payload `type` wins over the event name.
        assert!(!scan(
            b"event: response.output_text.delta\ndata: {\"type\":\"response.created\",\"delta\":\"hi\"}\n\n"
        ));
    }

    #[test]
    fn scanner_ignores_done_and_unparsable_payloads() {
        assert!(!scan(
            b"data: [DONE]\n\ndata: not json\n\ndata:\n\nevent: x\n\n"
        ));
    }

    #[test]
    fn scanner_waits_for_the_blank_line_that_completes_an_event() {
        let event = b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n";
        assert!(!scan(event));
        assert!(scan(&[event.as_slice(), b"\n"].concat()));
    }

    #[test]
    fn scanner_result_does_not_depend_on_where_the_chunks_split() {
        let stream: Vec<u8> = [
            b": hi\n\n".as_slice(),
            b"event: a\r\ndata: {\"type\":\"message_start\"}\r\n\r\n",
            CHAT_STREAM,
        ]
        .concat();
        let end_of_content = stream
            .windows(2)
            .enumerate()
            .filter(|(_, pair)| *pair == b"\n\n")
            .map(|(at, _)| at + 2)
            .find(|end| scan(&stream[..*end]))
            .expect("the content event completes");
        for split in 0..=stream.len() {
            let mut scanner = Scanner::new();
            let first = scanner.feed(&stream[..split]);
            let second = scanner.feed(&stream[split..]);
            assert_eq!(first, split >= end_of_content, "split {split}");
            assert!(first || second, "split {split} lost the content event");
        }
        let mut bytewise = Scanner::new();
        let found_at = stream
            .iter()
            .position(|byte| bytewise.feed(std::slice::from_ref(byte)))
            .map(|at| at + 1);
        assert_eq!(found_at, Some(end_of_content));
    }

    #[test]
    fn an_overlong_line_is_skipped_without_growing_the_buffer() {
        let mut scanner = Scanner::new();
        let huge = vec![b'a'; MAX_LINE_BYTES * 3];
        assert!(!scanner.feed(b"data: "));
        assert!(!scanner.feed(&huge));
        assert!(
            scanner.buffered_bytes() <= MAX_LINE_BYTES,
            "{}",
            scanner.buffered_bytes()
        );
        // The newline ends the skipped line; the event it belonged to is dropped.
        assert!(!scanner.feed(b"\n\n"));
        // Later events still work.
        assert!(scanner.feed(CHAT_STREAM));
    }

    #[test]
    fn an_oversized_event_is_dropped_but_the_next_one_is_read() {
        let mut scanner = Scanner::new();
        let line = format!("data: {}\n", "a".repeat(MAX_EVENT_BYTES / 2 + 1));
        for _ in 0..3 {
            assert!(!scanner.feed(line.as_bytes()));
        }
        assert!(scanner.buffered_bytes() <= MAX_EVENT_BYTES + MAX_LINE_BYTES);
        assert!(!scanner.feed(b"\n"));
        assert!(scanner.feed(CHAT_STREAM));
    }

    #[test]
    fn the_scanner_gives_up_after_the_scan_cap_and_after_too_many_events() {
        let mut scanner = Scanner::new();
        let filler = vec![b' '; 64 * 1024];
        let mut fed = 0;
        while !scanner.gave_up {
            assert!(!scanner.feed(&filler));
            fed += filler.len();
            assert!(fed <= MAX_SCAN_BYTES + filler.len());
        }
        assert!(
            !scanner.feed(CHAT_STREAM),
            "scan cap reached: content ignored"
        );

        let mut scanner = Scanner::new();
        let ping = b"data: {\"type\":\"ping\"}\n\n".repeat(MAX_EVENTS + 1);
        assert!(!scanner.feed(&ping));
        assert!(scanner.gave_up);
    }

    // ---- probe -------------------------------------------------------------

    fn at(origin: Instant, ms: u64) -> Instant {
        origin + Duration::from_millis(ms)
    }

    #[test]
    fn the_probe_stamps_the_first_byte_and_the_read_that_completed_the_content_event() {
        let origin = Instant::now();
        let mut probe = StreamProbe::sse_content();
        assert_eq!(
            (probe.first_byte_ms(origin), probe.ttft_ms(origin)),
            (None, None)
        );
        probe.observe_chunk(b"", at(origin, 10));
        assert_eq!(
            probe.first_byte_ms(origin),
            None,
            "an empty read is not a byte"
        );
        probe.observe_chunk(
            b"event: response.created\ndata: {\"type\":\"response.created\"}\n\n",
            at(origin, 80),
        );
        assert_eq!(probe.first_byte_ms(origin), Some(80));
        assert_eq!(probe.ttft_ms(origin), None);
        probe.observe_chunk(b": ping\n\n", at(origin, 500));
        probe.observe_chunk(
            b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n",
            at(origin, 900),
        );
        assert_eq!(probe.first_byte_ms(origin), Some(80));
        assert_eq!(probe.ttft_ms(origin), Some(900));
        assert!(
            !probe.is_scanning(),
            "the scan stops at the first content event"
        );
        probe.observe_chunk(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n",
            at(origin, 2_000),
        );
        assert_eq!(
            probe.ttft_ms(origin),
            Some(900),
            "the first content event stays the first"
        );
    }

    #[test]
    fn the_first_read_can_hold_both_the_first_byte_and_the_content() {
        let origin = Instant::now();
        let mut probe = StreamProbe::sse_content();
        probe.observe_chunk(CHAT_STREAM, at(origin, 640));
        assert_eq!(probe.first_byte_ms(origin), Some(640));
        assert_eq!(probe.ttft_ms(origin), Some(640));
    }

    #[test]
    fn first_byte_only_never_reports_a_ttft() {
        let origin = Instant::now();
        let mut probe = StreamProbe::first_byte_only();
        probe.observe_chunk(CHAT_STREAM, at(origin, 30));
        assert_eq!(probe.first_byte_ms(origin), Some(30));
        assert_eq!(probe.ttft_ms(origin), None);
    }

    #[test]
    fn a_json_body_has_a_first_byte_and_no_ttft() {
        let origin = Instant::now();
        let mut probe = StreamProbe::sse_content();
        probe.observe_chunk(
            b"{\"choices\":[{\"message\":{\"content\":\"hi\"}}]}",
            at(origin, 700),
        );
        assert_eq!(probe.first_byte_ms(origin), Some(700));
        assert_eq!(probe.ttft_ms(origin), None);
    }

    #[test]
    fn an_origin_after_the_read_saturates_to_zero() {
        let origin = Instant::now();
        let mut probe = StreamProbe::first_byte_only();
        probe.observe_chunk(b"x", origin);
        assert_eq!(
            probe.first_byte_ms(origin + Duration::from_millis(5)),
            Some(0)
        );
    }

    #[test]
    fn for_response_scans_only_successful_uncompressed_chat_event_streams() {
        let scans = |kind, status, content_type, encoding| {
            StreamProbe::for_response(kind, status, content_type, encoding).is_scanning()
        };
        let sse = Some("text/event-stream");
        assert!(scans(HopKind::Chat, 200, sse, None));
        assert!(scans(
            HopKind::Chat,
            200,
            Some("Text/Event-Stream; charset=utf-8"),
            None
        ));
        assert!(scans(HopKind::Chat, 200, sse, Some("identity")));
        assert!(scans(HopKind::Chat, 200, sse, Some("")));
        assert!(!scans(HopKind::Chat, 200, sse, Some("gzip")));
        assert!(!scans(HopKind::Chat, 200, Some("application/json"), None));
        assert!(!scans(HopKind::Chat, 200, None, None));
        assert!(!scans(HopKind::Chat, 429, sse, None));
        assert!(!scans(HopKind::Chat, 500, sse, None));
        assert!(!scans(HopKind::Tts, 200, sse, None));
        assert!(!scans(HopKind::Models, 200, sse, None));
    }

    #[test]
    fn debug_never_shows_response_text() {
        let origin = Instant::now();
        let mut probe = StreamProbe::sse_content();
        probe.observe_chunk(
            b"data: {\"choices\":[{\"delta\":{\"role\":\"TOPSECRETTEXT",
            at(origin, 1),
        );
        let text = format!("{probe:?}");
        assert!(!text.contains("TOPSECRET"), "{text}");
        assert!(text.contains("first_byte"), "{text}");
    }
}
