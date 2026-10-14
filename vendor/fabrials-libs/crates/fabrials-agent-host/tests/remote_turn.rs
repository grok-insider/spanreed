#![allow(missing_docs)]

use fabrials_agent_host::acp::{AgentCommand, AgentHandle, AgentMode};
use fabrials_agent_host::dispatch::{LiveSession, Workspace};
use fabrials_agent_host::protocol::{
    CommandEnvelope, Event, MULTI_WRITER_PROTOCOL_VERSION, Operation,
};
use fabrials_agent_host::remote::RemoteAccess;
use fabrials_agent_host::server::HostState;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn command(operation: Operation, epoch: u64, key: &str) -> CommandEnvelope {
    CommandEnvelope {
        protocol_version: MULTI_WRITER_PROTOCOL_VERSION,
        request_id: key.into(),
        idempotency_key: Some(key.into()),
        controller_epoch: Some(epoch),
        expected_revision: None,
        deadline_ms: None,
        operation,
    }
}

#[tokio::test]
async fn loading_an_already_live_session_does_not_erase_the_streamed_transcript() {
    let (state, agent, access, _, _, _directory) = setup().await;
    let epoch = access.connect_version(4).await.unwrap();
    state
        .emit_event(
            Event::MessageDelta {
                session_id: "s-1".into(),
                text: "Synthetic live message remains visible".into(),
            },
            None,
        )
        .await;
    let before = state.journal.lock().await.emitted_through();
    let reply = access
        .command(command(
            Operation::LoadSession {
                workspace_id: "ws".into(),
                session_id: "s-1".into(),
            },
            epoch,
            "reopen-live",
        ))
        .await;
    assert_eq!(reply.status, 202);
    assert_eq!(reply.body["result"]["sessionId"], "s-1");
    let fabrials_agent_host::journal::ReplayOutcome::Replay(events) =
        access.events(before).await.unwrap()
    else {
        panic!("replay expected");
    };
    assert!(!events.iter().any(
        |row| matches!(&row.event, Event::SessionSnapshot { session_id, .. } if session_id == "s-1")
    ));
    assert!(events.iter().any(|row| matches!(&row.event, Event::SessionStatus { session_id, state } if session_id == "s-1" && state == "idle")));
    access.disconnect().await;
    agent.shutdown().await.unwrap();
}

