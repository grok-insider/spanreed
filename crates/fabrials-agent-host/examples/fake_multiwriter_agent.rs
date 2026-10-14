#![allow(missing_docs)]

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};

fn emit(value: Value) {
    println!("{value}");
    std::io::stdout().flush().unwrap();
}
fn reply(id: Value, result: Value) {
    emit(json!({"jsonrpc":"2.0","id":id,"result":result}));
}

fn model_state(model: &str, effort: &str) -> Value {
    json!({"models":{"currentModelId":model,"availableModels":[{"modelId":"synthetic/custom","name":"Synthetic custom model"},{"modelId":"synthetic/alternate","name":"Synthetic alternate model"}]},"configOptions":[{"id":"reasoning_effort","currentValue":effort,"options":[{"value":"low","name":"Synthetic low"},{"value":"high","name":"Synthetic high"}]}]})
}

fn main() {
    let mut running = BTreeMap::<String, (Value, String)>::new();
    let mut accepted = Vec::<Value>::new();
    let mut created = 0u64;
    let catalogue = std::env::var("FAKE_MULTIWRITER_CATALOGUE").as_deref() == Ok("1");
    let images = std::env::var("FAKE_MULTIWRITER_IMAGES").as_deref() == Ok("1");
    let mut models = BTreeMap::<String, (String, String)>::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = message["id"].clone();
        let params = &message["params"];
        let session = params["sessionId"].as_str().unwrap_or("").to_owned();
        match message["method"].as_str().unwrap_or("") {
            "initialize" => reply(
                id,
                json!({"protocolVersion":1,"agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":images}},"authMethods":[],"_meta":{"agentVersion":"fake-multiwriter"}}),
            ),
            "session/load" => {
                if session == "history" {
                    emit(
                        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,"_meta":{"isReplay":true},"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"history"}}}}),
                    );
                }
                let (model, effort) = models
                    .entry(session.clone())
                    .or_insert_with(|| ("synthetic/custom".into(), "low".into()));
                let mut result = if catalogue {
                    model_state(model, effort)
                } else {
                    json!({})
                };
                result["_meta"] =
                    json!({"x.ai/runningPromptId":running.get(&session).map(|(_,id)|id)});
                reply(id, result);
                if session == "history" {
                    emit(
                        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" live"}}}}),
                    );
                }
            }
            "session/new" => {
                created += 1;
                reply(
                    id,
                    json!({"sessionId":format!("created-{created}"),"_meta":{}}),
                );
            }
            "session/prompt" => {
                let text = params["prompt"][0]["text"].as_str().unwrap_or("");
                if running.contains_key(&session) {
                    emit(
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"concurrent prompt"}}),
                    );
                    continue;
                }
                let prompt_id = params
                    .pointer("/_meta/promptId")
                    .and_then(Value::as_str)
                    .unwrap_or(text)
                    .to_owned();
                accepted.push(json!({"sessionId":session,"text":text,"promptId":prompt_id}));
                running.insert(session.clone(), (id, prompt_id));
                emit(
                    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":format!("accepted:{text}")}}}}),
                );
                if text == "permission" {
                    emit(
                        json!({"jsonrpc":"2.0","id":9001,"method":"session/request_permission","params":{"sessionId":session,"toolCall":{"toolCallId":"tool","title":"test"},"options":[{"optionId":"allow-once","name":"Allow","kind":"allow_once"},{"optionId":"reject-once","name":"Reject","kind":"reject_once"}]}}),
                    );
                }
            }
            "session/cancel" => {
                let target = params.pointer("/_meta/promptId").and_then(Value::as_str);
                if running
                    .get(&session)
                    .is_some_and(|(_, id)| target.is_none_or(|target| id == target))
                    && let Some((id, _)) = running.remove(&session)
                {
                    reply(id, json!({"stopReason":"cancelled"}));
                }
            }
            "test/release" => {
                if let Some((id, _)) = running.remove(&session) {
                    reply(id, json!({"stopReason":"end_turn"}));
                }
                reply(id, json!({}));
            }
            "session/set_model" | "session/set_config_option" if catalogue => {
                let (model, effort) = models
                    .entry(session.clone())
                    .or_insert_with(|| ("synthetic/custom".into(), "low".into()));
                if let Some(value) = params["modelId"].as_str() {
                    *model = value.into();
                }
                if params["configId"] == "reasoning_effort"
                    && let Some(value) = params["value"].as_str()
                {
                    *effort = value.into();
                }
                reply(id, model_state(model, effort));
            }
            "test/disable_models" => {
                emit(
                    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,"update":{"sessionUpdate":"config_option_update","configOptions":[{"id":"model","currentValue":"synthetic/custom","options":[]}]}}}),
                );
                reply(id, json!({}));
            }
            "test/state" => reply(
                id,
                json!({"running":running.keys().collect::<Vec<_>>(),"accepted":accepted,"created":created}),
            ),
            "" => {
                if id == 9001 {
                    accepted.push(json!({"permission":message["result"]}));
                }
            }
            _ => emit(
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"unsupported"}}),
            ),
        }
    }
}
