//! The one Responses <-> Chat Completions translator.
//!
//! Requests: a chat hop whose body is still a Responses request is translated
//! ([`adapt_upstream_body`]), and a Chat Completions request can be turned into
//! a Responses request ([`chat_request_to_responses`]) for Responses-only
//! upstreams. Replies: a final Responses object becomes a Chat Completions
//! reply ([`responses_to_chat_response`]). A 400 from strict `json_schema` can
//! be retried once as JSON mode, and an opaque rejection is rewritten so the
//! client can show its message.

use serde_json::{json, Map, Value};

const SCHEMA_LIMIT: usize = 16 * 1024;
const ERROR_LIMIT: usize = 240;

/// `true` when `path` is the chat completions route, ignoring a query string.
pub fn is_chat_completions(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path).trim_end_matches('/');
    path == "/v1/chat/completions"
}

/// Translate a Responses body that landed on chat completions.
///
/// Any other path, a body that is already chat, and a body that is not JSON
/// are returned unchanged.
pub fn adapt_upstream_body(path: &str, body: &[u8]) -> Vec<u8> {
    if !is_chat_completions(path) {
        return body.to_vec();
    }
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    let Some(obj) = value.as_object() else {
        return body.to_vec();
    };
    if obj.contains_key("messages") || !obj.contains_key("input") {
        return body.to_vec();
    }
    let Some(messages) = input_to_messages(obj.get("input")) else {
        return body.to_vec();
    };
    serde_json::to_vec(&responses_to_chat(obj, messages)).unwrap_or_else(|_| body.to_vec())
}

/// One downgraded body when `request` asks for strict `json_schema` and the
/// rejection is a 400 that is not auth, quota, or context length.
pub fn downgrade_after_rejection(
    status: u16,
    error_body: &[u8],
    request: &[u8],
) -> Option<Vec<u8>> {
    if status != 400 || structured_retry_blocked(error_body) {
        return None;
    }
    downgrade_structured_output(request)
}

/// Replace `json_schema` with JSON mode and name the schema in the prompt.
pub fn downgrade_structured_output(request: &[u8]) -> Option<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(request).ok()?;
    let obj = value.as_object_mut()?;
    if let Some(schema) = chat_json_schema(obj) {
        obj.insert(
            "response_format".into(),
            serde_json::json!({"type": "json_object"}),
        );
        inject_chat_system(obj, &schema_note(&schema));
        return serde_json::to_vec(&value).ok();
    }
    if let Some(schema) = messages_json_schema(obj) {
        strip_messages_format(obj);
        inject_messages_system(obj, &schema_note(&schema));
        return serde_json::to_vec(&value).ok();
    }
    None
}

/// A rejection the client can display: `{"error":"..."}` or
/// `{"error":{"message":"..."}}`. Anything else becomes that shape.
pub fn normalize_rejection(body: &[u8]) -> Vec<u8> {
    if client_readable_error(body) {
        return body.to_vec();
    }
    let message = rejection_excerpt(body);
    serde_json::to_vec(&serde_json::json!({
        "error": {"type": "upstream_error", "message": message}
    }))
    .unwrap_or_else(|_| body.to_vec())
}

/// When the body was rewritten, advertise JSON instead of the upstream type.
pub fn json_error_headers(
    mut headers: reqwest::header::HeaderMap,
    original: &[u8],
    normalized: &[u8],
) -> reqwest::header::HeaderMap {
    if original != normalized {
        headers.remove(reqwest::header::CONTENT_TYPE);
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
    }
    headers
}

fn structured_retry_blocked(error_body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(error_body).to_ascii_lowercase();
    const BLOCKED: &[&str] = &[
        "context",
        "too long",
        "context_length",
        "maximum context",
        "prompt is too long",
        "exceeds the model",
        "insufficient",
        "quota",
        "rate limit",
        "rate_limit",
        "unauthorized",
        "invalid api key",
        "authentication",
        "billing",
        "credit",
    ];
    BLOCKED.iter().any(|phrase| text.contains(phrase))
}

fn client_readable_error(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    match value.get("error") {
        Some(Value::String(message)) => !message.trim().is_empty(),
        Some(Value::Object(error)) => error
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| !message.trim().is_empty()),
        _ => false,
    }
}