async fn setup() -> (
    Arc<HostState>,
    Arc<AgentHandle>,
    RemoteAccess,
    String,
    String,
    tempfile::TempDir,
) {
    let directory = tempfile::tempdir().unwrap();
    let state = Arc::new(
        HostState::new(
            fabrials_agent_host::origin::LocalOrigin::new(
                "0123456789abcdef0123456789abcdef",
                21001,
            )
            .unwrap(),
        )
        .with_agent_mode(AgentMode::Shared)
        .with_persistence(
            directory.path(),
            Arc::new(fabrials_agent_host::picker::UnavailableDirectoryPicker),
        )
        .unwrap(),
    );
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("examples");
    path.push(format!("fake_agent{}", std::env::consts::EXE_SUFFIX));
    let (agent, events) = AgentHandle::spawn(&AgentCommand::new(path.to_string_lossy())).unwrap();
    agent.initialize().await.unwrap();
    agent.new_session(directory.path()).await.unwrap();
    state.attach_agent(agent.clone(), events);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let attached = state.agent.lock().await.is_some();
            let reconciled = match state.journal.lock().await.replay_after(0) {
                fabrials_agent_host::journal::ReplayOutcome::Replay(events) => events
                    .iter()
                    .any(|event| matches!(event.event, Event::WorkspacesChanged)),
                fabrials_agent_host::journal::ReplayOutcome::SnapshotRequired => false,
            };
            if attached && reconciled {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    {
        let mut session = state.session.lock().await;
        session.workspaces.insert(
            "ws".into(),
            Workspace {
                id: "ws".into(),
                display_name: "Workspace".into(),
                path: directory.path().into(),
            },
        );
        session
            .open_session(LiveSession {
                id: "s-1".into(),
                workspace_id: "ws".into(),
                workspace_name: "Workspace".into(),
                running: false,
                queued: vec![],
                opened_at_ms: 1,
            })
            .unwrap();
    }
    let nonce = state
        .pairing
        .lock()
        .await
        .mint_nonce(fabrials_agent_host::now_ms())
        .unwrap();
    let (access, paired) = RemoteAccess::pair(state.clone(), nonce.expose())
        .await
        .unwrap();
    (
        state,
        agent,
        access,
        paired.session_token,
        paired.csrf_token,
        directory,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turn_finishes_after_browser_disconnect_and_retry_never_repeats_output() {
    let (state, agent, access, token, csrf, _directory) = setup().await;
    let epoch = access.connect_version(4).await.unwrap();
    let operation = Operation::Prompt {
        session_id: "s-1".into(),
        text: "slow".into(),
        bash: false,
    };
    let started = Instant::now();
    let receipt = access
        .command(command(operation.clone(), epoch, "slow-turn"))
        .await;
    assert_eq!(receipt.body["result"]["outcome"], "commandAccepted");
    assert!(started.elapsed() < Duration::from_millis(300));
    access.disconnect().await;
    let resumed = RemoteAccess::authenticate(state.clone(), token, csrf)
        .await
        .unwrap();
    let next = resumed.connect_version(4).await.unwrap();
    let status = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let response = resumed
                .command(command(
                    Operation::GetCommandStatus {
                        idempotency_key: "slow-turn".into(),
                    },
                    next,
                    "status",
                ))
                .await;
            if !matches!(
                response.body["result"]["state"].as_str(),
                Some("running" | "queued")
            ) {
                break response;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(status.body["result"]["state"], "completed");
    let retry = resumed.command(command(operation, next, "slow-turn")).await;
    assert_eq!(retry.body["result"]["state"], "completed");
    let events = match resumed.events(0).await.unwrap() {
        fabrials_agent_host::journal::ReplayOutcome::Replay(events) => events,
        _ => panic!("unexpected reset"),
    };
    assert_eq!(
        events
            .iter()
            .filter(
                |event| matches!(&event.event, Event::MessageDelta { text, .. } if text == "world")
            )
            .count(),
        1
    );
    let prompts: Vec<_> = events
        .iter()
        .filter(|event| {
            matches!(
                &event.event,
                Event::PromptSent { session_id, text, idempotency_key, .. }
                    if session_id == "s-1" && text == "slow" && idempotency_key.is_none()
            )
        })
        .collect();
    assert_eq!(prompts.len(), 1);
    let first_reply = events
        .iter()
        .find(|event| matches!(event.event, Event::MessageDelta { .. }))
        .unwrap();
    assert!(prompts[0].event_sequence < first_reply.event_sequence);
    agent.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_question_is_answered_once_with_original_rpc_id() {
    let (state, agent, access, _, _, _directory) = setup().await;
    let epoch = access.connect_version(4).await.unwrap();
    let receipt = access
        .command(command(
            Operation::Prompt {
                session_id: "s-1".into(),
                text: "question".into(),
                bash: false,
            },
            epoch,
            "question-turn",
        ))
        .await;
    assert_eq!(receipt.status, 202);
    let request_id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(key) = state
                .session
                .lock()
                .await
                .pending_interactions
                .keys()
                .next()
                .cloned()
            {
                break key;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let operation = Operation::AnswerInteraction {
        session_id: "s-1".into(),
        request_id,
        answer: serde_json::json!({"outcome":"accepted","answers":{"Which?":["A"]}}),
    };
    assert_eq!(
        access
            .command(command(operation.clone(), epoch, "answer"))
            .await
            .status,
        202
    );
    assert_eq!(
        access
            .command(command(operation, epoch, "answer-again"))
            .await
            .status,
        422
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let response = access
                .command(command(
                    Operation::GetCommandStatus {
                        idempotency_key: "question-turn".into(),
                    },
                    epoch,
                    "status",
                ))
                .await;
            if response.body["result"]["state"] == "completed" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    agent.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_user_text_is_visible_and_the_browser_echo_is_suppressed() {
    let (state, agent, access, _, _, _directory) = setup().await;
    let epoch = access.connect_version(4).await.unwrap();
    access
        .command(command(
            Operation::Prompt {
                session_id: "s-1".into(),
                text: "echo-test".into(),
                bash: false,
            },
            epoch,
            "echo",
        ))
        .await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let response = access
                .command(command(
                    Operation::GetCommandStatus {
                        idempotency_key: "echo".into(),
                    },
                    epoch,
                    "status",
                ))
                .await;
            if response.body["result"]["state"] == "completed" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let events = match state.journal.lock().await.replay_after(0) {
        fabrials_agent_host::journal::ReplayOutcome::Replay(events) => events,
        _ => panic!("unexpected reset"),
    };
    let text: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.event {
            Event::UserMessageDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, vec!["from terminal"]);
    agent.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn captured_terminal_turn_requires_echo_opt_in_and_replays_once_in_order() {
    let (state, agent, access, token, csrf, directory) = setup().await;
    access.connect_version(4).await.unwrap();
    agent.load_session("s-1", directory.path()).await.unwrap();
    let raw = include_str!("fixtures/terminal_turn_1_0_50.json")
        .replace("01a129d4-ce4b-7991-945d-5400d4b5eee4", "s-1");
    let recorded: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let prompt = recorded[2]["params"]["update"]["content"]["text"]
        .as_str()
        .unwrap();
    agent
        .request("test/replay", serde_json::json!({"events":recorded}))
        .await
        .unwrap();
    let events = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let fabrials_agent_host::journal::ReplayOutcome::Replay(events) = state.journal.lock().await.replay_after(0)
                && events.iter().any(|entry| matches!(&entry.event, Event::MessageDelta { text, .. } if text == "DONE"))
                && events.iter().any(|entry| matches!(&entry.event, Event::SessionStatus { state, .. } if state == "idle"))
            { break events; }
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    let users: Vec<_> = events
        .iter()
        .filter(
            |entry| matches!(&entry.event, Event::UserMessageDelta { text, .. } if text == prompt),
        )
        .collect();
    assert_eq!(users.len(), 1);
    let reply = events
        .iter()
        .find(|entry| matches!(&entry.event, Event::MessageDelta { .. }))
        .unwrap();
    let tool = events
        .iter()
        .find(|entry| matches!(&entry.event, Event::ToolStart { .. }))
        .unwrap();
    assert!(users[0].event_sequence < reply.event_sequence);
    assert!(reply.event_sequence < tool.event_sequence);
    let sequence = events.last().unwrap().event_sequence;
    access.disconnect().await;
    let resumed = RemoteAccess::authenticate(state, token, csrf)
        .await
        .unwrap();
    resumed.connect_version(4).await.unwrap();
    let replay = match resumed.events(0).await.unwrap() {
        fabrials_agent_host::journal::ReplayOutcome::Replay(events) => events,
        _ => panic!("unexpected reset"),
    };
    assert_eq!(replay.iter().filter(|entry| matches!(&entry.event, Event::UserMessageDelta { text, .. } if text == prompt)).count(), 1);
    let recent = match resumed.events(sequence).await.unwrap() {
        fabrials_agent_host::journal::ReplayOutcome::Replay(events) => events,
        _ => panic!("unexpected reset"),
    };
    assert!(
        !recent
            .iter()
            .any(|entry| matches!(&entry.event, Event::UserMessageDelta { .. }))
    );
    agent.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identical_external_prompts_are_not_confused_with_the_pending_browser_prompt() {
    let (state, agent, access, _, _, _directory) = setup().await;
    let epoch = access.connect_version(4).await.unwrap();
    agent.request("test/replay", serde_json::json!({"events":[{
        "jsonrpc":"2.0", "method":"_x.ai/queue/changed", "params":{
            "sessionId":"s-1", "entries":[], "runningPromptId":"terminal-before-request", "runningKind":"prompt", "runningText":"echo-identity"
        }
    }]})).await.unwrap();
    access
        .command(command(
            Operation::Prompt {
                session_id: "s-1".into(),
                text: "echo-identity".into(),
                bash: false,
            },
            epoch,
            "identity",
        ))
        .await;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let response = access
                .command(command(
                    Operation::GetCommandStatus {
                        idempotency_key: "identity".into(),
                    },
                    epoch,
                    "status",
                ))
                .await;
            if response.body["result"]["state"] == "completed" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let events = match state.journal.lock().await.replay_after(0) {
        fabrials_agent_host::journal::ReplayOutcome::Replay(events) => events,
        _ => panic!("unexpected reset"),
    };
    let users: Vec<_> = events
        .iter()
        .filter_map(|entry| match &entry.event {
            Event::UserMessageDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(users, vec!["echo-identity"; 5]);
    assert_eq!(events.iter().filter(|entry| matches!(&entry.event, Event::PromptSent { text, .. } if text == "echo-identity")).count(), 1);
    agent.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_reconciliation_preserves_the_running_terminal_turn() {
    let directory = tempfile::tempdir().unwrap();
    let state = Arc::new(
        HostState::new(
            fabrials_agent_host::origin::LocalOrigin::new(
                "0123456789abcdef0123456789abcdef",
                21001,
            )
            .unwrap(),
        )
        .with_agent_mode(AgentMode::Shared),
    );
    {
        let mut sessions = state.session.lock().await;
        sessions.workspaces.insert(
            "workspace".into(),
            Workspace {
                id: "workspace".into(),
                display_name: "Workspace".into(),
                path: directory.path().into(),
            },
        );
        sessions
            .open_session(LiveSession {
                id: "existing-terminal".into(),
                workspace_id: "workspace".into(),
                workspace_name: "Workspace".into(),
                running: false,
                queued: Vec::new(),
                opened_at_ms: 0,
            })
            .unwrap();
    }
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("examples");
    path.push(format!("fake_agent{}", std::env::consts::EXE_SUFFIX));
    let command = AgentCommand::new(path.to_string_lossy())
        .with_env([("FAKE_AGENT_RESUME_RUNNING".into(), "1".into())]);
    let (agent, events) = AgentHandle::spawn(&command).unwrap();
    agent.initialize().await.unwrap();
    state.attach_agent(agent.clone(), events);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let replay = state.journal.lock().await.replay_after(0);
            if let fabrials_agent_host::journal::ReplayOutcome::Replay(events) = replay {
                if events.iter().any(|event| matches!(&event.event, Event::SessionStatus {session_id, state} if session_id == "existing-terminal" && state == "running")) {
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    let sessions = state.session.lock().await;
    assert_eq!(sessions.sessions.len(), 1);
    assert!(sessions.sessions["existing-terminal"].running);
    drop(sessions);
    agent.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broken_approval_pipe_restarts_adapter_and_reloads_without_resending() {
    let (state, agent, access, _, _, directory) = setup().await;
    agent.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while Arc::strong_count(&agent) > 1 || state.agent.lock().await.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let marker = directory.path().join("closed-stdin");
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("examples");
    path.push(format!("fake_agent{}", std::env::consts::EXE_SUFFIX));
    let supervisor = state.supervise_agent(
        AgentCommand::new(path.to_string_lossy())
            .with_mode(AgentMode::Shared)
            .with_env([(
                "FAKE_AGENT_BREAK_PERMISSION_PIPE".into(),
                marker.to_string_lossy().into_owned(),
            )]),
        fabrials_agent_host::server::RelaunchPolicy {
            initial_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_millis(50),
            stable_after: Duration::from_secs(1),
        },
    );
    let old_id = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if marker.exists() {
                if let Some(id) = state.session.lock().await.pending_permissions.keys().next() {
                    break id.clone();
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let epoch = access.connect_version(4).await.unwrap();
    let decision = |id: String| Operation::DecidePermission {
        session_id: "s-1".into(),
        request_id: id,
        option_id: "allow-once".into(),
    };
    let failed = access
        .command(command(decision(old_id.clone()), epoch, "broken-answer"))
        .await;
    assert_ne!(failed.status, 202);
    let recovered = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(id) = state
                .session
                .lock()
                .await
                .pending_permissions
                .keys()
                .find(|id| **id != old_id)
            {
                break id.clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    if recovered.is_err() {
        state.request_shutdown();
        supervisor.await.unwrap();
        panic!("replacement adapter must reload the pending approval");
    }
    let recovered = recovered.expect("replacement adapter must reload the pending approval");
    assert_eq!(
        access
            .command(command(decision(old_id), epoch, "stale-answer"))
            .await
            .status,
        422
    );
    assert!(!matches!(
        state.journal.lock().await.replay_after(0),
        fabrials_agent_host::journal::ReplayOutcome::Replay(events)
            if events.iter().any(|item| matches!(&item.event,
                Event::MessageDelta { text, .. } if text == "selected:allow-once"))
    ));
    assert_eq!(
        access
            .command(command(decision(recovered), epoch, "fresh-answer"))
            .await
            .status,
        202
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let fabrials_agent_host::journal::ReplayOutcome::Replay(events) =
                state.journal.lock().await.replay_after(0)
            {
                let selected = events
                    .iter()
                    .filter(|item| {
                        matches!(&item.event,
                    Event::MessageDelta { text, .. } if text == "selected:allow-once")
                    })
                    .count();
                if selected > 0 {
                    assert_eq!(selected, 1);
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    state.request_shutdown();
    supervisor.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn observers_are_notified_when_sessions_open_and_close() {
    let (state, agent, owner, token, csrf, _directory) = setup().await;
    let epoch = owner.connect_version(4).await.unwrap();
    let observer = RemoteAccess::authenticate(state.clone(), token, csrf)
        .await
        .unwrap();
    assert_eq!(observer.connect_version(4).await, Ok(0));
    for (key, operation) in [
        (
            "close-first",
            Operation::CloseSession {
                session_id: "s-1".into(),
            },
        ),
        (
            "load",
            Operation::LoadSession {
                workspace_id: "ws".into(),
                session_id: "s-1".into(),
            },
        ),
        (
            "close-second",
            Operation::CloseSession {
                session_id: "s-1".into(),
            },
        ),
        (
            "create",
            Operation::CreateSession {
                workspace_id: "ws".into(),
            },
        ),
    ] {
        let before = state.journal.lock().await.emitted_through();
        let result = owner.command(command(operation, epoch, key)).await;
        assert_eq!(result.status, 202, "{key}");
        let fabrials_agent_host::journal::ReplayOutcome::Replay(events) =
            observer.events(before).await.unwrap()
        else {
            panic!("observer cursor remains replayable");
        };
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.event, Event::WorkspacesChanged))
                .count(),
            1,
            "{key}"
        );
    }
    owner.disconnect().await;
    agent.shutdown().await.unwrap();
}
