use fabrials_agent_host::{
    dispatch::{LiveSession, Workspace},
    origin::LocalOrigin,
    server::HostState,
};
use fabrials_remote_host::shares::Shares;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("fixture_directory_required")?,
    );
    let input: Value = serde_json::from_slice(&std::fs::read(directory.join("input.json"))?)?;
    let port = input["proxyPort"].as_u64().ok_or("proxy_port_required")?;
    if port == 0 || port > u16::MAX as u64 {
        return Err("invalid_proxy_port".into());
    }
    let workspace = directory.join("workspace");
    let home = directory.join("grok");
    if std::env::var_os("GROK_HOME").as_deref() != Some(home.as_os_str()) {
        return Err("isolated_grok_home_required".into());
    }
    std::fs::create_dir_all(&workspace)?;
    let source = home
        .join("sessions")
        .join(fabrials_agent_host::session_catalog::encode_cwd_dirname(
            &workspace.to_string_lossy(),
        ))
        .join("synthetic-session")
        .join("updates.jsonl");
    std::fs::create_dir_all(source.parent().ok_or("source_parent_required")?)?;
    if !source.exists() {
        std::fs::write(
            &source,
            format!(
                "{}\n",
                json!({"update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"Synthetic publication one"}}})
            ),
        )?;
    }
    let host = Arc::new(HostState::new(LocalOrigin::new(
        "0123456789abcdef0123456789abcdef",
        21001,
    )?));
    {
        let mut state = host.session.lock().await;
        state.workspaces.insert(
            "synthetic-project".into(),
            Workspace {
                id: "synthetic-project".into(),
                display_name: "Synthetic project".into(),
                path: workspace,
            },
        );
        state.open_session(LiveSession {
            id: "synthetic-session".into(),
            workspace_id: "synthetic-project".into(),
            workspace_name: "Synthetic project".into(),
            running: false,
            queued: vec![],
            opened_at_ms: 1,
        })?;
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::https(format!("http://127.0.0.1:{port}"))?)
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()?;
    let publisher = Shares::new(host, directory.clone(), client)?;
    if !directory.join("remote-publications.json").exists() {
        publisher
            .start(
                "synthetic-session",
                input["shareId"].as_str().ok_or("share_required")?,
                input["generation"].as_u64().ok_or("generation_required")?,
                input["uploadToken"]
                    .as_str()
                    .ok_or("upload_token_required")?,
            )
            .await?;
    }
    std::fs::write(
        directory.join("ready.json"),
        serde_json::to_vec(&json!({"source":source,"pid":std::process::id()}))?,
    )?;
    let mut rounds = 0u64;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    while !directory.join("stop").exists() {
        if tokio::time::Instant::now() >= deadline {
            return Err("fixture_timed_out".into());
        }
        let error = publisher.sync().await.err();
        rounds += 1;
        std::fs::write(
            directory.join("status.json"),
            serde_json::to_vec(&json!({"rounds":rounds,"error":error}))?,
        )?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}