fn rejection_excerpt(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let trimmed = text.trim();
    if trimmed.is_empty() || is_markup(trimmed) {
        return "upstream rejected the request".into();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(message) = find_message(&value) {
            let message = truncate_text(&message, ERROR_LIMIT);
            if !message.is_empty() && !is_markup(&message) {
                return message;
            }
        }
    }
    let plain = truncate_text(trimmed, ERROR_LIMIT);
    if plain.is_empty() || is_markup(&plain) {
        "upstream rejected the request".into()
    } else {
        plain
    }
}

fn is_markup(text: &str) -> bool {
    let lower = text.trim_start().to_ascii_lowercase();
    lower.starts_with('<') || lower.contains("<html") || lower.contains("<!doctype")
}

fn find_message(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Value::Array(items) => items.iter().find_map(find_message),
        Value::Object(map) => ["message", "detail", "msg", "description", "error"]
            .iter()
            .find_map(|key| map.get(*key).and_then(find_message)),
        _ => None,
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= max_chars {
            out.push('…');
            break;
        }
        if ch.is_control() && ch != '\n' && ch != '\t' {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn schema_note(schema: &Value) -> String {
    let rendered = serde_json::to_string(schema).unwrap_or_else(|_| "{}".into());
    let rendered = truncate_text(&rendered, SCHEMA_LIMIT);
    format!(
        "Return one JSON object and nothing else. The response must be json matching this schema:\n{rendered}"
    )
}

fn chat_json_schema(obj: &Map<String, Value>) -> Option<Value> {
    let format = obj.get("response_format")?;
    if format.get("type").and_then(Value::as_str) != Some("json_schema") {
        return None;
    }
    Some(
        format
            .pointer("/json_schema/schema")
            .cloned()
            .or_else(|| format.get("schema").cloned())
            .unwrap_or_else(|| Value::Object(Map::new())),
    )
}

fn messages_json_schema(obj: &Map<String, Value>) -> Option<Value> {
    let format = obj.get("output_config")?.get("format")?;
    if format.get("type").and_then(Value::as_str) != Some("json_schema") {
        return None;
    }
    Some(
        format
            .get("schema")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new())),
    )
}

fn strip_messages_format(obj: &mut Map<String, Value>) {
    let Some(config) = obj.get_mut("output_config").and_then(Value::as_object_mut) else {
        return;
    };
    config.remove("format");
    if config.is_empty() {
        obj.remove("output_config");
    }
}

fn inject_chat_system(obj: &mut Map<String, Value>, note: &str) {
    let Some(messages) = obj.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    if messages
        .first()
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
        == Some("system")
    {
        if let Some(content) = messages
            .first_mut()
            .and_then(|message| message.get_mut("content"))
        {
            prepend_text(content, note);
        }
        return;
    }
    messages.insert(0, serde_json::json!({"role": "system", "content": note}));
}

fn inject_messages_system(obj: &mut Map<String, Value>, note: &str) {
    match obj.get("system") {
        None => {
            obj.insert("system".into(), Value::String(note.to_string()));
        }
        Some(Value::String(_)) => {
            let existing = obj
                .get("system")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            obj.insert(
                "system".into(),
                Value::String(format!("{note}\n\n{existing}")),
            );
        }
        Some(Value::Array(_)) => {
            if let Some(parts) = obj.get_mut("system").and_then(Value::as_array_mut) {
                parts.insert(0, serde_json::json!({"type": "text", "text": note}));
            }
        }
        _ => {
            obj.insert("system".into(), Value::String(note.to_string()));
        }
    }
}

fn prepend_text(content: &mut Value, note: &str) {
    match content {
        Value::String(text) => {
            let existing = text.clone();
            *content = Value::String(format!("{note}\n\n{existing}"));
        }
        Value::Array(parts) => {
            parts.insert(0, serde_json::json!({"type": "text", "text": note}));
        }
        other => *other = Value::String(note.to_string()),
    }
}

