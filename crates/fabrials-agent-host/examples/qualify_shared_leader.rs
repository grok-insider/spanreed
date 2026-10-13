#![allow(missing_docs)]

use fabrials_agent_host::acp::{AgentCommand, AgentEvent, AgentHandle, AgentMode};
use fabrials_agent_host::dispatch::Workspace;
use fabrials_agent_host::protocol::{CommandEnvelope, Operation, PROTOCOL_VERSION};
use fabrials_agent_host::remote::RemoteAccess;
use fabrials_agent_host::server::HostState;
use serde_json::json;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        ("GROK_XAI_API_BASE_URL", "https://ai.fabrials.com"),
        ("GROK_CLI_CHAT_PROXY_BASE_URL", "https://ai.fabrials.com"),
        ("GROK_MODELS_BASE_URL", "https://ai.fabrials.com"),
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
    env.push(("XAI_API_KEY".into(), key));
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
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        let nonce = state.pairing.lock().await.mint_nonce(fabrials_agent_host::now_ms()).unwrap();
        let (browser, grant) = RemoteAccess::pair(state.clone(), nonce.expose()).await.unwrap();
        let epoch = browser.connect().await.unwrap();
        let loaded = browser.command(command(Operation::LoadSession {
            workspace_id: "synthetic".into(), session_id: session_id.clone()
        }, epoch, "load-terminal-session")).await;
        assert_eq!(loaded.status, 202, "load response: {}", loaded.body);
        let prompt = Operation::Prompt {
            session_id: session_id.clone(),
            text: "This is an isolated transport acceptance test. Use exactly one shell command: printf 'REMOTE_ONCE\\n' >> effect.txt . Do not change the command. It must ask for approval. Then reply DONE. Run no other commands and do not change permissions.".into(),
            bash: false,
        };
        let accepted = browser.command(command(prompt.clone(), epoch, "one-turn")).await;
        assert_eq!(accepted.status, 202);
        assert_eq!(accepted.body["result"]["outcome"], "commandAccepted");
        let (request_id, params) = loop {
            tokio::select! {
                event = terminal_events.recv() => {
                    match event {
                        Some(AgentEvent::PermissionRequest { request_id, params }) => break (request_id, params),
                        Some(_) => {},
                        None => panic!("terminal client disconnected before approval"),
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    browser.heartbeat(epoch).await.unwrap();
                }
            }
        };
        let requested = params.pointer("/toolCall/rawInput/command").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(requested.trim(), "printf 'REMOTE_ONCE\\n' >> effect.txt", "unexpected effect refused");
        loop {
            if !state.session.lock().await.pending_permissions.is_empty() { break; }
            tokio::task::yield_now().await;
        }
        println!("Native Remote prompt and approval observed by the second official CLI client");
        browser.disconnect().await;
        let resumed = RemoteAccess::authenticate(state.clone(), grant.session_token, grant.csrf_token).await.unwrap();
        let next_epoch = resumed.connect().await.unwrap();
        assert!(!state.session.lock().await.pending_permissions.is_empty());
        terminal.answer_permission(&request_id, "allow-once").await.unwrap();
        println!("Browser disconnect/reconnect preserved approval; second CLI client answered");
        loop {
            let status = resumed.command(command(Operation::GetCommandStatus { idempotency_key: "one-turn".into() }, next_epoch, "status")).await;
            if status.body["result"]["state"] != "running" {
                assert_eq!(status.body["result"]["state"], "completed", "receipt: {}", status.body);
                break;
            }
            resumed.heartbeat(next_epoch).await.unwrap();
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(std::fs::read_to_string(workspace.join("effect.txt")).unwrap(), "REMOTE_ONCE\n");
        let retry = resumed.command(command(prompt, next_epoch, "one-turn")).await;
        assert_eq!(retry.body["result"]["state"], "completed");
        assert_eq!(std::fs::read_to_string(workspace.join("effect.txt")).unwrap(), "REMOTE_ONCE\n");
        tokio::time::timeout(Duration::from_secs(2), async {
            while !state.session.lock().await.pending_permissions.is_empty() {
                tokio::task::yield_now().await;
            }
        }).await.expect("externally answered approval must disappear from Remote");
        println!("PASS: completed receipt, no resubmission on retry, exactly one effect, externally resolved approval cleared");
        let status = resumed.command(command(Operation::GetCommandStatus { idempotency_key: "one-turn".into() }, next_epoch, "final-status")).await;
        println!("{}", json!({"state": status.body["result"]["state"], "effectLines": 1}));
    }).await;
    state.request_shutdown();
    terminal.shutdown().await?;
    adapter.shutdown().await?;
    leader.kill().await?;
    leader.wait().await?;
    result?;
    Ok(())
}
