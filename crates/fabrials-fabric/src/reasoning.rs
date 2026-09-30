//! Provenance of opaque reasoning blobs (Anthropic `thinking` signatures and
//! Responses `encrypted_content`).
//!
//! A client that switches models mid-session replays reasoning another
//! provider produced. Anthropic, ChatGPT Codex and xAI reject a blob they did
//! not issue, so the relay remembers which issuer streamed each blob and drops
//! the ones a hop's issuer cannot verify.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Issuer {
    Anthropic,
    OpenAi,
    Xai,
}

/// How much reasoning a request keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// Drop blobs another issuer produced and blocks that carry no blob the
    /// issuer could verify; keep blobs the relay has not seen.
    Unknown,
    /// Keep only blobs this issuer produced.
    OwnOnly,
}

const CAPACITY: usize = 250_000;

type Key = [u8; 16];

#[derive(Default)]
struct Registry {
    owners: HashMap<Key, Issuer>,
    order: VecDeque<Key>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

fn key(blob: &str) -> Key {
    let digest = Sha256::digest(blob.as_bytes());
    let mut key = [0u8; 16];
    key.copy_from_slice(&digest[..16]);
    key
}

pub fn record(issuer: Issuer, blob: &str) {
    if blob.is_empty() {
        return;
    }
    let key = key(blob);
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    if registry.owners.contains_key(&key) {
        return;
    }
    registry.owners.insert(key, issuer);
    registry.order.push_back(key);
    while registry.order.len() > CAPACITY {
        if let Some(old) = registry.order.pop_front() {
            registry.owners.remove(&old);
        }
    }
}

pub fn issuer_of(blob: &str) -> Option<Issuer> {
    if blob.is_empty() {
        return None;
    }
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .owners
        .get(&key(blob))
        .copied()
}

/// Remember every blob a response streamed or returned.
pub fn observe_response(issuer: Issuer, body: &[u8]) {
    for blob in response_blobs(body) {
        record(issuer, &blob);
    }
}

/// Remember the blobs of a request the issuer accepted, so reasoning that
/// predates this process keeps being forwarded.
pub fn observe_accepted_request(issuer: Issuer, body: &[u8]) {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return;
    };
    for blob in request_blobs(&value) {
        if issuer_of(&blob).is_none() {
            record(issuer, &blob);
        }
    }
}

fn response_blobs(body: &[u8]) -> Vec<String> {
    let mut blobs = Vec::new();
    let text = String::from_utf8_lossy(body);
    let mut saw_event = false;
    for line in text.lines() {
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        if let Ok(event) = serde_json::from_str::<Value>(payload.trim_start()) {
            saw_event = true;
            event_blobs(&event, &mut blobs);
        }
    }
    if !saw_event {
        if let Ok(value) = serde_json::from_slice::<Value>(body) {
            event_blobs(&value, &mut blobs);
        }
    }
    blobs
}

fn event_blobs(event: &Value, out: &mut Vec<String>) {
    let mut push = |value: &Value| {
        if let Some(blob) = value.as_str().filter(|blob| !blob.is_empty()) {
            out.push(blob.to_string());
        }
    };
    match event["type"].as_str().unwrap_or("") {
        "content_block_delta" if event["delta"]["type"] == "signature_delta" => {
            push(&event["delta"]["signature"]);
        }
        "content_block_start" => thinking_blob(&event["content_block"], &mut push),
        "response.output_item.done" | "response.output_item.added" => {
            reasoning_blob(&event["item"], &mut push);
        }
        "response.completed" | "response.incomplete" => {
            for item in event["response"]["output"].as_array().into_iter().flatten() {
                reasoning_blob(item, &mut push);
            }
        }
        _ => {
            for block in event["content"].as_array().into_iter().flatten() {
                thinking_blob(block, &mut push);
            }
            for item in event["output"].as_array().into_iter().flatten() {
                reasoning_blob(item, &mut push);
            }
        }
    }
}

fn thinking_blob(block: &Value, push: &mut impl FnMut(&Value)) {
    match block["type"].as_str() {
        Some("thinking") => push(&block["signature"]),
        Some("redacted_thinking") => push(&block["data"]),
        _ => {}
    }
}

fn reasoning_blob(item: &Value, push: &mut impl FnMut(&Value)) {
    if item["type"] == "reasoning" {
        push(&item["encrypted_content"]);
    }
}