fn responses_to_chat(obj: &Map<String, Value>, messages: Vec<Value>) -> Value {
    let mut out = Map::new();
    if let Some(model) = obj.get("model") {
        out.insert("model".into(), model.clone());
    }
    out.insert("messages".into(), Value::Array(messages));
    if let Some(format) = obj
        .get("text")
        .and_then(|text| text.get("format"))
        .and_then(response_format_from_text)
    {
        out.insert("response_format".into(), format);
    }
    if let Some(effort) = obj
        .get("reasoning")
        .and_then(|reasoning| reasoning.get("effort"))
        .and_then(Value::as_str)
        .filter(|effort| !effort.is_empty())
    {
        out.insert("reasoning_effort".into(), Value::String(effort.to_string()));
    } else if let Some(effort) = obj.get("reasoning_effort").filter(|value| !value.is_null()) {
        out.insert("reasoning_effort".into(), effort.clone());
    }
    if let Some(max) = obj
        .get("max_output_tokens")
        .or_else(|| obj.get("max_completion_tokens"))
        .or_else(|| obj.get("max_tokens"))
        .filter(|value| !value.is_null())
    {
        out.insert("max_tokens".into(), max.clone());
    }
    for key in [
        "stream",
        "stream_options",
        "temperature",
        "top_p",
        "user",
        "stop",
        "n",
    ] {
        if let Some(value) = obj.get(key).filter(|value| !value.is_null()) {
            out.insert(key.into(), value.clone());
        }
    }
    if let Some(tools) = obj.get("tools") {
        out.insert("tools".into(), convert_tools(tools));
    }
    if let Some(choice) = obj.get("tool_choice").and_then(map_tool_choice) {
        out.insert("tool_choice".into(), choice);
    }
    Value::Object(out)
}

fn response_format_from_text(format: &Value) -> Option<Value> {
    match format.get("type").and_then(Value::as_str)? {
        "json_schema" => {
            let mut schema = Map::new();
            if let Some(name) = format.get("name") {
                schema.insert("name".into(), name.clone());
            }
            if let Some(strict) = format.get("strict") {
                schema.insert("strict".into(), strict.clone());
            }
            if let Some(description) = format.get("description").filter(|value| !value.is_null()) {
                schema.insert("description".into(), description.clone());
            }
            if let Some(body) = format.get("schema") {
                schema.insert("schema".into(), body.clone());
            }
            Some(serde_json::json!({
                "type": "json_schema",
                "json_schema": schema,
            }))
        }
        "json_object" => Some(serde_json::json!({"type": "json_object"})),
        _ => None,
    }
}

fn convert_tools(tools: &Value) -> Value {
    let Some(items) = tools.as_array() else {
        return tools.clone();
    };
    Value::Array(
        items
            .iter()
            .map(|tool| {
                if tool.get("function").is_some() {
                    return tool.clone();
                }
                let kind = tool.get("type").and_then(Value::as_str).unwrap_or("");
                if kind != "function" && tool.get("name").is_none() {
                    return tool.clone();
                }
                let mut function = Map::new();
                for key in ["name", "description", "parameters", "strict"] {
                    if let Some(value) = tool.get(key) {
                        function.insert(key.into(), value.clone());
                    }
                }
                serde_json::json!({"type": "function", "function": function})
            })
            .collect(),
    )
}

fn map_tool_choice(choice: &Value) -> Option<Value> {
    match choice {
        Value::String(_) => Some(choice.clone()),
        Value::Object(obj) => {
            let name = obj.get("name").and_then(Value::as_str)?;
            if obj.get("type").and_then(Value::as_str) == Some("function")
                && obj.get("function").is_none()
            {
                Some(serde_json::json!({"type": "function", "function": {"name": name}}))
            } else {
                Some(choice.clone())
            }
        }
        _ => None,
    }
}

fn input_to_messages(input: Option<&Value>) -> Option<Vec<Value>> {
    let input = input?;
    let mut acc = MessageAcc::default();
    match input {
        Value::String(text) => acc.push_message("user", Value::String(text.clone())),
        Value::Array(items) => {
            for item in items {
                acc.push_item(item);
            }
        }
        Value::Object(_) => acc.push_item(input),
        _ => return None,
    }
    Some(acc.messages)
}

#[derive(Default)]
struct MessageAcc {
    messages: Vec<Value>,
    pending_reasoning: String,
}

