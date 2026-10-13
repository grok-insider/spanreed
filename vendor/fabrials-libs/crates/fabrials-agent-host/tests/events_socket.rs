//! WebSocket event channel checks against a real loopback socket.
//!
//! The upgrade path cannot be exercised through a `oneshot` router call,
//! because the extractor needs a connection that is genuinely upgradable.
//! These tests open real sockets so the subprotocol, cookie, and control-lease
//! rules are verified as a browser would meet them.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fabrials_agent_host::acp::AgentMode;
use fabrials_agent_host::pairing::PairedBrowser;
use fabrials_agent_host::protocol::{Event, PROTOCOL_VERSION};
use fabrials_agent_host::server::{CONTROLLER_HEADER, CSRF_HEADER, router};
use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tower::ServiceExt as _;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

use fabrials_agent_host::lease::LeaseState;
use fabrials_agent_host::origin::LocalOrigin;
use fabrials_agent_host::protocol::WS_SUBPROTOCOL;
use fabrials_agent_host::server::{HostState, bind, serve};
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::http::header;

const INSTALL: &str = "0123456789abcdef0123456789abcdef";

fn free_port() -> u16 {
    static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(21_100);
    for _ in 0..300 {
        let port = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert!(port < 21_900, "exhausted the test port range");
        if std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok() {
            return port;
        }
    }
    panic!("no free port");
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

struct Running {
    port: u16,
    state: Arc<HostState>,
    _task: tokio::task::JoinHandle<()>,
}

impl Running {
    fn host(&self) -> String {
        format!("{INSTALL}.grok-light.localhost:{}", self.port)
    }

    fn origin(&self) -> String {
        format!("http://{}", self.host())
    }

    fn events_url(&self) -> String {
        // Connect via 127.0.0.1: Windows CI does not resolve `*.localhost` to
        // loopback (WSAHOST_NOT_FOUND). Host header still carries the origin.
        format!(
            "ws://127.0.0.1:{}/events{}",
            self.port,
            if self.state.agent_mode == AgentMode::Shared {
                "?protocolVersion=4"
            } else {
                ""
            }
        )
    }
}

async fn start() -> Running {
    start_mode(AgentMode::Isolated).await
}

async fn start_mode(mode: AgentMode) -> Running {
    start_configuration(
        mode,
        fabrials_agent_host::remote_management::RemoteExperiments::default(),
    )
    .await
}

async fn start_configuration(
    mode: AgentMode,
    experiments: fabrials_agent_host::remote_management::RemoteExperiments,
) -> Running {
    let port = free_port();
    let origin = LocalOrigin::new(INSTALL, port).expect("origin");
    let state = Arc::new(
        HostState::new(origin.clone())
            .with_agent_mode(mode)
            .with_remote_experiments(experiments),
    );
    let listeners = bind(&origin).await.expect("bind");
    let served = Arc::clone(&state);
    let task = tokio::spawn(async move {
        let _ = serve(listeners, served).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    Running {
        port,
        state,
        _task: task,
    }
}

/// Pair a browser directly against host state and return its cookie value.
async fn pair(running: &Running) -> String {
    let paired = pair_grant(running).await;
    format!("gl_session={}", paired.session_token.expose())
}

async fn pair_grant(running: &Running) -> PairedBrowser {
    let mut broker = running.state.pairing.lock().await;
    let nonce = broker
        .mint_nonce(now_ms())
        .expect("mint")
        .expose()
        .to_owned();
    broker.redeem_nonce(&nonce, now_ms()).expect("redeem")
}

/// Attempt an events upgrade with the supplied headers.
async fn connect(
    running: &Running,
    cookie: Option<&str>,
    protocol: Option<&str>,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tokio_tungstenite::tungstenite::Error,
> {
    let mut request = running.events_url().into_client_request().expect("request");
    let headers = request.headers_mut();
    headers.insert(header::HOST, running.host().parse().expect("host header"));
    headers.insert(
        header::ORIGIN,
        running.origin().parse().expect("origin header"),
    );
    if let Some(cookie) = cookie {
        headers.insert(header::COOKIE, cookie.parse().expect("cookie header"));
    }
    if let Some(protocol) = protocol {
        headers.insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            protocol.parse().expect("protocol header"),
        );
    }
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(stream, _)| stream)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paired_browser_with_the_exact_subprotocol_connects() {
    let running = start().await;
    let cookie = pair(&running).await;
    let socket = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL)).await;
    assert!(socket.is_ok(), "upgrade must succeed: {:?}", socket.err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upgrade_without_the_subprotocol_is_refused() {
    let running = start().await;
    let cookie = pair(&running).await;
    let socket = connect(&running, Some(&cookie), None).await;
    assert!(
        socket.is_err(),
        "an upgrade without the versioned subprotocol must be refused"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upgrade_with_a_wrong_subprotocol_is_refused() {
    let running = start().await;
    let cookie = pair(&running).await;
    let socket = connect(&running, Some(&cookie), Some("light.local.v2")).await;
    assert!(socket.is_err(), "a version mismatch must be refused");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unpaired_upgrade_is_refused() {
    let running = start().await;
    let socket = connect(&running, None, Some(WS_SUBPROTOCOL)).await;
    assert!(
        socket.is_err(),
        "an upgrade without a paired cookie must be refused"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connecting_takes_the_control_lease() {
    let running = start().await;
    let cookie = pair(&running).await;

    {
        let lease = running.state.lease.lock().await;
        assert_eq!(lease.state(now_ms()), LeaseState::Vacant);
    }

    let _socket = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .expect("upgrade");
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;

    let lease = running.state.lease.lock().await;
    assert_eq!(
        lease.state(now_ms()),
        LeaseState::Held,
        "the first connection must hold the control lease"
    );
    assert!(lease.epoch() >= 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_tab_cannot_take_control_from_a_live_one() {
    let running = start().await;
    let cookie = pair(&running).await;

    let _first = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .expect("first upgrade");
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    let epoch_after_first = running.state.lease.lock().await.epoch();

    // A second tab of the same browser session: the upgrade itself succeeds,
    // but it must not become the controller.
    let _second = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .expect("second upgrade");
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let lease = running.state.lease.lock().await;
    assert_eq!(
        lease.epoch(),
        epoch_after_first,
        "a second tab must not acquire a new controller epoch while the first is live"
    );
    assert_eq!(lease.state(now_ms()), LeaseState::Held);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn losing_the_controller_interrupts_work_in_flight() {
    let running = start().await;
    let cookie = pair(&running).await;

    let socket = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .expect("upgrade");
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;

    // Something is dispatched and never confirmed.
    {
        let mut journal = running.state.journal.lock().await;
        journal.begin("prompt-1", "Prompt", Some("s-1"));
        assert!(journal.pending_reviews().is_empty());
    }

    drop(socket);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let journal = running.state.journal.lock().await;
    let pending = journal.pending_reviews();
    assert_eq!(
        pending.len(),
        1,
        "losing the controlling tab must open a review record"
    );
    assert_eq!(pending[0].operation, "Prompt");
    assert!(
        !pending[0].acknowledged,
        "the record must await explicit human review"
    );
}

async fn frame(socket: &mut Socket) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let message = socket
                .next()
                .await
                .expect("socket remains open")
                .expect("frame");
            if let Message::Text(text) = message {
                return serde_json::from_str(&text).expect("event JSON");
            }
        }
    })
    .await
    .expect("event arrives")
}

fn control(hello: &Value) -> (u64, String) {
    let epoch = hello["event"]["state"]
        .as_str()
        .expect("state")
        .strip_prefix("controlling:")
        .expect("controller")
        .parse()
        .expect("epoch");
    let token = hello["event"]["controllerToken"].as_str().expect("token");
    assert_eq!(token.len(), 64);
    assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    (epoch, token.into())
}

async fn command(
    running: &Running,
    grant: &PairedBrowser,
    epoch: u64,
    controller: Option<&str>,
    operation: Value,
    key: &str,
) -> StatusCode {
    command_response(running, grant, epoch, controller, operation, key)
        .await
        .0
}

async fn command_response(
    running: &Running,
    grant: &PairedBrowser,
    epoch: u64,
    controller: Option<&str>,
    operation: Value,
    key: &str,
) -> (StatusCode, Value) {
    let mut request = Request::post("/command")
        .header(header::HOST, running.host())
        .header(header::ORIGIN, running.origin())
        .header(
            header::COOKIE,
            format!("gl_session={}", grant.session_token.expose()),
        )
        .header(CSRF_HEADER, grant.csrf_token.expose())
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(controller) = controller {
        request = request.header(CONTROLLER_HEADER, controller);
    }
    let response = router(running.state.clone())
        .oneshot(
            request
                .body(Body::from(
                    json!({
                        "protocolVersion": if running.state.agent_mode==AgentMode::Shared {4}else{PROTOCOL_VERSION},
                        "requestId": key,
                        "idempotencyKey": key,
                        "controllerEpoch": epoch,
                        "operation": operation,
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("command response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_shared_websockets_are_writable_and_reconnecting_does_not_reset_the_other() {
    let running = start_mode(AgentMode::Shared).await;
    let phone = pair_grant(&running).await;
    let desktop = pair_grant(&running).await;
    let cookie = |grant: &PairedBrowser| format!("gl_session={}", grant.session_token.expose());
    let mut first = connect(&running, Some(&cookie(&phone)), Some(WS_SUBPROTOCOL))
        .await
        .unwrap();
    assert_eq!(frame(&mut first).await["event"]["state"], "writable");
    running
        .state
        .emit_event(
            Event::MessageDelta {
                session_id: "one".into(),
                text: "live".into(),
            },
            None,
        )
        .await;
    let before = running.state.journal.lock().await.emitted_through();
    let mut second = connect(&running, Some(&cookie(&desktop)), Some(WS_SUBPROTOCOL))
        .await
        .unwrap();
    let hello = frame(&mut second).await;
    assert_eq!(hello["event"]["state"], "writable");
    assert!(hello["event"].get("controllerToken").is_none());
    let mut snapshots = 0;
    loop {
        let event = frame(&mut second).await;
        if event["event"]["kind"] == "sessionSnapshot" {
            snapshots += 1;
            assert_eq!(event["event"]["messages"][0]["text"], "live");
        }
        if event["event"]["kind"] == "workspacesChanged" {
            break;
        }
    }
    assert_eq!(snapshots, 1);
    assert_eq!(running.state.journal.lock().await.emitted_through(), before);
    assert_eq!(
        running.state.lease.lock().await.state(now_ms()),
        LeaseState::Vacant
    );
    drop(first);
    second
        .send(Message::Text("heartbeat".into()))
        .await
        .unwrap();
    assert_eq!(
        command(
            &running,
            &desktop,
            0,
            None,
            json!({"kind":"bootstrap"}),
            "after-disconnect"
        )
        .await,
        StatusCode::ACCEPTED
    );
    running.state.request_shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revoking_one_shared_writer_closes_only_its_socket() {
    let running = start_mode(AgentMode::Shared).await;
    let owner = pair_grant(&running).await;
    let revoked = pair_grant(&running).await;
    let cookie = |grant: &PairedBrowser| format!("gl_session={}", grant.session_token.expose());
    let mut first = connect(&running, Some(&cookie(&owner)), Some(WS_SUBPROTOCOL))
        .await
        .unwrap();
    let mut second = connect(&running, Some(&cookie(&revoked)), Some(WS_SUBPROTOCOL))
        .await
        .unwrap();
    assert_eq!(frame(&mut first).await["event"]["state"], "writable");
    assert_eq!(frame(&mut second).await["event"]["state"], "writable");
    assert_eq!(
        command(
            &running,
            &owner,
            0,
            None,
            json!({"kind":"revokeBrowserPairing","sessionId":revoked.session_id}),
            "revoke"
        )
        .await,
        StatusCode::ACCEPTED
    );
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(Ok(message)) = second.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        command(
            &running,
            &revoked,
            0,
            None,
            json!({"kind":"bootstrap"}),
            "revoked-read"
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        command(
            &running,
            &owner,
            0,
            None,
            json!({"kind":"bootstrap"}),
            "owner-read"
        )
        .await,
        StatusCode::ACCEPTED
    );
    first.send(Message::Text("heartbeat".into())).await.unwrap();
    running.state.request_shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn isolated_mode_declines_transfer_and_cleans_refused_device_connections() {
    let running = start_configuration(
        AgentMode::Isolated,
        fabrials_agent_host::remote_management::RemoteExperiments {
            control_transfer: true,
            device_panel: true,
            projects: false,
        },
    )
    .await;
    let grant = pair_grant(&running).await;
    let cookie = format!("gl_session={}", grant.session_token.expose());
    let mut owner = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .unwrap();
    let (epoch, token) = control(&frame(&mut owner).await);
    let (_, capabilities) = command_response(
        &running,
        &grant,
        epoch,
        Some(&token),
        json!({"kind":"getRemoteCapabilities"}),
        "capabilities",
    )
    .await;
    assert_eq!(capabilities["result"]["controlTransfer"], false);
    assert_eq!(capabilities["result"]["devicePanel"], true);
    let mut refused = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .unwrap();
    assert_eq!(
        frame(&mut refused).await["event"]["code"],
        "controller_held"
    );
    let mut invalidations = 0;
    while invalidations < 2 {
        if frame(&mut owner).await["event"]["kind"] == "remoteDevicesChanged" {
            invalidations += 1;
        }
    }
    let (_, devices) = command_response(
        &running,
        &grant,
        epoch,
        Some(&token),
        json!({"kind":"listBrowserDevices"}),
        "devices",
    )
    .await;
    assert_eq!(devices["result"]["devices"][0]["connectionCount"], 1);
    let (status, response) = command_response(
        &running,
        &grant,
        epoch,
        Some(&token),
        json!({"kind":"requestControlTransfer"}),
        "request-isolated",
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response["error"], "remote_experiment_disabled");
    drop(refused);
    drop(owner);
    running.state.request_shutdown();
}

async fn network_command(
    running: &Running,
    grant: &PairedBrowser,
    epoch: u64,
    controller: &str,
    operation: Value,
    key: &str,
) -> (StatusCode, Value) {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let body = json!({"protocolVersion":if running.state.agent_mode==AgentMode::Shared {4}else{PROTOCOL_VERSION},"requestId":key,"idempotencyKey":key,"controllerEpoch":epoch,"operation":operation}).to_string();
    let request = format!(
        "POST /command HTTP/1.1\r\nHost: {}\r\nOrigin: {}\r\nCookie: gl_session={}\r\n{}: {}\r\n{}: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        running.host(),
        running.origin(),
        grant.session_token.expose(),
        CSRF_HEADER,
        grant.csrf_token.expose(),
        CONTROLLER_HEADER,
        controller,
        body.len(),
        body
    );
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut stream =
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, running.port))
                .await
                .unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        let split = response
            .windows(4)
            .position(|value| value == b"\r\n\r\n")
            .unwrap();
        let headers = std::str::from_utf8(&response[..split]).unwrap();
        let status =
            StatusCode::from_u16(headers.split_whitespace().nth(1).unwrap().parse().unwrap())
                .unwrap();
        let body = &response[split + 4..];
        (
            status,
            if body.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(body).unwrap()
            },
        )
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fifty_http_enqueues_from_two_writers_have_one_queue_without_takeover() {
    let running = start_mode(AgentMode::Shared).await;
    running
        .state
        .session
        .lock()
        .await
        .open_session(fabrials_agent_host::dispatch::LiveSession {
            id: "one".into(),
            workspace_id: "w".into(),
            workspace_name: "test".into(),
            running: false,
            queued: vec![],
            opened_at_ms: 1,
        })
        .unwrap();
    let phone = pair_grant(&running).await;
    let desktop = pair_grant(&running).await;
    let mut sockets = Vec::new();
    for grant in [&phone, &desktop] {
        let cookie = format!("gl_session={}", grant.session_token.expose());
        let mut socket = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
            .await
            .unwrap();
        assert_eq!(frame(&mut socket).await["event"]["state"], "writable");
        sockets.push(socket);
    }
    let mut samples = Vec::new();
    for trial in 0..50 {
        let grant = if trial % 2 == 0 { &phone } else { &desktop };
        let start = std::time::Instant::now();
        let (status,accepted)=network_command(&running,grant,0,"",json!({"kind":"prompt","sessionId":"one","text":format!("message {trial}"),"bash":false}),&format!("send-{trial}")).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
        let (_, snapshot) = network_command(
            &running,
            grant,
            0,
            "",
            json!({"kind":"getSessionExecution","sessionId":"one"}),
            "read",
        )
        .await;
        assert_eq!(snapshot["result"]["queue"].as_array().unwrap().len(), 1);
        let entry = accepted["result"]["entryId"].as_str().unwrap();
        assert_eq!(
            network_command(
                &running,
                grant,
                0,
                "",
                json!({"kind":"removeQueued","sessionId":"one","entryId":entry}),
                &format!("remove-{trial}")
            )
            .await
            .0,
            StatusCode::ACCEPTED
        );
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    assert!(
        samples[47] < 2000.0,
        "loopback enqueue/remove p95 {}ms",
        samples[47]
    );
    assert_eq!(
        running.state.lease.lock().await.state(now_ms()),
        LeaseState::Vacant
    );
    running.state.request_shutdown();
}
