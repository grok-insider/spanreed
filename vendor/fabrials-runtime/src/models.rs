//! Rewrite Grok/xAI `GET /v1/models` so Grok Build sees `-build` siblings
//! and a reasoning-effort menu on 4.5/4.6.
//!
//! Grok Build CLI: `id` is the picker catalog key; `model` is the routing
//! slug sent to the API. `-build` is not an api.x.ai id (404 / no team
//! access), so siblings keep `id = grok-4.N-build` and `model = grok-4.N`.
//! Request bodies that still send the picker id are rewritten the same way.
//!
//! `grok-4.20-*-non-reasoning`, `grok-4.20-0309-reasoning`, and multi-agent
//! are left alone: those ids reject `reasoning.effort` or mean something else.

use serde::de::{IgnoredAny, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fmt;

pub const MAX_MODELS_REWRITE_BYTES: usize = 128 * 1024;
pub const MODELS_REWRITE_MEMORY_RESERVATION: usize = 4 * 1024 * 1024;
const MAX_MODEL_ENTRIES: usize = 2_048;

const BUILD_SIBLINGS: &[(&str, &str, &str)] = &[
    ("grok-4.6", "grok-4.6-build", "Grok 4.6 Build"),
    ("grok-4.5", "grok-4.5-build", "Grok 4.5 Build"),
];

pub fn api_slug_for_picker(id: &str) -> Option<&'static str> {
    BUILD_SIBLINGS
        .iter()
        .find(|&&(_, sibling, _)| sibling == id)
        .map(|&(donor, _, _)| donor)
}

const EFFORT_46: &[&str] = &["low", "medium", "high", "xhigh"];
const EFFORT_45: &[&str] = &["low", "medium", "high"];

/// Rewrite a Grok/xAI OpenAI-style `{ data: [...] }` list in place.
/// Returns false when `data` is missing (caller should pass the body through).
pub fn rewrite_models_list(v: &mut Value) -> bool {
    let Some(data) = v.get_mut("data").and_then(|d| d.as_array_mut()) else {
        return false;
    };
    inject_build_siblings(data);
    mark_reasoning_effort(data);
    true
}

/// Parse, rewrite, serialize. `None` if the body is not a JSON object with `data`.
pub fn rewrite_grok_models_body(body: &[u8]) -> Option<Vec<u8>> {
    // Model catalogs are normally tiny. Avoid building a many-times-larger
    // serde DOM for an unexpectedly huge upstream document; pass it through
    // unchanged instead.
    if body.len() > MAX_MODELS_REWRITE_BYTES || !catalog_shape_within_bounds(body) {
        return None;
    }
    let mut v: Value = serde_json::from_slice(body).ok()?;
    if !rewrite_models_list(&mut v) {
        return None;
    }
    serde_json::to_vec(&v).ok()
}

fn catalog_shape_within_bounds(body: &[u8]) -> bool {
    #[derive(Deserialize)]
    struct CatalogEnvelope {
        #[serde(deserialize_with = "deserialize_bounded_model_data")]
        data: (),
    }

    serde_json::from_slice::<CatalogEnvelope>(body)
        .map(|envelope| envelope.data)
        .is_ok()
}

fn deserialize_bounded_model_data<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct BoundedModelData;

    impl<'de> Visitor<'de> for BoundedModelData {
        type Value = ();

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "at most {MAX_MODEL_ENTRIES} model entries")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            for _ in 0..MAX_MODEL_ENTRIES {
                if sequence.next_element::<IgnoredAny>()?.is_none() {
                    return Ok(());
                }
            }
            if sequence.next_element::<IgnoredAny>()?.is_some() {
                return Err(serde::de::Error::custom(
                    "model catalog has too many entries",
                ));
            }
            Ok(())
        }
    }

    deserializer.deserialize_seq(BoundedModelData)
}

