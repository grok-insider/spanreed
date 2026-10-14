use fabrials_agent_host::{
    dispatch::LiveSession, instance::ensure_private_directory, picker::UnavailableDirectoryPicker,
    server::HostState,
};
use fabrials_remote_host::direct::{DirectHost, prepare_for_web_origin};
use serde_json::json;
use std::{path::PathBuf, sync::Arc, time::Duration};

fn write_private_json(
    directory: &std::path::Path,
    name: &str,
    value: &serde_json::Value,
) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(directory.join(name))?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let web_origin = std::env::var("DIRECT_BROWSER_QA_WEB_ORIGIN")
        .unwrap_or_else(|_| fabrials_agent_host::origin::PRODUCTION_WEB_ORIGIN.into());
    fabrials_agent_host::origin::validate_hosted_web_origin(&web_origin)?;
    let directory = PathBuf::from(std::env::var("DIRECT_BROWSER_QA_DIRECTORY")?);
    ensure_private_directory(&directory)?;
    let grok_home = directory.join("synthetic-grok");
    if std::env::var_os("GROK_HOME").map(PathBuf::from) != Some(grok_home.clone()) {
        return Err("an isolated synthetic GROK_HOME is required".into());
    }
    let identity = fabrials_agent_host::state::load_or_create(&directory)?;
    let direct = prepare_for_web_origin(
        &directory,
        &identity.install_id,
        "https://direct-fixture.test",
        &web_origin,
    )?;
    let host_key = direct.public_key.clone();
    let host = Arc::new(
        HostState::new(identity.origin()?)
            .with_allowed_origins(vec![web_origin])
            .with_private_origin(Some(direct.endpoint.clone()))?
            .with_agent_mode(fabrials_agent_host::acp::AgentMode::Shared)
            .with_persistence(&directory, Arc::new(UnavailableDirectoryPicker))?,
    );
    host.enable_direct_connection(Arc::new(DirectHost::new(&directory, direct, &host).await?))?;
    let workspace = directory.join("Synthetic direct QA - no Grok process");
    std::fs::create_dir_all(&workspace)?;
    let mut index = fabrials_agent_host::workspace::load(&directory)?;
    let enrolled = index.enrol(&workspace, fabrials_agent_host::now_ms())?;
    fabrials_agent_host::workspace::persist(&directory, &index)?;
    host.load_workspaces(&index).await;
    let transcript = grok_home
        .join("sessions")
        .join(fabrials_agent_host::session_catalog::encode_cwd_dirname(
            workspace.to_str().ok_or("invalid path")?,
        ))
        .join("synthetic-direct-session");
    ensure_private_directory(&transcript)?;
    write_private_json(
        &transcript,
        "summary.json",
        &json!({"info":{"id":"synthetic-direct-session","cwd":workspace},"session_summary":"Synthetic direct QA - no Grok process","updated_at":"2026-10-13T00:00:00Z","num_messages":2,"session_kind":"conversation"}),
    )?;
    let updates = [("user_message_chunk", "Synthetic direct connection QA. No Grok process is running."), ("agent_message_chunk", "This is synthetic fixture history, not a model response.")].into_iter().map(|(kind,text)| json!({"params":{"sessionId":"synthetic-direct-session","update":{"sessionUpdate":kind,"content":{"type":"text","text":text}}}}).to_string()).collect::<Vec<_>>().join("\n") + "\n";
    std::fs::write(transcript.join("updates.jsonl"), updates)?;
    host.session.lock().await.open_session(LiveSession {
        id: "synthetic-direct-session".into(),
        workspace_id: enrolled.id,
        workspace_name: "Synthetic direct QA - no Grok process".into(),
        running: false,
        queued: Vec::new(),
        opened_at_ms: fabrials_agent_host::now_ms(),
    })?;
    let restored = fabrials_agent_host::session_catalog::rehydrate_session_async(
        workspace.clone(),
        "synthetic-direct-session".into(),
    )
    .await;
    host.emit_event(
        fabrials_agent_host::session_catalog::snapshot_from_rehydrate_with_runtime(
            &grok_home,
            Some(&workspace),
            "synthetic-direct-session".into(),
            restored,
            Vec::new(),
        ),
        None,
    )
    .await;
    let agent = if std::env::var("DIRECT_BROWSER_QA_ACP").as_deref() == Ok("1") {
        let program = std::env::current_exe()?
            .parent()
            .ok_or("fixture location")?
            .join(format!(
                "fake_multiwriter_agent{}",
                std::env::consts::EXE_SUFFIX
            ));
        let mut command = fabrials_agent_host::acp::AgentCommand::new(program.to_string_lossy())
            .with_mode(fabrials_agent_host::acp::AgentMode::Shared);
        command
            .env
            .push(("FAKE_MULTIWRITER_CATALOGUE".into(), "1".into()));
        if std::env::var("DIRECT_BROWSER_QA_IMAGES").as_deref() == Ok("1") {
            command
                .env
                .push(("FAKE_MULTIWRITER_IMAGES".into(), "1".into()));
        }
        let (agent, events) = fabrials_agent_host::acp::AgentHandle::spawn(&command)?;
        agent.initialize().await?;
        let before = host.journal.lock().await.emitted_through();
        host.attach_agent(agent.clone(), events);
        tokio::time::timeout(Duration::from_secs(10),async {
            loop {
                if matches!(host.journal.lock().await.replay_after(before),fabrials_agent_host::journal::ReplayOutcome::Replay(events) if events.iter().any(|event|matches!(event.event,fabrials_agent_host::protocol::Event::WorkspacesChanged))) {break;}
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await?;
        Some(agent)
    } else {
        None
    };
    let listeners = fabrials_agent_host::server::bind(&host.origin).await?;
    let nonce = host
        .pairing
        .lock()
        .await
        .mint_nonce(fabrials_agent_host::now_ms())?;
    write_private_json(
        &directory,
        "ready.json",
        &json!({"port":host.origin.port(),"pairing":{"endpoint":"https://direct-fixture.test","installationId":identity.install_id,"hostPublicKey":host_key,"nonce":nonce.expose(),"label":"Synthetic direct QA - no Grok process"}}),
    )?;
    let serving = tokio::spawn(fabrials_agent_host::server::serve(listeners, host.clone()));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1800);
    while !directory.join("stop").exists() && tokio::time::Instant::now() < deadline {
        if directory.join("pair-next").exists() {
            std::fs::remove_file(directory.join("pair-next"))?;
            let nonce = host
                .pairing
                .lock()
                .await
                .mint_nonce(fabrials_agent_host::now_ms())?;
            write_private_json(
                &directory,
                "pair-next.json",
                &json!({"endpoint":"https://direct-fixture.test","installationId":identity.install_id,"hostPublicKey":host_key,"nonce":nonce.expose(),"label":"Synthetic second client"}),
            )?;
        }
        if let Some(agent) = &agent {
            for (name, method) in [
                ("release-turn", "test/release"),
                ("disable-models", "test/disable_models"),
            ] {
                if let Ok(session_id) = std::fs::read_to_string(directory.join(name)) {
                    std::fs::remove_file(directory.join(name))?;
                    if session_id.trim().len() > 512 {
                        return Err("invalid synthetic session".into());
                    }
                    agent
                        .request(method, json!({"sessionId":session_id.trim()}))
                        .await?;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    host.request_shutdown();
    tokio::time::timeout(Duration::from_secs(5), serving).await???;
    Ok(())
}