impl MessageAcc {
    fn push_item(&mut self, item: &Value) {
        if let Some(text) = item.as_str() {
            self.push_message("user", Value::String(text.to_string()));
            return;
        }
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "function_call" => self.push_tool_call(item),
            "function_call_output" => self.push_tool_result(item),
            "reasoning" => {
                let text = collected_text(item);
                if !text.is_empty() {
                    if !self.pending_reasoning.is_empty() {
                        self.pending_reasoning.push('\n');
                    }
                    self.pending_reasoning.push_str(&text);
                }
            }
            _ if kind == "message" || item.get("role").is_some() => {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
                let content = item
                    .get("content")
                    .map(chat_content)
                    .unwrap_or_else(|| Value::String(String::new()));
                self.push_message(role, content);
            }
            _ => {}
        }
    }

    fn push_message(&mut self, role: &str, content: Value) {
        let mut message = Map::new();
        message.insert("role".into(), Value::String(role.to_string()));
        message.insert("content".into(), content);
        self.attach_reasoning(&mut message);
        self.messages.push(Value::Object(message));
    }

    fn push_tool_call(&mut self, item: &Value) {
        let call_id = item
            .get("call_id")
            .or_else(|| item.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("call")
            .to_string();
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let arguments = match item.get("arguments") {
            Some(Value::String(text)) => text.clone(),
            Some(value) => value.to_string(),
            None => "{}".into(),
        };
        if self.last_role() != Some("assistant") {
            let mut message = Map::new();
            message.insert("role".into(), Value::String("assistant".into()));
            message.insert("content".into(), Value::Null);
            message.insert("tool_calls".into(), Value::Array(Vec::new()));
            self.attach_reasoning(&mut message);
            self.messages.push(Value::Object(message));
        }
        let Some(message) = self.messages.last_mut().and_then(Value::as_object_mut) else {
            return;
        };
        if !message.get("tool_calls").is_some_and(Value::is_array) {
            message.insert("tool_calls".into(), Value::Array(Vec::new()));
        }
        let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) else {
            return;
        };
        calls.push(serde_json::json!({
            "id": call_id,
            "type": "function",
            "function": {"name": name, "arguments": arguments}
        }));
    }

    fn push_tool_result(&mut self, item: &Value) {
        let call_id = item
            .get("call_id")
            .or_else(|| item.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let content = match item.get("output") {
            Some(Value::String(text)) => Value::String(text.clone()),
            Some(value) => Value::String(collected_text(value)),
            None => Value::String(String::new()),
        };
        let mut message = Map::new();
        message.insert("role".into(), Value::String("tool".into()));
        message.insert("tool_call_id".into(), Value::String(call_id));
        message.insert("content".into(), content);
        self.messages.push(Value::Object(message));
    }

    fn last_role(&self) -> Option<&str> {
        self.messages
            .last()
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
    }

    fn attach_reasoning(&mut self, message: &mut Map<String, Value>) {
        if self.pending_reasoning.is_empty() {
            return;
        }
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            return;
        }
        message.insert(
            "reasoning_content".into(),
            Value::String(std::mem::take(&mut self.pending_reasoning)),
        );
    }
}

fn chat_content(content: &Value) -> Value {
    match content {
        Value::String(text) => Value::String(text.clone()),
        Value::Array(parts) => {
            let mut out = Vec::new();
            for part in parts {
                let kind = part.get("type").and_then(Value::as_str).unwrap_or("");
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if kind.is_empty()
                        || kind == "text"
                        || kind == "input_text"
                        || kind == "output_text"
                    {
                        out.push(serde_json::json!({"type": "text", "text": text}));
                        continue;
                    }
                }
                if kind == "input_image" || kind == "image_url" {
                    if let Some(url) = part
                        .get("image_url")
                        .and_then(Value::as_str)
                        .or_else(|| part.get("url").and_then(Value::as_str))
                    {
                        out.push(
                            serde_json::json!({"type": "image_url", "image_url": {"url": url}}),
                        );
                    }
                }
            }
            if out.len() == 1 && out[0].get("type").and_then(Value::as_str) == Some("text") {
                out[0]
                    .get("text")
                    .cloned()
                    .unwrap_or_else(|| Value::String(String::new()))
            } else {
                Value::Array(out)
            }
        }
        other => other.clone(),
    }
}

