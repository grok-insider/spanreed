//! Deterministic ACP agent used as a test fixture.
//!
//! Speaks the subset of ACP that the host drives, so the message loop,
//! permission round trip, and recovery paths can be exercised without a real
//! model, without tokens, and without network access.
//!
//! Scripted behaviour, selected by the prompt text:
//!
//! - `permission` — emits a `session/request_permission` request offering the
//!   full native option set, then reports which option the client selected.
//! - `crash` — exits mid-turn without answering, to drive the interrupted path.
//! - anything else — streams two message deltas and completes.
//!
//! Environment:
//!
//! - `FAKE_AGENT_INIT_LOG` — append one line to this file per `initialize`.
//! - `FAKE_AGENT_EXIT_ONCE` — if this marker file does not exist, create it
//!   and exit right after answering `initialize`, so the first process dies
//!   and later ones stay up.

use std::io::{BufRead as _, Write as _};

// One scripted dispatch table; splitting it would obscure the script the
// fixture implements.
#[allow(clippy::too_many_lines)]
fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut pending_permission: Option<serde_json::Value> = None;
    let mut pending_interaction: Option<serde_json::Value> = None;
    let mut initialized_echo = false;
    let mut session_echo = false;

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };

        // A response to our own permission request.
        if message.get("method").is_none() {
            if message.get("id") == Some(&serde_json::json!(9002)) {
                if let Some(prompt) = pending_interaction.take() {
                    respond(
                        &mut stdout,
                        Some(prompt),
                        &serde_json::json!({"stopReason":"end_turn"}),
                    );
                }
            }
            if let Some(expected) = &pending_permission
                && message.get("id") == Some(expected)
            {
                let selected = message
                    .pointer("/result/outcome/optionId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("none")
                    .to_owned();
                pending_permission = None;
                emit(
                    &mut stdout,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": "s-1",
                            "update": {
                                "sessionUpdate": "agent_message_chunk",
                                "content": { "type": "text", "text": format!("selected:{selected}") }
                            }
                        }
                    }),
                );
            }
            continue;
        }

        let method = message
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let id = message.get("id").cloned();

        match method {
            "initialize" => {
                initialized_echo = message
                    .pointer("/params/clientCapabilities/_meta/x.ai~1userMessageEcho")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true);
                respond(
                    &mut stdout,
                    id,
                    &serde_json::json!({
                        "protocolVersion": 1,
                        "agentCapabilities": { "loadSession": true },
                        "authMethods": [{ "id": "cached_token", "name": "cached_token" }],
                        "_meta": { "agentVersion": "fake-1.0.0" }
                    }),
                );
                if let Some(log) = std::env::var_os("FAKE_AGENT_INIT_LOG") {
                    let mut file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(log)
                        .expect("open init log");
                    let _ = writeln!(file, "initialize {}", std::process::id());
                }
                if let Some(marker) = std::env::var_os("FAKE_AGENT_EXIT_ONCE") {
                    let marker = std::path::PathBuf::from(marker);
                    if !marker.exists() {
                        std::fs::write(&marker, b"exited").expect("write marker");
                        std::process::exit(0);
                    }
                }
            }
            "session/load" => {
                session_echo = initialized_echo
                    && message.pointer("/params/_meta/clientUserMessageEcho")
                        == Some(&serde_json::json!(true));
                let meta = if std::env::var_os("FAKE_AGENT_RESUME_RUNNING").is_some() {
                    serde_json::json!({"x.ai/runningPromptId":"active-prompt"})
                } else {
                    serde_json::json!({})
                };
                respond(&mut stdout, id, &serde_json::json!({"_meta":meta}));
                if let Some(_marker) = std::env::var_os("FAKE_AGENT_BREAK_PERMISSION_PIPE") {
                    let request_id = serde_json::json!(9001);
                    pending_permission = Some(request_id.clone());
                    emit(
                        &mut stdout,
                        &serde_json::json!({
                            "jsonrpc":"2.0", "id":request_id,
                            "method":"session/request_permission",
                            "params":{
                                "sessionId":"s-1",
                                "toolCall":{"toolCallId":"t-1", "title":"synthetic effect"},
                                "options":[
                                    {"optionId":"allow-once", "name":"Allow once", "kind":"allow_once"},
                                    {"optionId":"reject-once", "name":"Reject", "kind":"reject_once"}
                                ]
                            }
                        }),
                    );
                    #[cfg(unix)]
                    if !std::path::Path::new(&_marker).exists() {
                        use std::os::unix::process::CommandExt as _;
                        panic!(
                            "pipe fixture exec failed: {}",
                            std::process::Command::new("sh")
                                .args([
                                    "-c",
                                    "exec 0<&-; printf closed > \"$1\"; exec sleep 30",
                                    "fixture"
                                ])
                                .arg(_marker)
                                .exec()
                        );
                    }
                }
            }
            "session/new" => {
                session_echo = initialized_echo
                    && message.pointer("/params/_meta/clientUserMessageEcho")
                        == Some(&serde_json::json!(true));
                respond(&mut stdout, id, &serde_json::json!({ "sessionId": "s-1" }));
            }
            "test/replay" => {
                if let Some(events) = message
                    .pointer("/params/events")
                    .and_then(serde_json::Value::as_array)
                {
                    for event in events {
                        if event
                            .pointer("/params/update/sessionUpdate")
                            .and_then(serde_json::Value::as_str)
                            != Some("user_message_chunk")
                            || session_echo
                        {
                            emit(&mut stdout, event);
                        }
                    }
                }
                respond(&mut stdout, id, &serde_json::json!({}));
            }
            "session/prompt" => {
                let text = message
                    .pointer("/params/prompt/0/text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                if text == "inspect-images" {
                    use base64::Engine;
                    use sha2::Digest;
                    let images: Vec<_> = message
                        .pointer("/params/prompt")
                        .and_then(serde_json::Value::as_array)
                        .unwrap()
                        .iter()
                        .skip(1)
                        .collect();
                    let hashes: Vec<_> = images
                        .iter()
                        .map(|block| {
                            assert_eq!(block["type"], "image");
                            let bytes = base64::engine::general_purpose::STANDARD
                                .decode(block["data"].as_str().unwrap())
                                .unwrap();
                            let format = image::guess_format(&bytes).unwrap();
                            assert_eq!(block["mimeType"], format.to_mime_type());
                            image::load_from_memory(&bytes).unwrap();
                            format!("{:x}", sha2::Sha256::digest(&bytes))
                        })
                        .collect();
                    let summary = format!("images:{}:{}", images.len(), hashes.join(","));
                    for block in &images {
                        emit(
                            &mut stdout,
                            &serde_json::json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","_meta":{"promptId":message.pointer("/params/_meta/promptId")},"update":{"sessionUpdate":"user_message_chunk","content":block}}}),
                        );
                    }
                    emit(
                        &mut stdout,
                        &serde_json::json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":summary}}}}),
                    );
                    respond(
                        &mut stdout,
                        id,
                        &serde_json::json!({"stopReason":"end_turn"}),
                    );
                    continue;
                }
                if text == "drop-images" {
                    emit(
                        &mut stdout,
                        &serde_json::json!({"jsonrpc":"2.0","method":"_x.ai/session_notification","params":{"method":"x.ai/session_notification","params":{"sessionId":"s-1","update":{"sessionUpdate":"image_dropped","notes":["synthetic image rejection"]}}}}),
                    );
                }
                if text == "question" {
                    pending_interaction = id;
                    emit(
                        &mut stdout,
                        &serde_json::json!({"jsonrpc":"2.0","id":9002,"method":"_x.ai/ask_user_question","params":{"method":"x.ai/ask_user_question","params":{"sessionId":"s-1","toolCallId":"question-1","questions":[{"question":"Which?","options":[{"label":"A","description":"First"}]}],"mode":"default"}}}),
                    );
                    continue;
                }
                if text == "slow" {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                }
                if text.contains("crash") {
                    // Die mid-turn without answering the request.
                    std::process::exit(3);
                }
                if text.contains("permission") {
                    let request_id = serde_json::json!(9001);
                    pending_permission = Some(request_id.clone());
                    emit(
                        &mut stdout,
                        &serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": request_id,
                            "method": "session/request_permission",
                            "params": {
                                "sessionId": "s-1",
                                "toolCall": { "toolCallId": "t-1", "title": "write a file" },
                                "options": [
                                    { "optionId": "always-allow", "name": "Always allow", "kind": "allow_always" },
                                    { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                                    { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" },
                                    { "optionId": "reject-always", "name": "Reject always", "kind": "reject_always" }
                                ]
                            }
                        }),
                    );
                    // The turn stays open until the client answers.
                    respond(
                        &mut stdout,
                        id,
                        &serde_json::json!({ "stopReason": "end_turn" }),
                    );
                    continue;
                }
                if text == "echo-test" && session_echo {
                    emit(
                        &mut stdout,
                        &serde_json::json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"from terminal"}}}}),
                    );
                }
                if session_echo {
                    let own_id = message
                        .pointer("/params/_meta/promptId")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    if text == "echo-identity" {
                        emit_user(&mut stdout, text, serde_json::json!({}), false);
                        for external_id in ["terminal-1", "terminal-2"] {
                            let queue = serde_json::json!({"jsonrpc":"2.0","method":"_x.ai/queue/changed","params":{"sessionId":"s-1","entries":[],"runningPromptId":external_id,"runningText":text,"runningKind":"prompt"}});
                            emit(&mut stdout, &queue);
                            emit(&mut stdout, &queue);
                            emit(
                                &mut stdout,
                                &serde_json::json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":text}}}}),
                            );
                        }
                    }
                    emit(
                        &mut stdout,
                        &serde_json::json!({"jsonrpc":"2.0","method":"_x.ai/queue/changed","params":{"sessionId":"s-1","entries":[],"runningPromptId":own_id,"runningText":text,"runningKind":"prompt"}}),
                    );
                    if text == "echo-identity" {
                        emit_user(
                            &mut stdout,
                            text,
                            serde_json::json!({"promptId": "terminal-explicit"}),
                            false,
                        );
                        emit_user(
                            &mut stdout,
                            text,
                            serde_json::json!({"isReplay": true, "promptId": own_id}),
                            false,
                        );
                        emit_user(&mut stdout, text, serde_json::json!({}), true);
                        emit_user(&mut stdout, "echo-", serde_json::json!({}), false);
                        emit_user(&mut stdout, "identity", serde_json::json!({}), false);
                    } else {
                        emit_user(&mut stdout, text, serde_json::json!({}), false);
                    }
                }
                for chunk in ["hello ", "world"] {
                    emit(
                        &mut stdout,
                        &serde_json::json!({
                            "jsonrpc": "2.0",
                            "method": "session/update",
                            "params": {
                                "sessionId": "s-1",
                                "update": {
                                    "sessionUpdate": "agent_message_chunk",
                                    "content": { "type": "text", "text": chunk }
                                }
                            }
                        }),
                    );
                }
                respond(
                    &mut stdout,
                    id,
                    &serde_json::json!({ "stopReason": "end_turn" }),
                );
            }
            "session/cancel" => { /* notification: nothing to answer */ }
            _ => {
                if let Some(id) = id {
                    emit(
                        &mut stdout,
                        &serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32601, "message": "Method not found" }
                        }),
                    );
                }
            }
        }
    }
}

fn respond(out: &mut std::io::Stdout, id: Option<serde_json::Value>, result: &serde_json::Value) {
    let Some(id) = id else { return };
    emit(
        out,
        &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    );
}

fn emit_user(out: &mut std::io::Stdout, text: &str, meta: serde_json::Value, hidden: bool) {
    emit(
        out,
        &serde_json::json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s-1","_meta":meta,"update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":text},"_meta":{"hideFromScrollback":hidden}}}}),
    );
}

fn emit(out: &mut std::io::Stdout, value: &serde_json::Value) {
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}