fn request_blobs(value: &Value) -> Vec<String> {
    let mut blobs = Vec::new();
    let mut push = |value: &Value| {
        if let Some(blob) = value.as_str().filter(|blob| !blob.is_empty()) {
            blobs.push(blob.to_string());
        }
    };
    for message in value["messages"].as_array().into_iter().flatten() {
        for block in message["content"].as_array().into_iter().flatten() {
            thinking_blob(block, &mut push);
        }
    }
    for item in value["input"].as_array().into_iter().flatten() {
        reasoning_blob(item, &mut push);
    }
    blobs
}

fn keeps(issuer: Issuer, blob: &str, keep: Keep) -> bool {
    match issuer_of(blob) {
        Some(owner) => owner == issuer,
        None => keep == Keep::Unknown && !blob.is_empty(),
    }
}

/// Drop the reasoning `issuer` cannot verify from a Messages or Responses
/// request. Returns whether anything was removed.
pub fn strip(issuer: Issuer, value: &mut Value, keep: Keep) -> bool {
    let mut changed = false;
    if let Some(messages) = value.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages.iter_mut() {
            if message["role"] != "assistant" {
                continue;
            }
            let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
                continue;
            };
            let before = content.len();
            content.retain(|block| match block["type"].as_str() {
                Some("thinking") => keeps(issuer, block["signature"].as_str().unwrap_or(""), keep),
                Some("redacted_thinking") => {
                    keeps(issuer, block["data"].as_str().unwrap_or(""), keep)
                }
                _ => true,
            });
            changed |= content.len() != before;
        }
        let before = messages.len();
        messages.retain(|message| {
            message["role"] != "assistant"
                || !message["content"].as_array().is_some_and(Vec::is_empty)
        });
        changed |= messages.len() != before;
    }
    if let Some(input) = value.get_mut("input").and_then(Value::as_array_mut) {
        let before = input.len();
        input.retain(|item| {
            if item["type"] != "reasoning" {
                return true;
            }
            let blob = item["encrypted_content"].as_str().unwrap_or("");
            if blob.is_empty() {
                return issuer == Issuer::Xai && keep == Keep::Unknown;
            }
            keeps(issuer, blob, keep)
        });
        changed |= input.len() != before;
    }
    changed
}

/// Byte form of [`strip`]; `None` when the body is unchanged or not JSON.
pub fn strip_body(issuer: Issuer, body: &[u8], keep: Keep) -> Option<Vec<u8>> {
    if !mentions_reasoning(body) {
        return None;
    }
    let mut value: Value = serde_json::from_slice(body).ok()?;
    if !strip(issuer, &mut value, keep) {
        return None;
    }
    serde_json::to_vec(&value).ok()
}

fn mentions_reasoning(body: &[u8]) -> bool {
    const NEEDLES: [&[u8]; 3] = [b"\"thinking\"", b"\"redacted_thinking\"", b"\"reasoning\""];
    NEEDLES
        .iter()
        .any(|needle| body.windows(needle.len()).any(|window| window == *needle))
}

