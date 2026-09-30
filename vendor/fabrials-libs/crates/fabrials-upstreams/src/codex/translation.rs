use super::CodexAdapter;
use fabrials_fabric::provider::{CredentialInjector, Upstream, UsageExtractor};
use fabrials_fabric::reasoning::{self, Issuer, Keep};
use fabrials_types::HopRecord;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};

/// Oldest Codex release ChatGPT lists every current subscription model to
/// (GPT-6.1 Sol needs 0.159). The catalog is requested with at least this
/// version, so an older client still sees the models it can already call.
pub(crate) const DEFAULT_CATALOG_CLIENT_VERSION: &str = "0.159.0";

fn version_parts(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// The newer of the client's version and [`DEFAULT_CATALOG_CLIENT_VERSION`].
pub(crate) fn catalog_client_version(requested: Option<&str>) -> &str {
    match requested {
        Some(version) if version_parts(version) > version_parts(DEFAULT_CATALOG_CLIENT_VERSION) => {
            version
        }
        _ => DEFAULT_CATALOG_CLIENT_VERSION,
    }
}

pub(crate) fn client_version_from_query(query: &str) -> Option<&str> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "client_version" && valid_client_version(value)).then_some(value)
    })
}

fn valid_client_version(value: &str) -> bool {
    (1..=16).contains(&value.len())
        && value.chars().any(|character| character.is_ascii_digit())
        && value
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(crate) fn strip_unsupported_parameters(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        object.remove("temperature");
    }
}

pub fn responses_request(mut value: Value, chat: bool) -> Result<Value, String> {
    // Optional JSON nulls mean unspecified, including clients overriding an
    // SDK's default output cap for subscription endpoints.
    value
        .as_object_mut()
        .ok_or("Expected a JSON request object")?
        .retain(|_, field| !field.is_null());
    let object = value.as_object().ok_or("Expected a JSON request object")?;
    if object
        .get("model")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("A Codex model is required".into());
    }
    if object.contains_key("max_output_tokens")
        || object.contains_key("max_tokens")
        || object.contains_key("max_completion_tokens")
    {
        return Err("Codex subscriptions do not support an output-token limit. Remove max_output_tokens, max_tokens and max_completion_tokens; use an API provider if your client requires a token cap.".into());
    }
    strip_unsupported_parameters(&mut value);
    if chat {
        value = fabrials_fabric::wire_compat::chat_request_to_responses(&value)?;
    }
    for item in value["input"].as_array_mut().into_iter().flatten() {
        if item["role"] == "system" {
            item["role"] = json!("developer");
        }
    }
    reasoning::strip(Issuer::OpenAi, &mut value, Keep::Unknown);
    value["store"] = json!(false);
    value["stream"] = json!(true);
    if value.get("instructions").is_none() {
        value["instructions"] = json!("");
    }
    Ok(value)
}

fn json_reply(client: &mut dyn Write, status: u16, value: &Value) -> Result<(), String> {
    let body = value.to_string();
    write!(client,"HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).map_err(|_|"Client disconnected".into())
}
fn rejection_body(response: reqwest::blocking::Response) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = response.take(64 * 1024).read_to_end(&mut bytes);
    bytes
}

/// The upstream's own reason, bounded, so a client can show why Codex refused.
fn rejection_message(body: &[u8]) -> String {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let detail = value["error"]["message"]
        .as_str()
        .or_else(|| value["detail"].as_str())
        .or_else(|| value["message"].as_str())
        .map(str::trim)
        .filter(|detail| !detail.is_empty());
    match detail {
        Some(detail) => {
            let detail: String = detail.chars().take(300).collect();
            format!("Codex rejected the request: {detail}")
        }
        None => "Codex rejected the request. Check authorization, model availability and supported parameters.".into(),
    }
}

fn reject(
    client: &mut dyn Write,
    record: &mut HopRecord,
    started: std::time::Instant,
    status: u16,
    rejected: &[u8],
) -> Result<Option<HopRecord>, String> {
    record.status = Some(status);
    record.duration_ms = Some(started.elapsed().as_millis() as u64);
    json_reply(
        client,
        status,
        &json!({"error":{"message":rejection_message(rejected),"type":"upstream_error","code":status}}),
    )?;
    Ok(Some(record.clone()))
}

