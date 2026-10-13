#![allow(missing_docs)]

use fabrials_agent_host::acp::{AgentCommand, AgentEvent, AgentHandle, AgentMode};
use fabrials_agent_host::dispatch::Workspace;
use fabrials_agent_host::journal::ReplayOutcome;
use fabrials_agent_host::protocol::Event;
use fabrials_agent_host::protocol::{CommandEnvelope, Operation, PROTOCOL_VERSION};
use fabrials_agent_host::remote::RemoteAccess;
use fabrials_agent_host::server::HostState;
use serde_json::{Value, json};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

fn command(operation: Operation, epoch: u64, key: &str) -> CommandEnvelope {
    CommandEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: key.into(),
        idempotency_key: Some(key.into()),
        controller_epoch: Some(epoch),
        expected_revision: None,
        deadline_ms: None,
        operation,
    }
}

async fn permission(
    events: &mut mpsc::Receiver<AgentEvent>,
    state: &HostState,
    browser: &RemoteAccess,
    epoch: u64,
    expected: &str,
) -> Result<(Value, String), String> {
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    let mut updates = 0;
    let result = tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Some(AgentEvent::PermissionRequest { request_id, params }) => {
                        let received = params.pointer("/toolCall/rawInput/command").and_then(Value::as_str).unwrap_or("");
                        if received.trim() != expected { return Err("unexpected_effect_refused".into()); }
                        let projected = tokio::time::timeout(Duration::from_secs(3), async {
                            loop {
                                let session = state.session.lock().await;
                                if let Some((key, _)) = session.permission_details.iter().find(|(_, (_, description))| description.as_deref().is_some_and(|value| value.trim() == expected)) {
                                    return key.clone();
                                }
                                drop(session);
                                tokio::time::sleep(Duration::from_millis(10)).await;
                            }
                        }).await.map_err(|_| "remote_permission_missing")?;
                        return Ok((request_id, projected));
                    }
                    Some(AgentEvent::Exited) | None => return Err("terminal_client_closed".into()),
                    Some(_) => { updates += 1; }
                },
                _ = interval.tick() => {
                    browser.heartbeat(epoch).await.map_err(str::to_owned)?;
                    println!("{}", json!({"milestone":"waiting_permission","terminalEvents":updates,"remotePending":state.session.lock().await.pending_permissions.len()}));
                }
            }
        }
    }).await;
    result.unwrap_or_else(|_| Err(format!("permission_timeout_terminal_events_{updates}")))
}

fn prompt(marker: &str) -> String {
    format!(
        "This is an isolated transport acceptance test. Use exactly one shell command: printf '{marker}\\n' >> effect.txt . Do not change the command. It must ask for approval. Then reply DONE. Run no other commands and do not change permissions."
    )
}

async fn receipt(browser: &RemoteAccess, epoch: u64, key: &str) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let reply = browser
                .command(command(
                    Operation::GetCommandStatus {
                        idempotency_key: key.into(),
                    },
                    epoch,
                    "read-receipt",
                ))
                .await;
            let state = reply.body["result"]["state"].as_str().unwrap_or("missing");
            if state == "completed" {
                return Ok(());
            }
            if state != "running" {
                return Err(format!("receipt_{state}"));
            }
            browser.heartbeat(epoch).await.map_err(str::to_owned)?;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| "receipt_timeout")?
}

async fn unsupported_contract() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    let command = AgentCommand::new(std::env::current_exe()?.to_string_lossy().into_owned())
        .with_mode(AgentMode::Shared)
        .with_working_directory(workspace.path());
    let (client, _events) = AgentHandle::spawn(&command)?;
    let result = tokio::time::timeout(Duration::from_secs(3), client.initialize()).await?;
    if result.is_ok() {
        return Err("unsupported executable unexpectedly initialized".into());
    }
    if workspace.path().join("effect.txt").exists() {
        return Err("unsupported executable produced an effect".into());
    }
    let _ = client.shutdown().await;
    println!(
        "{}",
        json!({"milestone":"unsupported_cli","scope":"synthetic executable refuses shared mode before initialize","passed":true})
    );
    Ok(())
}