/// A rejection that names replayed reasoning (signature, encrypted content,
/// or a reasoning item the upstream cannot resolve).
pub fn rejection_names_reasoning(status: u16, body: &[u8]) -> bool {
    if !matches!(status, 400 | 404 | 422) {
        return false;
    }
    let text = String::from_utf8_lossy(body).to_ascii_lowercase();
    [
        "signature",
        "thinking",
        "encrypted_content",
        "decrypt",
        "reasoning",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

/// One retry body after a reasoning rejection: only blobs `issuer` produced.
pub fn retry_after_rejection(
    issuer: Issuer,
    status: u16,
    rejected: &[u8],
    request: &[u8],
) -> Option<Vec<u8>> {
    if !rejection_names_reasoning(status, rejected) {
        return None;
    }
    strip_body(issuer, request, Keep::OwnOnly)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn unique(tag: &str) -> String {
        let mut bytes = [0u8; 12];
        getrandom::getrandom(&mut bytes).expect("random bytes");
        format!("{tag}-{}", hex::encode(bytes))
    }

    #[test]
    fn stream_blobs_are_recorded_per_issuer() {
        let signature = unique("sig");
        let encrypted = unique("enc");
        let claude = format!(
            "event: content_block_delta\ndata: {}\n\n",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":signature}})
        );
        observe_response(Issuer::Anthropic, claude.as_bytes());
        let codex = format!(
            "data: {}\n\n",
            json!({"type":"response.output_item.done","item":{"type":"reasoning","id":"rs_1","encrypted_content":encrypted}})
        );
        observe_response(Issuer::OpenAi, codex.as_bytes());
        assert_eq!(issuer_of(&signature), Some(Issuer::Anthropic));
        assert_eq!(issuer_of(&encrypted), Some(Issuer::OpenAi));
    }

    #[test]
    fn messages_drop_foreign_and_unsigned_thinking() {
        let own = unique("own");
        let foreign = unique("foreign");
        let unknown = unique("unknown");
        record(Issuer::Anthropic, &own);
        record(Issuer::Xai, &foreign);
        let mut request = json!({"messages":[
            {"role":"user","content":"hi"},
            {"role":"assistant","content":[{"type":"thinking","thinking":"a","signature":""},{"type":"tool_use","id":"t1","name":"read","input":{}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]},
            {"role":"assistant","content":[{"type":"thinking","thinking":"b","signature":foreign}]},
            {"role":"user","content":"again"},
            {"role":"assistant","content":[{"type":"thinking","thinking":"c","signature":own},{"type":"thinking","thinking":"d","signature":unknown},{"type":"text","text":"ok"}]}
        ]});
        assert!(strip(Issuer::Anthropic, &mut request, Keep::Unknown));
        let messages = request["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[1]["content"][0]["type"], "tool_use");
        assert_eq!(messages[4]["content"].as_array().map(Vec::len), Some(3));
        assert!(strip(Issuer::Anthropic, &mut request, Keep::OwnOnly));
        assert_eq!(
            request["messages"][4]["content"].as_array().map(Vec::len),
            Some(2)
        );
        assert_eq!(request["messages"][4]["content"][0]["signature"], own);
    }

    #[test]
    fn responses_drop_foreign_items_and_text_only_items_for_codex() {
        let own = unique("own");
        let foreign = unique("foreign");
        record(Issuer::OpenAi, &own);
        record(Issuer::Anthropic, &foreign);
        let request = json!({"input":[
            {"type":"message","role":"user","content":"hi"},
            {"type":"reasoning","id":"rs_a","summary":[{"type":"summary_text","text":"t"}]},
            {"type":"reasoning","id":"rs_b","summary":[],"encrypted_content":foreign},
            {"type":"reasoning","id":"rs_c","summary":[],"encrypted_content":own}
        ]});
        let mut codex = request.clone();
        assert!(strip(Issuer::OpenAi, &mut codex, Keep::Unknown));
        assert_eq!(codex["input"].as_array().map(Vec::len), Some(2));
        assert_eq!(codex["input"][1]["id"], "rs_c");
        let mut xai = request;
        assert!(strip(Issuer::Xai, &mut xai, Keep::Unknown));
        assert_eq!(xai["input"].as_array().map(Vec::len), Some(2));
        assert_eq!(xai["input"][1]["id"], "rs_a");
    }

    #[test]
    fn accepted_requests_teach_unknown_blobs() {
        let unknown = unique("carried");
        let body = json!({"messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"x","signature":unknown}]}]});
        observe_accepted_request(Issuer::Anthropic, body.to_string().as_bytes());
        assert_eq!(issuer_of(&unknown), Some(Issuer::Anthropic));
    }

    #[test]
    fn retry_only_after_a_reasoning_rejection() {
        let unknown = unique("stale");
        let body = json!({"messages":[{"role":"assistant","content":[{"type":"thinking","thinking":"x","signature":unknown},{"type":"text","text":"t"}]}]}).to_string();
        let rejected = br#"{"type":"error","error":{"type":"invalid_request_error","message":"messages.1.content.0: Invalid `signature` in `thinking` block"}}"#;
        let retry = retry_after_rejection(Issuer::Anthropic, 400, rejected, body.as_bytes())
            .expect("retry body");
        let value: Value = serde_json::from_slice(&retry).expect("json");
        assert_eq!(
            value["messages"][0]["content"].as_array().map(Vec::len),
            Some(1)
        );
        assert!(retry_after_rejection(Issuer::Anthropic, 429, rejected, body.as_bytes()).is_none());
        assert!(retry_after_rejection(
            Issuer::Anthropic,
            400,
            b"{\"error\":\"max_tokens\"}",
            body.as_bytes()
        )
        .is_none());
    }

    #[test]
    fn bodies_without_reasoning_are_left_alone() {
        let body = br#"{"messages":[{"role":"user","content":"hi"}]}"#;
        assert!(strip_body(Issuer::Anthropic, body, Keep::Unknown).is_none());
    }
}
