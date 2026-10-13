#![allow(missing_docs)]

use fabrials_agent_host::acp::{AgentCommand, AgentEvent, AgentHandle, AgentMode};
use fabrials_agent_host::dispatch::Workspace;
use fabrials_agent_host::journal::ReplayOutcome;
use fabrials_agent_host::protocol::{CommandEnvelope, Event, Operation, PROTOCOL_VERSION};
use fabrials_agent_host::remote::RemoteAccess;
use fabrials_agent_host::server::HostState;
use serde_json::{Value, json};
use std::path::PathBuf;
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

fn qualified_permission(params: &Value, url: &str) -> bool {
    let input = &params["toolCall"]["rawInput"];
    let Some(name) = input["tool_name"].as_str() else {
        return false;
    };
    let arguments = &input["tool_input"];
    match name {
        "syl__browser_open" => arguments == &json!({"url":url}),
        "syl__browser_snapshot" | "syl__browser_close" | "syl__status" => {
            arguments.as_object().is_some_and(|map| map.is_empty())
        }
        "syl__browser_click" => arguments.as_object().is_some_and(|map| {
            map.len() == 2
                && map.get("ref").is_some_and(Value::is_string)
                && map.get("observation").is_some_and(Value::is_string)
        }),
        _ => false,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("FABRIALS_LOCAL_DEV_KEY")?;
    let output = PathBuf::from(std::env::var("QUALIFY_SYL_OUTPUT")?);
    let url = std::env::var("QUALIFY_SYL_URL")?;
    let proxy = std::env::var("QUALIFY_SYL_PROXY")?;
    let original_home = std::env::var("QUALIFY_SYL_HOME")?;
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
    let proxy_args = json!([
        proxy,
        output.to_string_lossy(),
        "--proxy",
        "--url",
        url,
        "--home",
        original_home
    ]);
    std::fs::write(
        grok_home.join("config.toml"),
        format!("[mcp_servers.syl]\ncommand = \"python3\"\nargs = {proxy_args}\n"),
    )?;
    let mut environment: Vec<(String, String)> = [
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
        ("GROK_AGENT_ID", "remote-syl-qualification"),
        ("GROK_XAI_API_BASE_URL", "https://ai.fabrials.com/v1"),
        ("GROK_CLI_CHAT_PROXY_BASE_URL", "https://ai.fabrials.com/v1"),
        ("GROK_MODELS_BASE_URL", "https://ai.fabrials.com/v1"),
    ]
    .into_iter()
    .map(|(name, value)| (name.into(), value.into()))
    .collect();
    for (name, path) in [
        ("HOME", &home),
        ("USERPROFILE", &home),
        ("GROK_HOME", &grok_home),
        ("XDG_CONFIG_HOME", &config),
        ("GROK_LEADER_SOCKET", &socket),
    ] {
        environment.push((name.into(), path.to_string_lossy().into_owned()));
    }
    for name in [
        "XAI_API_KEY",
        "GROK_CODE_XAI_API_KEY",
        "AI_RELAY_VIRTUAL_KEY",
        "FABRIALS_LOCAL_DEV_KEY",
        "GROK_API_KEY",
    ] {
        environment.push((name.into(), key.clone()));
    }
    let program = std::env::var("FABRIALS_AGENT_PROGRAM")?;
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
        .envs(environment.iter().cloned())
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
        .with_env(environment);
    let (adapter, mut incoming) = AgentHandle::spawn(&agent_command)?;
    let initialized = adapter.initialize().await?;
    println!(
        "Official Grok {}",
        initialized.agent_version().unwrap_or("unknown")
    );
    let (session_id, _) = adapter.new_session(&workspace).await?;
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
            display_name: "Synthetic Syl browser".into(),
            path: workspace,
        },
    );
    let (forward, events) = mpsc::channel(256);
    let (decisions, mut permissions) = mpsc::channel(64);
    let forwarding = tokio::spawn(async move {
        while let Some(event) = incoming.recv().await {
            if let AgentEvent::PermissionRequest { request_id, params } = &event {
                let _ = decisions.send((request_id.clone(), params.clone())).await;
            }
            if forward.send(event).await.is_err() {
                break;
            }
        }
    });
    state.attach_agent(adapter.clone(), events);
    let owner_control = tokio::spawn(fabrials_agent_host::control::serve(
        fabrials_agent_host::control::bind(&host_directory)?,
        state.clone(),
    ));
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.agent.lock().await.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let result = tokio::time::timeout(Duration::from_secs(240), async {
        let nonce = state.pairing.lock().await.mint_nonce(fabrials_agent_host::now_ms()).unwrap();
        let (mut access, grant) = RemoteAccess::pair(state.clone(), nonce.expose()).await.unwrap();
        let mut epoch = access.connect().await.map_err(str::to_owned)?;
        let loaded = access.command(command(Operation::LoadSession { workspace_id:"synthetic".into(), session_id:session_id.clone() }, epoch, "load-syl-session")).await;
        if loaded.status != 202 { return Err("session_load_failed".to_owned()); }
        let text = format!("This is an authorized synthetic browser acceptance test. Use only the syl MCP browser_open/browser_snapshot/browser_click/browser_close tools and tool discovery if necessary; no shell, files or other tools. Open exactly {url}. Take a snapshot and observe Counter 0 and Choice blue. Click Increment once using its ref and observation. For the explicit stale-observation safety test, immediately repeat browser_click with EXACTLY the previous ref and observation, without refreshing them; this second call must fail as stale and must not increment again. Then take a fresh snapshot, click Choose green once using the fresh ref and observation, take a final snapshot showing Counter 1 and Choice green, and close the browser. Do not repair, retry, or substitute failed operations. Finish with a short factual report. Each permission is approved only once.");
        let operation = Operation::Prompt { session_id:session_id.clone(), text, bash:false };
        let accepted = access.command(command(operation.clone(), epoch, "syl-turn")).await;
        if accepted.status != 202 { return Err("prompt_refused".to_owned()); }
        let mut disconnect_seen = false;
        let mut approved = 0;
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        let mut heartbeat = tokio::time::interval(Duration::from_secs(3));
        loop {
            tokio::select! {
                permission = permissions.recv() => {
                    let Some((request_id, params)) = permission else { return Err("permission_channel_closed".into()); };
                    if !qualified_permission(&params, &url) {
                        std::fs::write(output.join("refused-permission.json"), serde_json::to_vec_pretty(&params).unwrap()).unwrap();
                        return Err("out_of_scope_permission_refused".into());
                    }
                    let projected_id = tokio::time::timeout(Duration::from_secs(2), async {
                        loop {
                            if let Some((id,_)) = state.session.lock().await.pending_permissions.iter().find(|(_, value)| value.request_id == request_id) { return id.clone(); }
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    }).await.map_err(|_| "permission_projection_missing")?;
                    let reply = access.command(command(Operation::DecidePermission { session_id:session_id.clone(), request_id:projected_id, option_id:"allow-once".into() }, epoch, &format!("syl-permission-{approved}"))).await;
                    if reply.status != 202 { return Err("remote_permission_failed".into()); }
                    approved += 1;
                },
                _ = heartbeat.tick() => { access.heartbeat(epoch).await.map_err(str::to_owned)?; },
                _ = tick.tick() => {
                    let fixture: Value = serde_json::from_slice(&std::fs::read(output.join("fixture-state.json")).unwrap()).unwrap();
                    if !disconnect_seen && fixture["counter"] == 1 {
                        access.disconnect().await;
                        let old = access.command(command(operation.clone(), epoch, "old-socket-replay")).await;
                        if old.status != 409 { return Err("disconnected_connection_accepted".into()); }
                        access = RemoteAccess::authenticate(state.clone(), grant.session_token.clone(), grant.csrf_token.clone()).await.unwrap();
                        epoch = access.connect().await.map_err(str::to_owned)?;
                        let replay = access.command(command(operation.clone(), epoch, "syl-turn")).await;
                        if replay.body["result"]["state"] != "running" { return Err("uncertain_retry_not_running".into()); }
                        disconnect_seen = true;
                        println!("{}", json!({"milestone":"disconnected_during_delayed_effect","passed":true}));
                    }
                    let receipt = access.command(command(Operation::GetCommandStatus { idempotency_key:"syl-turn".into() }, epoch, "syl-status")).await;
                    let status = receipt.body["result"]["state"].as_str().unwrap_or("missing");
                    if status == "completed" { break; }
                    if status != "running" { return Err(format!("unexpected_receipt_{status}")); }
                }
            }
        }
        if !disconnect_seen || approved == 0 { return Err("required_transport_milestone_missing".into()); }
        let replay = access.command(command(operation.clone(), epoch, "syl-turn")).await;
        if replay.body["result"]["state"] != "completed" { return Err("completed_retry_failed".into()); }
        let ReplayOutcome::Replay(events) = access.events(0).await.map_err(str::to_owned)? else { return Err("tool_projection_missing".into()); };
        let resolved: std::collections::BTreeSet<_> = events.iter().filter_map(|row| match &row.event {
            Event::ToolStart { tool_call_id, name, .. } if name.starts_with("syl__browser_") => Some(tool_call_id.clone()),
            Event::ToolProgress { tool_call_id, title:Some(title), .. } if title.starts_with("syl__browser_") => Some(tool_call_id.clone()),
            _ => None,
        }).collect();
        let ends = events.iter().filter(|row| matches!(&row.event, Event::ToolEnd { tool_call_id, .. } if resolved.contains(tool_call_id))).count();
        let failures = events.iter().filter(|row| matches!(&row.event, Event::ToolEnd { tool_call_id, failed:true, .. } if resolved.contains(tool_call_id))).count();
        std::fs::write(output.join("remote-events.json"), serde_json::to_vec_pretty(&events).unwrap()).unwrap();
        if resolved.len() < 8 || ends != resolved.len() || failures == 0 { return Err("incomplete_tool_projection".into()); }
        let revoked = fabrials_agent_host::control::call(&host_directory, &fabrials_agent_host::control::ControlRequest::RevokePairing { session_id:access.session_id().into() }).await.map_err(|_| "owner_control_unavailable")?;
        if !matches!(revoked, fabrials_agent_host::control::ControlResponse::Revoked) { return Err("revocation_not_durable".into()); }
        let refused = access.command(command(operation, epoch, "revoked-replay")).await;
        if refused.status != 403 { return Err("revoked_device_accepted".into()); }
        if access.events(0).await.is_ok() { return Err("revoked_device_received_events".into()); }
        let before = std::fs::read(output.join("fixture-state.json")).unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        if before != std::fs::read(output.join("fixture-state.json")).unwrap() { return Err("unexpected_late_effect".into()); }
        println!("{}", json!({"milestone":"syl_remote_acceptance","permissions":approved,"resolvedSylTools":resolved.len(),"toolEnds":ends,"toolFailures":failures,"revokedRefused":true,"completedRetryNoReplay":true}));
        Ok::<(), String>(())
    }).await;
    if let ReplayOutcome::Replay(events) = state.journal.lock().await.replay_after(0) {
        std::fs::write(
            output.join("remote-events-final.json"),
            serde_json::to_string_pretty(&events)?.replace(&key, "<redacted>"),
        )?;
    }
    state.request_shutdown();
    let _ = adapter.shutdown().await;
    let _ = leader.kill().await;
    let _ = leader.wait().await;
    forwarding.abort();
    owner_control.abort();
    result??;
    Ok(())
}