async fn finish_prompt(
    future: impl std::future::Future<Output = Result<Value, fabrials_agent_host::acp::AcpError>>,
    access: &RemoteAccess,
    epoch: u64,
) -> Result<(), String> {
    tokio::pin!(future);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(3));
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            tokio::select! {
                result = &mut future => return result.map(|_| ()).map_err(|error| format!("prompt_failed:{error}")),
                _ = heartbeat.tick() => access.heartbeat(epoch).await.map_err(str::to_owned)?,
            }
        }
    }).await.map_err(|_| "prompt_completion_timeout")?
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("agent") {
        eprintln!("unsupported_shared_mode");
        std::process::exit(64);
    }
    unsupported_contract().await?;
    if std::env::args().nth(1).as_deref() == Some("--unsupported-contract") {
        return Ok(());
    }
    let key = std::env::var("FABRIALS_LOCAL_DEV_KEY").map_err(
        |_| "FABRIALS_LOCAL_DEV_KEY is required; this qualification uses real inference",
    )?;
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().join("workspace");
    let home = directory.path().join("home");
    let grok_home = home.join(".grok");
    let config = home.join(".config");
    let host_directory = directory.path().join("host");
    for path in [&workspace, &grok_home, &config, &host_directory] {
        std::fs::create_dir_all(path)?;
    }
    let socket = grok_home.join("leader.sock");
    let mut env: Vec<(String, String)> = [
        ("GROK_TELEMETRY_ENABLED", "false"),
        ("GROK_TELEMETRY_MIXPANEL_ENABLED", "false"),
        ("GROK_TELEMETRY_TRACE_UPLOAD", "false"),
        ("GROK_FEEDBACK_ENABLED", "false"),
        ("GROK_TRACE_UPLOAD", "false"),
        ("GROK_INSTRUMENTATION", "disabled"),
        ("OTEL_SDK_DISABLED", "true"),
        ("DISABLE_TELEMETRY", "1"),
        ("DISABLE_FEEDBACK_COMMAND", "1"),
        ("GROK_DISABLE_AUTOUPDATER", "1"),
        ("GROK_PROMPT_SUGGESTIONS", "false"),
        ("GROK_TURN_SUMMARY", "0"),
        ("GROK_DEFAULT_PERMISSION_MODE", "ask"),
        ("GROK_AGENT_ID", "remote-qualification"),
        ("GROK_XAI_API_BASE_URL", "https://ai.fabrials.com/v1"),
        ("GROK_CLI_CHAT_PROXY_BASE_URL", "https://ai.fabrials.com/v1"),
        ("GROK_MODELS_BASE_URL", "https://ai.fabrials.com/v1"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect();
    for (key, path) in [
        ("HOME", &home),
        ("USERPROFILE", &home),
        ("GROK_HOME", &grok_home),
        ("XDG_CONFIG_HOME", &config),
        ("GROK_LEADER_SOCKET", &socket),
    ] {
        env.push((key.into(), path.to_string_lossy().into_owned()));
    }
    for name in [
        "XAI_API_KEY",
        "GROK_CODE_XAI_API_KEY",
        "AI_RELAY_VIRTUAL_KEY",
        "FABRIALS_LOCAL_DEV_KEY",
        "GROK_API_KEY",
    ] {
        env.push((name.into(), key.clone()));
    }
    let program = std::env::var("FABRIALS_AGENT_PROGRAM").unwrap_or_else(|_| "grok".into());
    let mut leader = tokio::process::Command::new(&program)
        .args([
            "agent",
            "-m",
            "grok-4.6-build",
            "leader",
            "--no-exit-on-disconnect",
            "--relay-on-demand",
            "--no-auto-update",
        ])
        .envs(env.iter().cloned())
        .current_dir(&workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    tokio::time::timeout(Duration::from_secs(15), async {
        while !socket.exists() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?;
    let agent_command = AgentCommand::new(program)
        .with_mode(AgentMode::Shared)
        .with_working_directory(&workspace)
        .with_env(env);
    let (terminal, mut terminal_events) = AgentHandle::spawn(&agent_command)?;
    let init = terminal.initialize().await?;
    println!(
        "Official Grok {}",
        init.agent_version().unwrap_or("unknown")
    );
    let (session_id, _) = terminal.new_session(&workspace).await?;
    let (adapter, events) = AgentHandle::spawn(&agent_command)?;
    adapter.initialize().await?;
    let state = Arc::new(
        HostState::new(fabrials_agent_host::origin::LocalOrigin::new(
            "0123456789abcdef0123456789abcdef",
            21001,
        )?)
        .with_agent_mode(AgentMode::Shared)
        .with_persistence(
            &host_directory,
            Arc::new(fabrials_agent_host::picker::UnavailableDirectoryPicker),
        )?,
    );
    state.session.lock().await.workspaces.insert(
        "synthetic".into(),
        Workspace {
            id: "synthetic".into(),
            display_name: "Synthetic workspace".into(),
            path: workspace.clone(),
        },
    );
    state.attach_agent(adapter.clone(), events);
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.agent.lock().await.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let mut replacement = None;
    let result = tokio::time::timeout(Duration::from_secs(200), async {
        let nonce = state.pairing.lock().await.mint_nonce(fabrials_agent_host::now_ms()).unwrap();
        let (browser, grant) = RemoteAccess::pair(state.clone(), nonce.expose()).await.unwrap();
        let epoch = browser.connect().await.unwrap();
        let loaded = browser.command(command(Operation::LoadSession { workspace_id:"synthetic".into(), session_id:session_id.clone() }, epoch, "load-terminal-session")).await;
        assert_eq!(loaded.status, 202, "{}", loaded.body);
        let nonce = state.pairing.lock().await.mint_nonce(fabrials_agent_host::now_ms()).unwrap();
        let (observer, _) = RemoteAccess::pair(state.clone(), nonce.expose()).await.unwrap();
        assert_eq!(observer.connect().await, Err("controller_held"));
        let operation = Operation::Prompt { session_id:session_id.clone(), text:prompt("REMOTE_ONCE"), bash:false };
        let refused = observer.command(command(operation.clone(), epoch, "observer-forbidden")).await;
        assert_eq!(refused.status, 409);
        assert!(!workspace.join("effect.txt").exists());
        let accepted = browser.command(command(operation.clone(), epoch, "remote-turn")).await;
        assert_eq!(accepted.status, 202);
        let (native_id, _) = permission(&mut terminal_events, &state, &browser, epoch, "printf 'REMOTE_ONCE\\n' >> effect.txt").await?;
        let ReplayOutcome::Replay(observed) = observer.events(0).await.unwrap() else { return Err("observer_replay_missing".to_owned()); };
        assert!(observed.iter().any(|row| matches!(&row.event, Event::PermissionRequest { session_id: id, .. } if id == &session_id)));
        println!("{}", json!({"milestone":"remote_to_terminal_permission_and_observer","passed":true}));
        browser.disconnect().await;
        let resumed = RemoteAccess::authenticate(state.clone(), grant.session_token, grant.csrf_token).await.unwrap();
        let epoch = resumed.connect().await.unwrap();
        assert!(!state.session.lock().await.pending_permissions.is_empty());
        terminal.answer_permission(&native_id, "allow-once").await.unwrap();
        receipt(&resumed, epoch, "remote-turn").await?;
        let retry = resumed.command(command(operation, epoch, "remote-turn")).await;
        assert_eq!(retry.body["result"]["state"], "completed");
        assert_eq!(std::fs::read_to_string(workspace.join("effect.txt")).unwrap(), "REMOTE_ONCE\n");
        println!("{}", json!({"milestone":"reconnect_and_no_replay","passed":true}));
        let terminal_turn_text = prompt("TERMINAL_ONCE");
        let terminal_turn = terminal.prompt(&session_id, &terminal_turn_text);
        tokio::pin!(terminal_turn);
        let (_, projected_id) = {
            let permission_wait = permission(&mut terminal_events, &state, &resumed, epoch, "printf 'TERMINAL_ONCE\\n' >> effect.txt");
            tokio::pin!(permission_wait);
            tokio::select! {
                permission = &mut permission_wait => permission?,
                result = &mut terminal_turn => return Err(format!("terminal_turn_completed_without_permission:{result:?}")),
            }
        };
        let decided = resumed.command(command(Operation::DecidePermission { session_id:session_id.clone(), request_id:projected_id, option_id:"allow-once".into() }, epoch, "approve-terminal-turn")).await;
        assert_eq!(decided.status, 202, "{}", decided.body);
        finish_prompt(terminal_turn, &resumed, epoch).await?;
        assert_eq!(std::fs::read_to_string(workspace.join("effect.txt")).unwrap(), "REMOTE_ONCE\nTERMINAL_ONCE\n");
        let ReplayOutcome::Replay(observed) = resumed.events(0).await.unwrap() else { return Err("terminal_echo_missing".to_owned()); };
        assert!(observed.iter().any(|row| matches!(&row.event, Event::UserMessageDelta { text, .. } if text.contains("TERMINAL_ONCE"))));
        println!("{}", json!({"milestone":"terminal_to_remote_prompt_and_permission","passed":true}));
        let loss_turn_text = prompt("ADAPTER_LOSS_ONCE");
        let loss_turn = terminal.prompt(&session_id, &loss_turn_text);
        tokio::pin!(loss_turn);
        let (native_id, _) = {
            let permission_wait = permission(&mut terminal_events, &state, &resumed, epoch, "printf 'ADAPTER_LOSS_ONCE\\n' >> effect.txt");
            tokio::pin!(permission_wait);
            tokio::select! {
                permission = &mut permission_wait => permission?,
                result = &mut loss_turn => return Err(format!("loss_turn_completed_without_permission:{result:?}")),
            }
        };
        adapter.shutdown().await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while state.agent.lock().await.is_some() { tokio::time::sleep(Duration::from_millis(10)).await; }
        }).await.map_err(|_| "adapter_loss_not_observed")?;
        terminal.answer_permission(&native_id, "allow-once").await.unwrap();
        finish_prompt(loss_turn, &resumed, epoch).await?;
        let (new_adapter, new_events) = AgentHandle::spawn(&agent_command).unwrap();
        new_adapter.initialize().await.unwrap();
        state.attach_agent(new_adapter.clone(), new_events);
        replacement = Some(new_adapter);
        tokio::time::timeout(Duration::from_secs(3), async {
            while state.agent.lock().await.is_none() { tokio::time::sleep(Duration::from_millis(10)).await; }
        }).await.map_err(|_| "replacement_not_attached")?;
        resumed.heartbeat(epoch).await.map_err(str::to_owned)?;
        let loaded = resumed.command(command(Operation::LoadSession { workspace_id:"synthetic".into(), session_id:session_id.clone() }, epoch, "load-after-adapter-loss")).await;
        assert_eq!(loaded.status, 202, "{}", loaded.body);
        assert_eq!(std::fs::read_to_string(workspace.join("effect.txt")).unwrap(), "REMOTE_ONCE\nTERMINAL_ONCE\nADAPTER_LOSS_ONCE\n");
        assert!(state.session.lock().await.pending_permissions.is_empty());
        println!("{}", json!({"milestone":"adapter_loss_preserves_terminal_and_reconciles","passed":true}));
        println!("PASS: official shared ACP matrix, three exact effects, both directions, observer, reconnect, adapter loss");
        Ok::<(), String>(())
    }).await;
    state.request_shutdown();
    if let Some(replacement) = replacement {
        let _ = replacement.shutdown().await;
    }
    terminal.shutdown().await?;
    adapter.shutdown().await?;
    leader.kill().await?;
    leader.wait().await?;
    result??;
    Ok(())
}
