//! The embeddable host end to end: `serve` with the scripted agent, pairing
//! through the control channel, a command, the event socket, and `/healthz`;
//! plus agent relaunch after an exit.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fabrials_agent_host::acp::AgentCommand;
use fabrials_agent_host::control::{self, ControlRequest, ControlResponse};
use fabrials_agent_host::journal::ReplayOutcome;
use fabrials_agent_host::origin::{LocalOrigin, PRODUCTION_WEB_ORIGIN};
use fabrials_agent_host::protocol::{Event, WS_SUBPROTOCOL};
use fabrials_agent_host::server::{AGENT_READY, AGENT_RESTARTING, HostState};
use fabrials_agent_host::state::{self, InstallIdentity};
use fabrials_agent_host::{HostConfig, HostIdentity, RelaunchPolicy};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::header;

const INSTALL: &str = "fedcba9876543210fedcba9876543210";

/// Path of the compiled fake agent next to the test binary.
fn fake_agent() -> PathBuf {
    let mut path = std::env::current_exe().expect("test exe");
    path.pop(); // deps/
    path.pop(); // debug/
    path.push("examples");
    path.push(format!("fake_agent{}", std::env::consts::EXE_SUFFIX));
    if !path.exists() {
        let status = std::process::Command::new(env!("CARGO"))
            .args([
                "build",
                "-p",
                "fabrials-agent-host",
                "--example",
                "fake_agent",
            ])
            .status()
            .expect("spawn cargo");
        assert!(status.success(), "cargo build --example fake_agent failed");
    }
    assert!(path.exists(), "missing {}", path.display());
    path
}

fn free_port() -> u16 {
    static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(22_100);
    for _ in 0..300 {
        let port = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert!(port < 22_900, "exhausted the test port range");
        if std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok()
            && std::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, port)).is_ok()
        {
            return port;
        }
    }
    panic!("no free port");
}

fn init_count(log: &Path) -> usize {
    std::fs::read_to_string(log).map_or(0, |text| text.lines().count())
}

async fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    for _ in 0..200 {
        if ready() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

/// One HTTP/1.1 exchange with `Connection: close`; returns status and body.
async fn http(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect");
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str(&format!(
        "Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut response))
        .await
        .expect("response in time")
        .expect("read");
    let response = String::from_utf8_lossy(&response).into_owned();
    let (head, body) = response.split_once("\r\n\r\n").expect("http response");
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("status");
    (status, body.to_owned())
}

/// Next text frame from the socket, skipping control frames.
async fn next_text<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> Option<String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use futures_util::StreamExt as _;
    use tokio_tungstenite::tungstenite::Message;

    loop {
        match socket.next().await? {
            Ok(Message::Text(text)) => return Some(text.to_string()),
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
            Ok(Message::Binary(_) | Message::Close(_)) | Err(_) => return None,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serve_pairs_dispatches_streams_and_reports_its_identity() {
    let root = tempfile::tempdir().expect("tempdir");
    let state_dir = root.path().join("agent");
    std::fs::create_dir(&state_dir).expect("state dir");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700))
            .expect("chmod");
    }
    let port = free_port();
    state::persist(
        &state_dir,
        &InstallIdentity {
            install_id: INSTALL.to_owned(),
            port,
        },
    )
    .expect("identity");
    let init_log = root.path().join("init.log");

    let mut config = HostConfig::new(&state_dir, HostIdentity::new("spanreed", "9.9.9"));
    config.agent_program = fake_agent().to_string_lossy().into_owned();
    config.agent_env = vec![(
        "FAKE_AGENT_INIT_LOG".to_owned(),
        init_log.to_string_lossy().into_owned(),
    )];
    config.picker = Some(Arc::new(
        fabrials_agent_host::picker::UnavailableDirectoryPicker,
    ));
    let serving = tokio::spawn(fabrials_agent_host::serve(config));

    // The host spawned and initialised the configured agent.
    wait_until("the agent handshake", || init_count(&init_log) >= 1).await;

    // Health names the embedding program and keeps the existing fields.
    let (status, body) = http(port, "GET", "/healthz", &[], "").await;
    assert_eq!(status, 200, "{body}");
    let health: serde_json::Value = serde_json::from_str(&body).expect("health json");
    assert_eq!(health["ok"], true);
    assert_eq!(health["mode"], "bridge");
    assert_eq!(health["protocolVersion"], 2);
    assert_eq!(health["host"], "spanreed");
    assert_eq!(health["hostVersion"], "9.9.9");

    // Pair the way `open` does: the control channel mints the nonce.
    let ControlResponse::Paired { url, origin } =
        control::call(&state_dir, &ControlRequest::MintNonce)
            .await
            .expect("mint")
    else {
        panic!("expected a pairing response");
    };
    assert_eq!(
        origin,
        format!("http://{INSTALL}.grok-light.localhost:{port}")
    );
    assert!(
        url.starts_with(&format!("{PRODUCTION_WEB_ORIGIN}/#pair=")),
        "{url}"
    );
    let nonce = url
        .split_once("#pair=")
        .and_then(|(_, rest)| rest.split('&').next())
        .expect("nonce");

    let (status, body) = http(
        port,
        "POST",
        "/pair",
        &[("Origin", PRODUCTION_WEB_ORIGIN)],
        &format!(r#"{{"nonce":"{nonce}"}}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let paired: serde_json::Value = serde_json::from_str(&body).expect("pair json");
    let token = paired["sessionToken"].as_str().expect("token").to_owned();
    let csrf = paired["csrfToken"].as_str().expect("csrf").to_owned();

    // A hosted command with the session header and CSRF token.
    let (status, body) = http(
        port,
        "POST",
        "/command",
        &[
            ("Origin", PRODUCTION_WEB_ORIGIN),
            ("x-gl-session", &token),
            ("x-grok-light-csrf", &csrf),
        ],
        r#"{"protocolVersion":2,"requestId":"req-1","operation":{"kind":"bootstrap"}}"#,
    )
    .await;
    assert_eq!(status, 202, "{body}");
    let answer: serde_json::Value = serde_json::from_str(&body).expect("command json");
    assert_eq!(answer["requestId"], "req-1");
    assert_eq!(answer["result"]["outcome"], "workspaces", "{body}");

    // The event socket authenticates with the `gls.` subprotocol.
    let mut request = format!("ws://127.0.0.1:{port}/events")
        .into_client_request()
        .expect("ws request");
    let headers = request.headers_mut();
    headers.insert(
        header::ORIGIN,
        PRODUCTION_WEB_ORIGIN.parse().expect("origin"),
    );
    headers.insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        format!("{WS_SUBPROTOCOL}, gls.{token}")
            .parse()
            .expect("protocols"),
    );
    let (mut socket, response) = tokio_tungstenite::connect_async(request)
        .await
        .expect("events upgrade");
    assert_eq!(
        response
            .headers()
            .get(header::SEC_WEBSOCKET_PROTOCOL)
            .and_then(|value| value.to_str().ok()),
        Some(WS_SUBPROTOCOL)
    );
    let frame = tokio::time::timeout(Duration::from_secs(5), next_text(&mut socket))
        .await
        .expect("frame in time")
        .expect("a text frame");
    let envelope: serde_json::Value = serde_json::from_str(&frame).expect("event json");
    assert_eq!(envelope["protocolVersion"], 2);
    let kind = envelope["event"]["kind"].as_str().expect("kind");
    assert!(
        kind == "hostStatus" || kind == "sessionSnapshot",
        "first event is host state, got {frame}"
    );
    drop(socket);

    // `stop` over the control channel ends `serve` cleanly.
    assert!(matches!(
        control::call(&state_dir, &ControlRequest::Stop).await,
        Ok(ControlResponse::Stopping)
    ));
    let result = tokio::time::timeout(Duration::from_secs(10), serving)
        .await
        .expect("serve returns after stop")
        .expect("join");
    assert!(result.is_ok(), "serve ended with {result:?}");
}

fn host_status_states(journal: &fabrials_agent_host::journal::Journal) -> Vec<String> {
    match journal.replay_after(0) {
        ReplayOutcome::Replay(events) => events
            .into_iter()
            .filter_map(|envelope| match envelope.event {
                Event::HostStatus { state } => Some(state),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_that_exits_is_relaunched_and_the_host_returns_to_ready() {
    let root = tempfile::tempdir().expect("tempdir");
    let init_log = root.path().join("init.log");
    let marker = root.path().join("exited-once");
    let state = Arc::new(HostState::new(
        LocalOrigin::new(INSTALL, 20_002).expect("origin"),
    ));
    let command = AgentCommand::new(fake_agent().to_string_lossy().into_owned())
        .with_working_directory(root.path())
        .with_env([
            (
                "FAKE_AGENT_INIT_LOG".to_owned(),
                init_log.to_string_lossy().into_owned(),
            ),
            (
                "FAKE_AGENT_EXIT_ONCE".to_owned(),
                marker.to_string_lossy().into_owned(),
            ),
        ]);
    let policy = RelaunchPolicy {
        initial_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(200),
        stable_after: Duration::from_secs(60),
    };
    let supervisor = state.supervise_agent(command, policy);

    // The first process exits after its handshake; a second one is started
    // and completes its own.
    wait_until("a second initialize", || init_count(&init_log) >= 2).await;
    assert!(marker.exists(), "the first agent took the exit path");

    let mut states = Vec::new();
    for _ in 0..200 {
        states = {
            let journal = state.journal.lock().await;
            host_status_states(&journal)
        };
        if states.iter().filter(|s| *s == AGENT_READY).count() >= 2
            && state.agent.lock().await.is_some()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        states,
        vec![
            AGENT_READY.to_owned(),
            AGENT_RESTARTING.to_owned(),
            AGENT_READY.to_owned()
        ],
        "ready, restarting, ready again"
    );
    assert!(
        state.agent.lock().await.is_some(),
        "the relaunched agent is attached"
    );

    // Shutdown ends supervision instead of relaunching again.
    state.request_shutdown();
    tokio::time::timeout(Duration::from_secs(5), supervisor)
        .await
        .expect("supervisor stops on shutdown")
        .expect("join");
    assert_eq!(init_count(&init_log), 2, "no relaunch after shutdown");
    assert!(state.agent.lock().await.is_none());
}