/// The SSE packet with its `data:` payload replaced by `event`.
fn rewrite_data(raw: &str, event: &Value) -> Vec<u8> {
    let mut out = String::new();
    for line in raw.lines().filter(|line| !line.starts_with("data:")) {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("data: ");
    out.push_str(&event.to_string());
    out.push('\n');
    out.into_bytes()
}

fn data(client: &mut dyn Write, value: &Value) -> bool {
    write!(client, "data: {value}\n\n")
        .and_then(|_| client.flush())
        .is_ok()
}

pub fn forward(
    adapter: &CodexAdapter,
    method: &str,
    path: &str,
    body: &[u8],
    token: &str,
    secret: Option<&Value>,
    client: &mut dyn Write,
) -> Result<Option<HopRecord>, String> {
    let started = std::time::Instant::now();
    let api_path = path.split_once('?').map(|(path, _)| path).unwrap_or(path);
    let chat = api_path.ends_with("/chat/completions");
    let completion_id = format!("chatcmpl-{}", now_ms());
    let created = now_ms() / 1000;
    let mut record = HopRecord {
        ts_ms: now_ms(),
        status: Some(502),
        ..Default::default()
    };
    let original: Value = if method == "GET" {
        json!({})
    } else {
        match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(_) => {
                json_reply(
                    client,
                    400,
                    &json!({"error":{"message":"Invalid JSON","type":"invalid_request_error"}}),
                )?;
                return Ok(None);
            }
        }
    };
    let stream = original["stream"].as_bool().unwrap_or(false);
    let request = if method == "GET" {
        None
    } else {
        match responses_request(original.clone(), chat) {
            Ok(v) => Some(v),
            Err(message) => {
                json_reply(
                    client,
                    400,
                    &json!({"error":{"message":message,"type":"invalid_request_error"}}),
                )?;
                return Ok(None);
            }
        }
    };
    record.model = original["model"].as_str().map(str::to_owned);
    let base = adapter.base.as_deref().unwrap_or("https://chatgpt.com");
    let requested_version = path
        .split_once('?')
        .and_then(|(_, query)| client_version_from_query(query));
    let endpoint = if method == "GET" {
        format!(
            "/backend-api/codex/models?client_version={}",
            catalog_client_version(requested_version)
        )
    } else {
        "/backend-api/codex/responses".to_string()
    };
    let url = fabrials_fabric::forward::validated_upstream_url(base, &endpoint)
        .map_err(|_| "Invalid Codex origin")?;
    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not initialize Codex proxy")?;
    let hop = Upstream {
        base: base.into(),
        path: path.into(),
        route: "codex",
        account_alias: None,
    };
    let headers = adapter.credential_headers(token, &hop, secret)?;
    let send = |request: Option<&Value>| {
        let mut req = http.request(
            if method == "GET" {
                reqwest::Method::GET
            } else {
                reqwest::Method::POST
            },
            url.clone(),
        );
        for (key, value) in &headers {
            req = req.header(key, value);
        }
        if let Some(request) = request {
            req = req
                .header("Content-Type", "application/json")
                .header("Accept", "text/event-stream")
                .body(request.to_string());
        }
        req.send().map_err(|_| "Codex upstream unavailable")
    };
    let mut request = request;
    let mut response = send(request.as_ref())?;
    let mut status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let rejected = rejection_body(response);
        let retry = request
            .clone()
            .filter(|_| matches!(status, 400 | 404))
            .and_then(|mut next| {
                reasoning::strip(Issuer::OpenAi, &mut next, Keep::OwnOnly).then_some(next)
            });
        let Some(next) = retry else {
            return reject(client, &mut record, started, status, &rejected);
        };
        request = Some(next);
        response = send(request.as_ref())?;
        status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let rejected = rejection_body(response);
            return reject(client, &mut record, started, status, &rejected);
        }
    }
    if let Some(request) = request.as_ref() {
        reasoning::observe_accepted_request(Issuer::OpenAi, request.to_string().as_bytes());
    }
    if method == "GET" {
        let mut bytes = Vec::new();
        response
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Codex model catalog unavailable")?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("Codex model catalog too large".into());
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid Codex model catalog")?;
        let rows = value["models"]
            .as_array()
            .filter(|rows| rows.len() <= 2048)
            .ok_or("Invalid Codex model catalog")?;
        let models = rows
            .iter()
            .map(|model| {
                let id = model["slug"]
                    .as_str()
                    .filter(|id| {
                        !id.is_empty()
                            && id.len() <= 256
                            && id.bytes().all(|b| b.is_ascii_graphic())
                    })
                    .ok_or("Invalid Codex model identifier")?;
                Ok(json!({"id":id,"object":"model","owned_by":"openai"}))
            })
            .collect::<Result<Vec<_>, String>>()?;
        // Codex CLI deserializes this endpoint into its native catalog. A
        // reduced OpenAI list makes the picker fall back to the binary's
        // bundled models and hide anything that shipped after that build.
        if requested_version.is_some() {
            json_reply(client, 200, &value)?;
        } else {
            json_reply(client, 200, &json!({"object":"list","data":models}))?;
        }
        return Ok(None);
    }
    if stream && write!(client,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n").is_err() {record.status=Some(499);return Ok(Some(record));}
    let mut reader = BufReader::new(response);
    let mut packet = Vec::new();
    let mut total = 0usize;
    let mut terminal = None;
    let mut tool_indices = std::collections::HashMap::new();
    let mut done_items: Vec<Value> = Vec::new();
    loop {
        let mut line = Vec::new();
        let n = match reader
            .by_ref()
            .take(8 * 1024 * 1024 + 1)
            .read_until(b'\n', &mut line)
        {
            Ok(n) => n,
            Err(_) => break,
        };
        total = total.saturating_add(n);
        if line.len() > 8 * 1024 * 1024 || total > 64 * 1024 * 1024 {
            break;
        }
        if n > 0 && !matches!(line.as_slice(), b"\n" | b"\r\n") {
            packet.extend(line);
            if packet.len() > 8 * 1024 * 1024 {
                break;
            }
            continue;
        }
        if packet.is_empty() {
            if n == 0 {
                break;
            }
            continue;
        }
        let raw = String::from_utf8_lossy(&packet);
        let payload = raw
            .lines()
            .filter_map(|l| l.strip_prefix("data:").map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n");
        let event: Value = match serde_json::from_str(&payload) {
            Ok(v) => v,
            Err(_) => {
                packet.clear();
                if n == 0 {
                    break;
                }
                continue;
            }
        };
        let mut event = event;
        let kind = event["type"].as_str().unwrap_or("").to_string();
        let kind = kind.as_str();
        if kind == "response.output_item.done" {
            if event["item"]["type"] == "reasoning" {
                if let Some(blob) = event["item"]["encrypted_content"].as_str() {
                    reasoning::record(Issuer::OpenAi, blob);
                }
            }
            done_items.push(event["item"].clone());
        }
        let mut rewritten = false;
        if matches!(kind, "response.completed" | "response.incomplete")
            && event["response"]["output"]
                .as_array()
                .is_some_and(Vec::is_empty)
            && !done_items.is_empty()
        {
            event["response"]["output"] = Value::Array(std::mem::take(&mut done_items));
            rewritten = true;
        }
        if matches!(kind, "response.completed" | "response.incomplete") {
            let response = &event["response"];
            if !response.is_object()
                || !response["output"].is_array()
                || response["id"].as_str().is_none_or(str::is_empty)
                || response["model"].as_str().is_none_or(str::is_empty)
                || !matches!(
                    response["status"].as_str(),
                    Some("completed" | "incomplete")
                )
            {
                break;
            }
            terminal = Some(response.clone());
        }
        if matches!(kind, "error" | "response.failed") {
            break;
        }
        if stream {
            let ok = if chat {
                let mut delta = None;
                match kind {
                    "response.created" => delta = Some(json!({"role":"assistant","content":""})),
                    "response.output_text.delta" => delta = Some(json!({"content":event["delta"]})),
                    "response.refusal.delta" => delta = Some(json!({"refusal":event["delta"]})),
                    "response.output_item.added" if event["item"]["type"] == "function_call" => {
                        let index = tool_indices.len();
                        tool_indices.insert(event["output_index"].as_u64().unwrap_or(0), index);
                        delta = Some(
                            json!({"tool_calls":[{"index":index,"id":event["item"]["call_id"],"type":"function","function":{"name":event["item"]["name"],"arguments":""}}]}),
                        );
                    }
                    "response.function_call_arguments.delta" => {
                        if let Some(index) =
                            tool_indices.get(&event["output_index"].as_u64().unwrap_or(0))
                        {
                            delta = Some(
                                json!({"tool_calls":[{"index":index,"function":{"arguments":event["delta"]}}]}),
                            );
                        }
                    }
                    _ => {}
                }
                delta.is_none_or(|delta|data(client,&json!({"id":completion_id,"object":"chat.completion.chunk","created":created,"model":record.model,"choices":[{"index":0,"delta":delta,"finish_reason":null}]})))
            } else {
                let packet = if rewritten {
                    rewrite_data(&raw, &event)
                } else {
                    packet.clone()
                };
                client
                    .write_all(&packet)
                    .and_then(|_| client.write_all(b"\n"))
                    .and_then(|_| client.flush())
                    .is_ok()
            };
            if !ok {
                record.status = Some(499);
                return Ok(Some(record));
            }
        }
        packet.clear();
        if terminal.is_some() || n == 0 {
            break;
        }
    }
    let Some(terminal) = terminal else {
        let error = json!({"error":{"message":"Codex stream ended before completion","type":"upstream_error"}});
        if stream {
            data(client, &error);
        } else {
            json_reply(client, 502, &error)?;
        }
        return Ok(Some(record));
    };
    if let Some(usage) = adapter.parse_usage(terminal.to_string().as_bytes()) {
        record = usage;
    }
    record.status = Some(200);
    record.duration_ms = Some(started.elapsed().as_millis() as u64);
    if chat {
        let result = fabrials_fabric::wire_compat::responses_to_chat_response(&terminal);
        if stream {
            data(
                client,
                &json!({"id":completion_id,"object":"chat.completion.chunk","created":created,"model":terminal["model"],"choices":[{"index":0,"delta":{},"finish_reason":result["choices"][0]["finish_reason"]}]}),
            );
            if original["stream_options"]["include_usage"] == true {
                data(
                    client,
                    &json!({"id":completion_id,"object":"chat.completion.chunk","created":created,"model":terminal["model"],"choices":[],"usage":result["usage"]}),
                );
            }
            let _ = client.write_all(b"data: [DONE]\n\n");
        } else {
            json_reply(client, 200, &result)?;
        }
    } else if !stream {
        json_reply(client, 200, &terminal)?;
    }
    Ok(Some(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn null_optional_caps_are_unspecified_but_numeric_caps_stay_rejected() {
        let translated = responses_request(json!({"model":"allowed","messages":[{"role":"user","content":"Hi"}],"max_tokens":null,"max_completion_tokens":null,"max_output_tokens":null,"temperature":null}), true).unwrap();
        assert_eq!(translated["input"][0]["role"], "user");
        for key in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
            assert!(translated.get(key).is_none());
            let mut request = json!({"model":"allowed","messages":[]});
            request[key] = json!(4);
            assert!(responses_request(request, true).is_err());
        }
    }

    #[test]
    fn sampling_temperature_is_removed_without_filtering_unconfirmed_fields() {
        for chat in [false, true] {
            for temperature in [json!(1.0), Value::Null] {
                let mut original = json!({"model":"gpt-6.1","temperature":temperature});
                if !chat {
                    original["top_p"] = json!(0.9);
                }
                original[if chat { "messages" } else { "input" }] =
                    json!([{"role":"user","content":"Summarize"}]);
                original[if chat {
                    "reasoning_effort"
                } else {
                    "reasoning"
                }] = if chat {
                    json!("low")
                } else {
                    json!({"effort":"low"})
                };
                let request = responses_request(original, chat).unwrap();
                assert!(request.get("temperature").is_none());
                if !chat {
                    assert_eq!(request["top_p"], 0.9);
                }
                assert_eq!(request["reasoning"]["effort"], "low");
            }
        }
    }

    #[test]
    fn subscription_output_caps_are_rejected_before_contacting_provider() {
        for field in ["max_output_tokens", "max_tokens", "max_completion_tokens"] {
            let mut request = json!({"model":"fixture","messages":[],"input":[]});
            request[field] = json!(32);
            assert!(responses_request(request.clone(), true)
                .unwrap_err()
                .contains("token limit"));
            assert!(responses_request(request, false).is_err());
        }
    }
    #[test]
    fn chat_tools_images_and_instructions_preserve_semantics() {
        let request=responses_request(json!({"model":"fixture","messages":[
            {"role":"system","content":"Be concise"},
            {"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,fixture"}}]},
            {"role":"assistant","tool_calls":[{"id":"call-1","type":"function","function":{"name":"weather","arguments":"{}"}}]},
            {"role":"tool","tool_call_id":"call-1","content":"sunny"}],
            "tools":[{"type":"function","function":{"name":"weather","parameters":{"type":"object"}}}],
            "tool_choice":{"type":"function","function":{"name":"weather"}}}),true).unwrap();
        assert_eq!(request["instructions"], "Be concise");
        assert_eq!(request["store"], false);
        assert_eq!(request["input"][0]["content"][0]["type"], "input_image");
        assert_eq!(request["input"][1]["call_id"], "call-1");
        assert_eq!(request["input"][2]["output"], "sunny");
        assert_eq!(request["tools"][0]["name"], "weather");
        assert_eq!(request["tool_choice"]["name"], "weather");
        assert!(
            responses_request(json!({"model":"fixture","messages":[],"audio":{}}), true).is_err()
        );
    }

    /// Serves one scripted reply per connection and hands back each request body.
    fn scripted_upstream(
        replies: Vec<(u16, String)>,
    ) -> (String, std::thread::JoinHandle<Vec<Value>>) {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for (status, reply) in replies {
                let (mut socket, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(socket.try_clone().unwrap());
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_lowercase().strip_prefix("content-length: ") {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                let (status, reply) = if body.get("temperature").is_some() {
                    (
                        400,
                        r#"{"error":{"message":"Unsupported parameter: temperature"}}"#.to_string(),
                    )
                } else {
                    (status, reply)
                };
                bodies.push(body);
                let kind = if status == 200 {
                    "text/event-stream"
                } else {
                    "application/json"
                };
                write!(socket, "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).unwrap();
            }
            bodies
        });
        (base, handle)
    }

    #[test]
    fn compaction_and_followup_complete_against_temperature_rejecting_upstream() {
        for chat in [false, true] {
            let summary = "Summary of prior turns";
            let reply = |text: &str| {
                format!(
                    "data: {}\n\n",
                    json!({"type":"response.completed","response":{"id":"resp_compact","model":"gpt-6.1","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}],"usage":{"input_tokens":8,"output_tokens":2,"total_tokens":10}}})
                )
            };
            let (base, upstream) =
                scripted_upstream(vec![(200, reply(summary)), (200, reply("Continued"))]);
            let adapter = CodexAdapter { base: Some(base) };
            for (prompt, expected) in [("Summarize prior turns", summary), (summary, "Continued")] {
                let mut request = json!({"model":"gpt-6.1","temperature":1.0,"stream":false});
                request[if chat {
                    "reasoning_effort"
                } else {
                    "reasoning"
                }] = if chat {
                    json!("low")
                } else {
                    json!({"effort":"low"})
                };
                request[if chat { "messages" } else { "input" }] =
                    json!([{"role":"user","content":prompt}]);
                let mut output = Vec::new();
                let record = forward(
                    &adapter,
                    "POST",
                    if chat {
                        "/backend-api/codex/chat/completions"
                    } else {
                        "/backend-api/codex/responses"
                    },
                    request.to_string().as_bytes(),
                    "fixture-token",
                    Some(&json!({"account_id":"fixture-account"})),
                    &mut output,
                )
                .unwrap()
                .unwrap();
                assert_eq!(record.status, Some(200));
                assert_eq!(record.total_tokens, 10);
                let wire = String::from_utf8(output).unwrap();
                let response: Value =
                    serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap();
                if chat {
                    assert_eq!(response["choices"][0]["message"]["content"], expected);
                    assert_eq!(response["choices"][0]["finish_reason"], "stop");
                } else {
                    assert_eq!(response["output"][0]["content"][0]["text"], expected);
                    assert_eq!(response["status"], "completed");
                }
            }
            let bodies = upstream.join().unwrap();
            assert_eq!(bodies.len(), 2);
            for body in bodies {
                assert!(body.get("temperature").is_none());
                assert_eq!(body["reasoning"]["effort"], "low");
                assert_eq!(body["store"], false);
                assert_eq!(body["stream"], true);
            }
        }
    }

    #[test]
    fn system_messages_become_developer_messages() {
        let request = responses_request(
            json!({"model":"fixture","input":[{"type":"message","role":"system","content":"rules"},{"type":"message","role":"user","content":"hi"}]}),
            false,
        )
        .unwrap();
        assert_eq!(request["input"][0]["role"], "developer");
        assert_eq!(request["input"][1]["role"], "user");
    }

    #[test]
    fn empty_completed_output_is_filled_from_streamed_items() {
        let call = json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"read_file","arguments":"{}","status":"completed"});
        let terminal = json!({"id":"resp_1","model":"fixture","status":"completed","output":[],"usage":{"input_tokens":3,"output_tokens":1,"total_tokens":4}});
        let stream = format!(
            "event: response.output_item.done\ndata: {}\n\nevent: response.completed\ndata: {}\n\n",
            json!({"type":"response.output_item.done","output_index":0,"item":call}),
            json!({"type":"response.completed","response":terminal})
        );
        let (base, upstream) = scripted_upstream(vec![(200, stream)]);
        let adapter = CodexAdapter { base: Some(base) };
        let mut output = Vec::new();
        forward(
            &adapter,
            "POST",
            "/codex/v1/responses",
            json!({"model":"fixture","input":[],"stream":true})
                .to_string()
                .as_bytes(),
            "fixture-token",
            Some(&json!({"account_id":"fixture-account"})),
            &mut output,
        )
        .unwrap();
        upstream.join().unwrap();
        let wire = String::from_utf8(output).unwrap();
        let completed = wire
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|event| event["type"] == "response.completed")
            .unwrap();
        assert_eq!(completed["response"]["output"][0]["call_id"], "call_1");
        assert!(wire.contains("event: response.completed"));
    }

    #[test]
    fn rejected_reasoning_is_retried_without_it_and_errors_keep_their_reason() {
        let stale = json!({"type":"reasoning","id":"rs_stale","summary":[],"encrypted_content":"stale-blob-from-another-process"});
        let ok = format!(
            "data: {}\n\n",
            json!({"type":"response.completed","response":{"id":"resp_2","model":"fixture","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"hi"}]}]}})
        );
        let (base, upstream) = scripted_upstream(vec![
            (
                400,
                r#"{"error":{"message":"Item with id 'rs_stale' not found."}}"#.into(),
            ),
            (200, ok),
        ]);
        let adapter = CodexAdapter { base: Some(base) };
        let body = json!({"model":"fixture","stream":true,"input":[{"type":"message","role":"user","content":"hi"},stale]});
        let mut output = Vec::new();
        let record = forward(
            &adapter,
            "POST",
            "/codex/v1/responses",
            body.to_string().as_bytes(),
            "fixture-token",
            Some(&json!({"account_id":"fixture-account"})),
            &mut output,
        )
        .unwrap()
        .unwrap();
        let bodies = upstream.join().unwrap();
        assert_eq!(record.status, Some(200));
        assert_eq!(bodies[0]["input"].as_array().map(Vec::len), Some(2));
        assert_eq!(bodies[1]["input"].as_array().map(Vec::len), Some(1));

        let (base, upstream) = scripted_upstream(vec![(
            400,
            r#"{"error":{"message":"Unsupported parameter: temperature"}}"#.into(),
        )]);
        let adapter = CodexAdapter { base: Some(base) };
        let mut output = Vec::new();
        forward(
            &adapter,
            "POST",
            "/codex/v1/responses",
            json!({"model":"fixture","input":[]}).to_string().as_bytes(),
            "fixture-token",
            Some(&json!({"account_id":"fixture-account"})),
            &mut output,
        )
        .unwrap();
        upstream.join().unwrap();
        let wire = String::from_utf8(output).unwrap();
        assert!(wire.contains("Codex rejected the request: Unsupported parameter: temperature"));
    }

    #[test]
    fn fragmented_sse_becomes_chat_chunks_and_one_usage_record() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fixture = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                headers.push_str(&line);
            }
            assert!(headers
                .to_lowercase()
                .contains("chatgpt-account-id: fixture-account"));
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|s| s.parse::<usize>().ok())
                })
                .unwrap();
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["stream"], true);
            let terminal = json!({"id":"resp-fixture","created_at":1,"model":"fixture","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"hello"}]}],"usage":{"input_tokens":8,"output_tokens":2,"total_tokens":10}});
            let events = [
                json!({"type":"response.created","response":{"id":"resp-fixture"}}),
                json!({"type":"response.output_text.delta","delta":"hello"}),
                json!({"type":"response.completed","response":terminal}),
            ];
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").unwrap();
            for event in events {
                let packet = format!("data: {event}\n\n");
                for bytes in packet.as_bytes().chunks(3) {
                    socket.write_all(bytes).unwrap();
                }
            }
        });
        let adapter = CodexAdapter { base: Some(base) };
        let mut output = Vec::new();
        let record=forward(&adapter,"POST","/backend-api/codex/chat/completions",json!({"model":"fixture","messages":[{"role":"user","content":"hi"}],"stream":true,"stream_options":{"include_usage":true}}).to_string().as_bytes(),"fixture-token",Some(&json!({"account_id":"fixture-account"})),&mut output).unwrap().unwrap();
        fixture.join().unwrap();
        assert_eq!(record.input_tokens, 8);
        assert_eq!(record.output_tokens, 2);
        assert_eq!(record.status, Some(200));
        let wire = String::from_utf8(output).unwrap();
        assert!(wire.contains("hello"));
        assert!(wire.ends_with("data: [DONE]\n\n"));
        let chunks = wire
            .lines()
            .filter_map(|s| s.strip_prefix("data: "))
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .collect::<Vec<_>>();
        assert!(chunks.iter().all(|c| c["id"] == chunks[0]["id"]));
        assert_eq!(chunks.last().unwrap()["usage"]["total_tokens"], 10);
    }

    #[test]
    fn catalog_version_never_drops_below_the_floor() {
        assert_eq!(catalog_client_version(None), DEFAULT_CATALOG_CLIENT_VERSION);
        assert_eq!(
            catalog_client_version(Some("0.158.0")),
            DEFAULT_CATALOG_CLIENT_VERSION
        );
        assert_eq!(catalog_client_version(Some("0.160.2")), "0.160.2");
        assert_eq!(catalog_client_version(Some("1.0")), "1.0");
    }

    #[test]
    fn client_version_accepts_only_a_dotted_release() {
        assert_eq!(
            client_version_from_query("client_version=0.155.1"),
            Some("0.155.1")
        );
        assert_eq!(
            client_version_from_query("foo=1&client_version=0.156.0"),
            Some("0.156.0")
        );
        assert!(client_version_from_query("client_version=0.155.1%0aX").is_none());
        assert!(client_version_from_query("client_version=../0").is_none());
        assert!(client_version_from_query("client_version=").is_none());
        assert!(client_version_from_query("client_version=0..155").is_none());
    }

    #[test]
    fn codex_client_version_receives_the_native_catalog() {
        use std::io::{BufRead, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fixture = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(socket.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            assert!(
                request.contains(&format!("GET /backend-api/codex/models?client_version={DEFAULT_CATALOG_CLIENT_VERSION} HTTP/1.1"))
            );
            let body = r#"{"models":[{"slug":"gpt-6-sol","visibility":"list","display_name":"GPT-6-Sol"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).unwrap();
        });
        let adapter = CodexAdapter { base: Some(base) };
        let mut output = Vec::new();
        forward(
            &adapter,
            "GET",
            "/backend-api/codex/models?client_version=0.155.1",
            b"",
            "fixture-token",
            Some(&json!({"account_id":"fixture-account"})),
            &mut output,
        )
        .unwrap();
        fixture.join().unwrap();
        let wire = String::from_utf8(output).unwrap();
        let body = wire.split("\r\n\r\n").nth(1).unwrap();
        let value: Value = serde_json::from_str(body).unwrap();
        assert_eq!(value["models"][0]["slug"], "gpt-6-sol");
        assert_eq!(value["models"][0]["display_name"], "GPT-6-Sol");
        assert!(value.get("data").is_none());
    }

    #[test]
    fn catalog_without_client_version_stays_an_openai_list() {
        use std::io::{BufRead, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fixture = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(socket.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            assert!(request.contains(&format!(
                "GET /backend-api/codex/models?client_version={DEFAULT_CATALOG_CLIENT_VERSION} HTTP/1.1"
            )));
            let body = r#"{"models":[{"slug":"gpt-6-sol","visibility":"list"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).unwrap();
        });
        let adapter = CodexAdapter { base: Some(base) };
        let mut output = Vec::new();
        forward(
            &adapter,
            "GET",
            "/backend-api/codex/models",
            b"",
            "fixture-token",
            Some(&json!({"account_id":"fixture-account"})),
            &mut output,
        )
        .unwrap();
        fixture.join().unwrap();
        let wire = String::from_utf8(output).unwrap();
        let body = wire.split("\r\n\r\n").nth(1).unwrap();
        let value: Value = serde_json::from_str(body).unwrap();
        assert_eq!(value["object"], "list");
        assert_eq!(value["data"][0]["id"], "gpt-6-sol");
        assert!(value.get("models").is_none());
    }
}
