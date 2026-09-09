use super::CodexAdapter;
use crate::provider::{Provider, Upstream};
use fabrials_model::UsageRecord;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn responses_request(mut value: Value, chat: bool) -> Result<Value, String> {
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
    if chat {
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
                return Err(format!("Unsupported Codex Chat Completions field: {key}"));
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
        value = request;
    }
    value["store"] = json!(false);
    value["stream"] = json!(true);
    if value.get("instructions").is_none() {
        value["instructions"] = json!("");
    }
    Ok(value)
}

pub fn chat_response(response: &Value) -> Value {
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
fn json_reply(client: &mut dyn Write, status: u16, value: &Value) -> Result<(), String> {
    let body = value.to_string();
    write!(client,"HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).map_err(|_|"Client disconnected".into())
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
) -> Result<Option<UsageRecord>, String> {
    let started = std::time::Instant::now();
    let chat = path.ends_with("/chat/completions");
    let completion_id = format!("chatcmpl-{}", now_ms());
    let created = now_ms() / 1000;
    let mut record = UsageRecord {
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
    let endpoint = if method == "GET" {
        "/backend-api/codex/models?client_version=0.153.4"
    } else {
        "/backend-api/codex/responses"
    };
    let url = crate::forward::validated_upstream_url(base, endpoint)
        .map_err(|_| "Invalid Codex origin")?;
    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not initialize Codex proxy")?;
    let mut req = http.request(
        if method == "GET" {
            reqwest::Method::GET
        } else {
            reqwest::Method::POST
        },
        url,
    );
    for (key, value) in adapter.credential_headers(
        token,
        &Upstream {
            base: base.into(),
            path: path.into(),
            route: "codex",
            account_alias: None,
        },
        secret,
    )? {
        req = req.header(key, value);
    }
    if let Some(request) = request {
        req = req
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .body(request.to_string());
    }
    let response = req.send().map_err(|_| "Codex upstream unavailable")?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        record.status = Some(status);
        record.duration_ms = Some(started.elapsed().as_millis() as u64);
        json_reply(
            client,
            status,
            &json!({"error":{"message":"Codex rejected the request. Check authorization, model availability and supported parameters.","type":"upstream_error","code":status}}),
        )?;
        return Ok(Some(record));
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
        json_reply(client, 200, &json!({"object":"list","data":models}))?;
        return Ok(None);
    }
    if stream && write!(client,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n").is_err() {record.status=Some(499);return Ok(Some(record));}
    let mut reader = BufReader::new(response);
    let mut packet = Vec::new();
    let mut total = 0usize;
    let mut terminal = None;
    let mut tool_indices = std::collections::HashMap::new();
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
        let kind = event["type"].as_str().unwrap_or("");
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
        let result = chat_response(&terminal);
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
}