/// Map picker ids (`grok-4.6-build`) to the api.x.ai slug. `None` if unchanged.
pub fn rewrite_grok_request_model(body: &[u8]) -> Option<Vec<u8>> {
    let (start, end) = top_level_model_value(body)?;
    let model: String = serde_json::from_slice(&body[start..end]).ok()?;
    let slug = api_slug_for_picker(model.trim())?;
    let replacement = serde_json::to_vec(slug).ok()?;
    let mut rewritten = Vec::with_capacity(body.len() + replacement.len());
    rewritten.extend_from_slice(&body[..start]);
    rewritten.extend_from_slice(&replacement);
    rewritten.extend_from_slice(&body[end..]);
    Some(rewritten)
}

/// Locate the encoded value of a top-level `model` member without
/// materializing the rest of a potentially large request JSON document.
fn top_level_model_value(body: &[u8]) -> Option<(usize, usize)> {
    fn whitespace(body: &[u8], cursor: &mut usize) {
        while body
            .get(*cursor)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            *cursor += 1;
        }
    }

    fn string_end(body: &[u8], start: usize) -> Option<usize> {
        if body.get(start) != Some(&b'"') {
            return None;
        }
        let mut escaped = false;
        for (offset, byte) in body[start + 1..].iter().copied().enumerate() {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                return Some(start + offset + 2);
            }
        }
        None
    }

    fn skip_value(body: &[u8], cursor: &mut usize) -> Option<()> {
        whitespace(body, cursor);
        if body.get(*cursor) == Some(&b'"') {
            *cursor = string_end(body, *cursor)?;
            return Some(());
        }
        let mut nested = 0_usize;
        let mut in_string = false;
        let mut escaped = false;
        while let Some(byte) = body.get(*cursor).copied() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
            } else {
                match byte {
                    b'"' => in_string = true,
                    b'[' | b'{' => nested = nested.checked_add(1)?,
                    b']' => nested = nested.checked_sub(1)?,
                    b'}' if nested == 0 => return Some(()),
                    b'}' => nested = nested.checked_sub(1)?,
                    b',' if nested == 0 => return Some(()),
                    _ => {}
                }
            }
            *cursor += 1;
        }
        Some(())
    }

    let mut cursor = 0;
    whitespace(body, &mut cursor);
    if body.get(cursor) != Some(&b'{') {
        return None;
    }
    cursor += 1;
    loop {
        whitespace(body, &mut cursor);
        if body.get(cursor) == Some(&b'}') {
            return None;
        }
        let key_start = cursor;
        let key_end = string_end(body, key_start)?;
        let key: String = serde_json::from_slice(&body[key_start..key_end]).ok()?;
        cursor = key_end;
        whitespace(body, &mut cursor);
        if body.get(cursor) != Some(&b':') {
            return None;
        }
        cursor += 1;
        whitespace(body, &mut cursor);
        if key == "model" {
            let end = string_end(body, cursor)?;
            return Some((cursor, end));
        }
        skip_value(body, &mut cursor)?;
        whitespace(body, &mut cursor);
        match body.get(cursor) {
            Some(b',') => cursor += 1,
            Some(b'}') => return None,
            _ => return None,
        }
    }
}