fn collected_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => ["content", "summary", "text", "output"]
            .iter()
            .filter_map(|key| map.get(*key))
            .map(collected_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A Chat Completions request as a Responses request. System and developer
/// messages become `instructions`; tool calls and results become function
/// items. Fields the conversion cannot carry are rejected, not dropped.
pub fn chat_request_to_responses(value: &Value) -> Result<Value, String> {
    let object = value.as_object().ok_or("Expected a JSON request object")?;
    for key in object.keys() {
        if ![
            "model",
            "messages",
            "stream",
            "stream_options",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "reasoning_effort",
        ]
        .contains(&key.as_str())
        {
            return Err(format!("Unsupported Chat Completions field: {key}"));
        }
    }
    let messages = value["messages"]
        .as_array()
        .ok_or("messages must be an array")?;
    let mut input = Vec::new();
    let mut instructions = Vec::new();
    for message in messages {
        let role = message["role"].as_str().ok_or("Message role missing")?;
        if matches!(role, "system" | "developer") {
            instructions.push(
                message["content"]
                    .as_str()
                    .ok_or("Instructions must be text")?
                    .to_owned(),
            );
            continue;
        }
        if role == "tool" {
            input.push(json!({"type":"function_call_output","call_id":message["tool_call_id"].as_str().ok_or("Tool call ID missing")?,"output":message["content"].as_str().ok_or("Tool output must be text")?}));
            continue;
        }
        if !matches!(role, "user" | "assistant") {
            return Err("Unsupported message role".into());
        }
        if let Some(calls) = message.get("tool_calls") {
            if role != "assistant" {
                return Err("Only assistant messages may contain tool calls".into());
            }
            for call in calls.as_array().ok_or("tool_calls must be an array")? {
                if call["type"] != "function" {
                    return Err("Only function tools are supported".into());
                }
                input.push(json!({"type":"function_call","call_id":call["id"].as_str().ok_or("Tool call ID missing")?,"name":call["function"]["name"].as_str().ok_or("Tool name missing")?,"arguments":call["function"]["arguments"].as_str().ok_or("Tool arguments must be a string")?}));
            }
        }
        let content = &message["content"];
        if content.is_null() {
            continue;
        }
        let parts = if let Some(text) = content.as_str() {
            vec![
                json!({"type":if role=="assistant" {"output_text"} else {"input_text"},"text":text}),
            ]
        } else {
            content.as_array().ok_or("Unsupported message content")?.iter().map(|part| {
                match part["type"].as_str() {
                    Some("text")=>Ok(json!({"type":if role=="assistant" {"output_text"} else {"input_text"},"text":part["text"].as_str().ok_or("Text missing")?})),
                    Some("image_url") if role=="user"=>Ok(json!({"type":"input_image","image_url":part["image_url"]["url"].as_str().ok_or("Image URL missing")?,"detail":part["image_url"].get("detail").cloned().unwrap_or(json!("auto"))})),
                    _=>Err("Unsupported content type".to_string()),
                }
            }).collect::<Result<Vec<_>,String>>()?
        };
        input.push(json!({"role":role,"content":parts}));
    }
    let mut request =
        json!({"model":value["model"],"input":input,"instructions":instructions.join("\n\n")});
    if let Some(tools) = value.get("tools") {
        let converted = tools
            .as_array()
            .ok_or("tools must be an array")?
            .iter()
            .map(|tool| {
                if tool["type"] != "function" {
                    return Err("Only function tools are supported".to_string());
                }
                let mut function = tool["function"]
                    .as_object()
                    .ok_or("Invalid function tool")?
                    .clone();
                function.insert("type".into(), json!("function"));
                Ok(Value::Object(function))
            })
            .collect::<Result<Vec<_>, String>>()?;
        request["tools"] = json!(converted);
    }
    for key in ["tool_choice", "parallel_tool_calls"] {
        if let Some(v) = value.get(key) {
            request[key] = v.clone();
        }
    }
    if request["tool_choice"].is_object() {
        request["tool_choice"] = json!({"type":"function","name":request["tool_choice"]["function"]["name"].as_str().ok_or("Invalid tool choice")?});
    }
    if let Some(effort) = value.get("reasoning_effort") {
        request["reasoning"] = json!({"effort":effort});
    }
    Ok(request)
}

/// A final Responses object as a non-streaming Chat Completions reply.
pub fn responses_to_chat_response(response: &Value) -> Value {
    let mut text = String::new();
    let mut tools = Vec::new();
    let mut refusal = String::new();
    for item in response["output"].as_array().into_iter().flatten() {
        if item["type"] == "function_call" {
            tools.push(json!({"id":item["call_id"],"type":"function","function":{"name":item["name"],"arguments":item["arguments"]}}));
        }
        for part in item["content"].as_array().into_iter().flatten() {
            if let Some(s) = part["text"].as_str() {
                text.push_str(s);
            }
            if let Some(s) = part["refusal"].as_str() {
                refusal.push_str(s);
            }
        }
    }
    let mut message =
        json!({"role":"assistant","content":if text.is_empty(){Value::Null}else{json!(text)}});
    if !tools.is_empty() {
        message["tool_calls"] = json!(tools);
    }
    if !refusal.is_empty() {
        message["refusal"] = json!(refusal);
    }
    json!({"id":response["id"],"object":"chat.completion","created":response["created_at"],"model":response["model"],
        "choices":[{"index":0,"message":message,"finish_reason":if response["status"]=="incomplete" {"length"} else if !tools.is_empty(){"tool_calls"}else{"stop"}}],"usage":chat_usage(&response["usage"])})
}
fn chat_usage(usage: &Value) -> Value {
    json!({"prompt_tokens":usage["input_tokens"],"completion_tokens":usage["output_tokens"],"total_tokens":usage["total_tokens"],
        "prompt_tokens_details":usage["input_tokens_details"],"completion_tokens_details":usage["output_tokens_details"]})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_schema_becomes_chat_messages() {
        let body = serde_json::json!({
            "model": "deepseek-flash",
            "input": [
                {"type": "message", "role": "system", "content": "You are the hidden completion evaluator."},
                {"type": "reasoning", "content": [{"type": "reasoning_text", "text": "looked"}]},
                {"type": "message", "role": "assistant", "content": "did the edit"},
                {"type": "function_call", "call_id": "call_1", "name": "read_file", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "fn main() {}"}
            ],
            "text": {"format": {
                "type": "json_schema",
                "name": "structured_output",
                "strict": true,
                "schema": {"type": "object", "properties": {"decision": {"type": "string"}}}
            }},
            "reasoning": {"effort": "low"},
            "max_output_tokens": 800,
            "stream": true,
            "tools": [{"type": "function", "name": "read_file", "parameters": {"type": "object"}}]
        });
        let raw = serde_json::to_vec(&body).unwrap();
        let out: Value =
            serde_json::from_slice(&adapt_upstream_body("/v1/chat/completions", &raw)).unwrap();
        assert!(out.get("input").is_none());
        assert_eq!(out["messages"][0]["role"], "system");
        assert_eq!(out["messages"][1]["reasoning_content"], "looked");
        assert_eq!(out["messages"][1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(out["messages"][2]["role"], "tool");
        assert_eq!(out["response_format"]["type"], "json_schema");
        assert_eq!(
            out["response_format"]["json_schema"]["name"],
            "structured_output"
        );
        assert_eq!(out["response_format"]["json_schema"]["strict"], true);
        assert_eq!(out["reasoning_effort"], "low");
        assert_eq!(out["max_tokens"], 800);
        assert_eq!(out["stream"], true);
        assert_eq!(out["tools"][0]["function"]["name"], "read_file");
    }

    #[test]
    fn string_input_becomes_a_user_message() {
        let raw = br#"{"model":"deepseek-flash","input":"hi"}"#;
        let out: Value =
            serde_json::from_slice(&adapt_upstream_body("/v1/chat/completions?x=1", raw)).unwrap();
        assert_eq!(out["model"], "deepseek-flash");
        assert_eq!(out["messages"][0]["content"], "hi");
        assert!(out.get("input").is_none());
    }

    #[test]
    fn grok_responses_path_keeps_input() {
        let raw = br#"{"model":"grok-4.7","input":"hi","stream":true}"#;
        assert_eq!(adapt_upstream_body("/v1/responses", raw), raw);
    }

    #[test]
    fn chat_body_is_unchanged() {
        let raw = br#"{"model":"deepseek-flash","messages":[{"role":"user","content":"hi"}]}"#;
        assert_eq!(adapt_upstream_body("/v1/chat/completions", raw), raw);
    }

    #[test]
    fn html_400_on_json_schema_downgrades_to_json_object() {
        let request = serde_json::json!({
            "model": "deepseek-flash",
            "messages": [
                {"role": "system", "content": "Evaluate."},
                {"role": "user", "content": "goal"}
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "structured_output",
                    "strict": true,
                    "schema": {"type": "object", "required": ["decision"]}
                }
            }
        });
        let raw = serde_json::to_vec(&request).unwrap();
        let next = downgrade_after_rejection(400, b"<html>nope</html>", &raw).unwrap();
        let out: Value = serde_json::from_slice(&next).unwrap();
        assert_eq!(out["response_format"]["type"], "json_object");
        let system = out["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("json"));
        assert!(system.contains("decision"));
        assert!(system.contains("Evaluate."));
    }

    #[test]
    fn messages_schema_drops_output_format() {
        let request = serde_json::json!({
            "model": "union-alpha",
            "system": "Be brief.",
            "messages": [{"role": "user", "content": "goal"}],
            "output_config": {"format": {"type": "json_schema", "schema": {"type": "object"}}}
        });
        let raw = serde_json::to_vec(&request).unwrap();
        let next = downgrade_structured_output(&raw).unwrap();
        let out: Value = serde_json::from_slice(&next).unwrap();
        assert!(out.get("output_config").is_none());
        assert!(out["system"].as_str().unwrap().contains("json"));
        assert!(out["system"].as_str().unwrap().contains("Be brief."));
    }

    #[test]
    fn context_length_400_is_not_retried() {
        let request = br#"{"messages":[],"response_format":{"type":"json_schema","json_schema":{"schema":{}}}}"#;
        let error = br#"{"error":{"message":"The prompt is too long for this model's context window","type":"invalid_request_error"}}"#;
        assert!(downgrade_after_rejection(400, error, request).is_none());
        assert!(downgrade_after_rejection(401, b"<html></html>", request).is_none());
        assert!(downgrade_after_rejection(429, b"", request).is_none());
    }

    #[test]
    fn opaque_rejection_becomes_an_error_message() {
        let wrapped: Value =
            serde_json::from_slice(&normalize_rejection(b"<html>secret</html>")).unwrap();
        assert_eq!(wrapped["error"]["type"], "upstream_error");
        assert_eq!(wrapped["error"]["message"], "upstream rejected the request");
        let kept = br#"{"error":{"message":"bad schema","type":"invalid_request_error"}}"#;
        assert_eq!(normalize_rejection(kept), kept);
        let flat = br#"{"error":"invalid_model"}"#;
        assert_eq!(normalize_rejection(flat), flat);
        let from_message: Value = serde_json::from_slice(&normalize_rejection(
            br#"{"message":"response_format is not supported"}"#,
        ))
        .unwrap();
        assert_eq!(
            from_message["error"]["message"],
            "response_format is not supported"
        );
    }

    #[test]
    fn chat_requests_become_responses_requests_and_back() {
        let chat = serde_json::json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "Be brief"},
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "f", "arguments": "{}"}}]},
                {"role": "tool", "tool_call_id": "c1", "content": "ok"}
            ],
            "reasoning_effort": "low"
        });
        let request = chat_request_to_responses(&chat).unwrap();
        assert_eq!(request["instructions"], "Be brief");
        assert_eq!(request["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(request["input"][1]["type"], "function_call");
        assert_eq!(request["input"][2]["output"], "ok");
        assert_eq!(request["reasoning"]["effort"], "low");
        assert!(chat_request_to_responses(
            &serde_json::json!({"model": "m", "messages": [], "audio": {}})
        )
        .is_err());

        let reply = responses_to_chat_response(&serde_json::json!({
            "id": "r1", "model": "m", "status": "completed",
            "output": [
                {"type": "message", "content": [{"type": "output_text", "text": "Hello"}]},
                {"type": "function_call", "call_id": "c2", "name": "g", "arguments": "{}"}
            ],
            "usage": {"input_tokens": 3, "output_tokens": 2, "total_tokens": 5}
        }));
        assert_eq!(reply["choices"][0]["message"]["content"], "Hello");
        assert_eq!(reply["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(reply["usage"]["prompt_tokens"], 3);
    }
}