fn entry_id(item: &Value) -> Option<&str> {
    item.get("id")
        .or_else(|| item.get("model"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn inject_build_siblings(data: &mut Vec<Value>) {
    let mut existing: HashSet<String> = data
        .iter()
        .filter_map(|item| entry_id(item).map(str::to_string))
        .collect();
    let mut inserts: Vec<(usize, Value)> = Vec::new();
    for (i, item) in data.iter().enumerate() {
        let Some(id) = entry_id(item) else {
            continue;
        };
        for &(donor, sibling, name) in BUILD_SIBLINGS {
            if id != donor || !existing.insert(sibling.to_string()) {
                continue;
            }
            let mut clone = item.clone();
            if let Some(obj) = clone.as_object_mut() {
                obj.insert("id".into(), json!(sibling));
                obj.insert("model".into(), json!(donor));
                obj.insert("name".into(), json!(name));
            }
            inserts.push((i + 1, clone));
        }
    }
    for (offset, (i, v)) in inserts.into_iter().enumerate() {
        data.insert(i + offset, v);
    }
}

fn mark_reasoning_effort(data: &mut [Value]) {
    for item in data {
        let Some(id) = entry_id(item) else {
            continue;
        };
        let efforts = match id {
            "grok-4.6" | "grok-4.6-build" => EFFORT_46,
            "grok-4.5" | "grok-4.5-build" => EFFORT_45,
            _ => continue,
        };
        if let Some(obj) = item.as_object_mut() {
            obj.insert("supportsReasoningEffort".into(), json!(true));
            obj.insert("reasoningEfforts".into(), json!(efforts));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(v: &Value) -> Vec<String> {
        v["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| entry_id(item).map(str::to_string))
            .collect()
    }

    fn entry<'a>(v: &'a Value, id: &str) -> &'a Value {
        v["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| entry_id(item) == Some(id))
            .unwrap_or_else(|| panic!("missing {id}"))
    }

    fn rewrite(v: Value) -> Value {
        let mut v = v;
        assert!(rewrite_models_list(&mut v));
        v
    }

    #[test]
    fn injects_build_sibling_next_to_donor() {
        let out = rewrite(json!({
            "object": "list",
            "data": [
                {"id": "grok-4.6", "name": "Grok 4.6", "object": "model"},
                {"id": "grok-imagine-image"},
            ]
        }));
        assert_eq!(
            ids(&out),
            vec!["grok-4.6", "grok-4.6-build", "grok-imagine-image"]
        );
        let build = entry(&out, "grok-4.6-build");
        assert_eq!(build["name"], "Grok 4.6 Build");
        assert_eq!(build["object"], "model");
        assert_eq!(build["model"], "grok-4.6");
        assert_eq!(entry(&out, "grok-4.6").get("model"), None);
    }

    #[test]
    fn does_not_duplicate_existing_build() {
        let out = rewrite(json!({
            "data": [
                {"id": "grok-4.6"},
                {"id": "grok-4.6-build", "name": "already"},
            ]
        }));
        assert_eq!(ids(&out), vec!["grok-4.6", "grok-4.6-build"]);
        assert_eq!(entry(&out, "grok-4.6-build")["name"], "already");
    }

    #[test]
    fn donor_absent_does_not_invent_build() {
        let out = rewrite(json!({
            "data": [
                {"id": "grok-4.20-0309-non-reasoning"},
                {"id": "grok-imagine-image"},
            ]
        }));
        assert_eq!(
            ids(&out),
            vec!["grok-4.20-0309-non-reasoning", "grok-imagine-image"]
        );
    }

    #[test]
    fn flags_46_and_build_include_xhigh() {
        let out = rewrite(json!({
            "data": [{"id": "grok-4.6"}]
        }));
        for id in ["grok-4.6", "grok-4.6-build"] {
            let e = entry(&out, id);
            assert_eq!(e["supportsReasoningEffort"], true, "{id}");
            assert_eq!(
                e["reasoningEfforts"],
                json!(["low", "medium", "high", "xhigh"]),
                "{id}"
            );
        }
    }

    #[test]
    fn flags_45_without_xhigh() {
        let out = rewrite(json!({
            "data": [{"id": "grok-4.5", "model": "grok-4.5"}]
        }));
        assert_eq!(ids(&out), vec!["grok-4.5", "grok-4.5-build"]);
        assert_eq!(entry(&out, "grok-4.5-build")["model"], "grok-4.5");
        assert_eq!(entry(&out, "grok-4.5")["model"], "grok-4.5");
        assert_eq!(entry(&out, "grok-4.5-build")["name"], "Grok 4.5 Build");
        for id in ["grok-4.5", "grok-4.5-build"] {
            let e = entry(&out, id);
            assert_eq!(e["supportsReasoningEffort"], true, "{id}");
            assert_eq!(
                e["reasoningEfforts"],
                json!(["low", "medium", "high"]),
                "{id}"
            );
        }
    }

    #[test]
    fn leaves_non_reasoning_and_4_20_reasoning_and_multi_agent_intact() {
        let out = rewrite(json!({
            "data": [
                {"id": "grok-4.20-0309-non-reasoning", "name": "NR"},
                {"id": "grok-4.20-0309-reasoning"},
                {"id": "grok-4.20-multi-agent-0309", "supportsReasoningEffort": false},
            ]
        }));
        let nr = entry(&out, "grok-4.20-0309-non-reasoning");
        assert!(nr.get("supportsReasoningEffort").is_none());
        assert!(nr.get("reasoningEfforts").is_none());
        assert_eq!(nr["name"], "NR");
        let r = entry(&out, "grok-4.20-0309-reasoning");
        assert!(r.get("supportsReasoningEffort").is_none());
        assert!(r.get("reasoningEfforts").is_none());
        let ma = entry(&out, "grok-4.20-multi-agent-0309");
        assert_eq!(ma["supportsReasoningEffort"], false);
        assert!(ma.get("reasoningEfforts").is_none());
    }

    #[test]
    fn does_not_filter_imagine() {
        let out = rewrite(json!({
            "data": [
                {"id": "grok-4.6"},
                {"id": "grok-imagine-image"},
                {"id": "grok-imagine-video"},
            ]
        }));
        assert!(ids(&out).contains(&"grok-imagine-image".into()));
        assert!(ids(&out).contains(&"grok-imagine-video".into()));
    }

    #[test]
    fn invalid_json_and_missing_data_are_none() {
        assert!(rewrite_grok_models_body(b"not json").is_none());
        assert!(rewrite_grok_models_body(b"[]").is_none());
        assert!(rewrite_grok_models_body(br#"{"object":"list"}"#).is_none());
        assert!(rewrite_grok_models_body(br#"{"data":[]}"#).is_some());
    }

    #[test]
    fn request_rewrites_build_picker_id_to_api_slug() {
        let out: Value = serde_json::from_slice(
            &rewrite_grok_request_model(br#"{"model":"grok-4.6-build","stream":true}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(out["model"], "grok-4.6");
        assert_eq!(out["stream"], true);
        let out: Value = serde_json::from_slice(
            &rewrite_grok_request_model(br#"{"model":" grok-4.5-build "}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(out["model"], "grok-4.5");
        assert!(rewrite_grok_request_model(br#"{"model":"grok-4.6"}"#).is_none());
        assert!(
            rewrite_grok_request_model(br#"{"model":"grok-4.20-0309-non-reasoning"}"#).is_none()
        );
        assert!(rewrite_grok_request_model(b"not json").is_none());
    }

    #[test]
    fn request_rewrite_preserves_large_unknown_fields_byte_for_byte() {
        let padding = "x".repeat(1024 * 1024);
        let body = format!(
            "{{\"input\":{{\"quoted\":\"model: \\\"untouched\\\"\",\"padding\":\"{padding}\"}},\"model\": \"grok-4.6-build\",\"stream\":true}}"
        );
        let rewritten = rewrite_grok_request_model(body.as_bytes()).expect("build rewrite");
        let rewritten = String::from_utf8(rewritten).unwrap();
        assert!(rewritten.contains(&format!("\"padding\":\"{padding}\"")));
        assert!(rewritten.contains("\"quoted\":\"model: \\\"untouched\\\"\""));
        assert!(rewritten.contains("\"model\": \"grok-4.6\""));
        assert_eq!(rewritten.len(), body.len() - "-build".len());
    }

    #[test]
    fn oversized_model_catalog_is_not_materialized_for_rewrite() {
        let body = vec![b' '; MAX_MODELS_REWRITE_BYTES + 1];
        assert!(rewrite_grok_models_body(&body).is_none());
    }

    #[test]
    fn many_tiny_model_entries_are_rejected_before_dom_materialization() {
        let mut body = String::from("{\"data\":[");
        for index in 0..=MAX_MODEL_ENTRIES {
            if index > 0 {
                body.push(',');
            }
            body.push_str("{}");
        }
        body.push_str("]}");
        assert!(body.len() < MAX_MODELS_REWRITE_BYTES);
        assert!(rewrite_grok_models_body(body.as_bytes()).is_none());
    }
}
