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
        format!("ws://127.0.0.1:{}/events", self.port)
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
                        "protocolVersion": PROTOCOL_VERSION,
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
async fn shared_observer_receives_events_but_cannot_borrow_control() {
    let running = start_mode(AgentMode::Shared).await;
    let grant = pair_grant(&running).await;
    let cookie = format!("gl_session={}", grant.session_token.expose());
    let mut owner = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .expect("owner");
    let (epoch, token) = control(&frame(&mut owner).await);
    let mut observer = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .expect("observer");
    let hello = frame(&mut observer).await;
    assert_eq!(hello["event"]["state"], "watching");
    assert!(hello["event"].get("controllerToken").is_none());
    observer
        .send(Message::Text("heartbeat".into()))
        .await
        .expect("observer heartbeat");
    let emitted = running
        .state
        .emit_event(Event::WorkspacesChanged, None)
        .await;
    loop {
        let received = frame(&mut observer).await;
        assert!(received["event"].get("controllerToken").is_none());
        if received["eventSequence"] == emitted.event_sequence {
            assert_eq!(received["event"]["kind"], "workspacesChanged");
            break;
        }
    }
    for borrowed in [
        None,
        Some("conn-1"),
        Some("remote-1"),
        Some("wrong-controller"),
    ] {
        assert_eq!(
            command(
                &running,
                &grant,
                epoch,
                borrowed,
                json!({"kind":"openWorkspacePicker"}),
                "denied"
            )
            .await,
            StatusCode::CONFLICT
        );
        assert!(!running.state.session.lock().await.picker_open);
    }
    let other_grant = pair_grant(&running).await;
    assert_eq!(
        command(
            &running,
            &other_grant,
            epoch,
            Some(&token),
            json!({"kind":"openWorkspacePicker"}),
            "foreign-session"
        )
        .await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        command(
            &running,
            &grant,
            epoch,
            None,
            json!({"kind":"listWorkspaces"}),
            "read"
        )
        .await,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        command(
            &running,
            &grant,
            epoch,
            Some(&token),
            json!({"kind":"openWorkspacePicker"}),
            "owner"
        )
        .await,
        StatusCode::ACCEPTED
    );
    drop(observer);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(running.state.lease.lock().await.epoch(), epoch);
    assert_eq!(
        running.state.lease.lock().await.state(now_ms()),
        LeaseState::Held
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_observer_reconnects_for_control_with_new_epoch_and_token() {
    let running = start_mode(AgentMode::Shared).await;
    let grant = pair_grant(&running).await;
    let cookie = format!("gl_session={}", grant.session_token.expose());
    let mut owner = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .expect("owner");
    let (old_epoch, old_token) = control(&frame(&mut owner).await);
    let mut observer = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .expect("observer");
    assert_eq!(frame(&mut observer).await["event"]["state"], "watching");
    running
        .state
        .journal
        .lock()
        .await
        .begin("inflight", "Prompt", Some("session"));
    drop(owner);
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while running.state.lease.lock().await.state(now_ms()) != LeaseState::Vacant {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("lease released");
    assert!(
        running
            .state
            .journal
            .lock()
            .await
            .pending_reviews()
            .is_empty()
    );
    let emitted = running
        .state
        .emit_event(Event::WorkspacesChanged, None)
        .await;
    loop {
        let received = frame(&mut observer).await;
        assert!(received["event"].get("controllerToken").is_none());
        if received["eventSequence"] == emitted.event_sequence {
            break;
        }
    }
    drop(observer);
    let mut next = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .expect("handover");
    let (epoch, token) = control(&frame(&mut next).await);
    assert!(epoch > old_epoch);
    assert_ne!(token, old_token);
    for (candidate_epoch, candidate_token) in [
        (old_epoch, old_token.as_str()),
        (epoch, old_token.as_str()),
        (old_epoch, token.as_str()),
    ] {
        assert_eq!(
            command(
                &running,
                &grant,
                candidate_epoch,
                Some(candidate_token),
                json!({"kind":"openWorkspacePicker"}),
                "stale"
            )
            .await,
            StatusCode::CONFLICT
        );
    }
    assert_eq!(
        command(
            &running,
            &grant,
            epoch,
            Some(&token),
            json!({"kind":"openWorkspacePicker"}),
            "new-owner"
        )
        .await,
        StatusCode::ACCEPTED
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revoking_a_shared_observer_closes_its_socket_without_releasing_owner() {
    let running = start_mode(AgentMode::Shared).await;
    let grant = pair_grant(&running).await;
    let observer_grant = pair_grant(&running).await;
    let cookie = format!("gl_session={}", grant.session_token.expose());
    let observer_cookie = format!("gl_session={}", observer_grant.session_token.expose());
    let mut owner = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
        .await
        .expect("owner");
    let (epoch, token) = control(&frame(&mut owner).await);
    let mut observer = connect(&running, Some(&observer_cookie), Some(WS_SUBPROTOCOL))
        .await
        .expect("observer");
    assert_eq!(frame(&mut observer).await["event"]["state"], "watching");
    assert_eq!(
        command(
            &running,
            &grant,
            epoch,
            Some(&token),
            json!({"kind":"revokeBrowserPairing", "sessionId":observer_grant.session_id}),
            "revoke"
        )
        .await,
        StatusCode::ACCEPTED
    );
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(Ok(message)) = observer.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    })
    .await
    .expect("revoked observer closed");
    assert_eq!(running.state.lease.lock().await.epoch(), epoch);
    assert_eq!(
        running.state.lease.lock().await.state(now_ms()),
        LeaseState::Held
    );
    assert_eq!(
        command(
            &running,
            &observer_grant,
            epoch,
            None,
            json!({"kind":"listWorkspaces"}),
            "revoked-read"
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_transfer_targets_tokens_and_preserves_both_sockets() {
    for mode in [AgentMode::Shared] {
        let running = start_configuration(
            mode,
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
        let (epoch, owner_token) = control(&frame(&mut owner).await);
        let mut observer = connect(&running, Some(&cookie), Some(WS_SUBPROTOCOL))
            .await
            .unwrap();
        let hello = frame(&mut observer).await;
        assert_eq!(hello["event"]["state"], "watching");
        let observer_token = hello["event"]["controllerToken"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(observer_token, owner_token);
        let (status, requested) = command_response(
            &running,
            &grant,
            0,
            Some(&observer_token),
            json!({"kind":"requestControlTransfer"}),
            "request-transfer",
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{requested}");
        let request_id = requested["result"]["request"]["requestId"]
            .as_str()
            .unwrap();
        assert_eq!(
            command(
                &running,
                &grant,
                epoch,
                Some(&owner_token),
                json!({"kind":"approveControlTransfer","requestId":request_id}),
                "approve-transfer"
            )
            .await,
            StatusCode::ACCEPTED
        );
        loop {
            let received = frame(&mut owner).await;
            if received["event"]["kind"] == "hostStatus" {
                assert_eq!(received["event"]["state"], "watching");
                assert_eq!(received["event"]["controllerToken"], owner_token);
                assert!(!received.to_string().contains(&observer_token));
                break;
            }
        }
        loop {
            let received = frame(&mut observer).await;
            if received["event"]["kind"] == "hostStatus" {
                let (new_epoch, new_token) = control(&received);
                assert_eq!(new_epoch, epoch + 1);
                assert_eq!(new_token, observer_token);
                assert!(!received.to_string().contains(&owner_token));
                break;
            }
        }
        assert_eq!(
            command(
                &running,
                &grant,
                epoch + 1,
                Some(&owner_token),
                json!({"kind":"openWorkspacePicker"}),
                "borrow-epoch"
            )
            .await,
            StatusCode::CONFLICT
        );
        owner.send(Message::Text("heartbeat".into())).await.unwrap();
        running
            .state
            .journal
            .lock()
            .await
            .begin("in-flight-transfer", "Prompt", Some("session"));
        drop(owner);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            running
                .state
                .journal
                .lock()
                .await
                .pending_reviews()
                .is_empty()
        );
        assert_eq!(
            running.state.lease.lock().await.state(now_ms()),
            LeaseState::Held
        );
        assert_eq!(
            command(
                &running,
                &grant,
                epoch + 1,
                Some(&observer_token),
                json!({"kind":"openWorkspacePicker"}),
                "new-controller"
            )
            .await,
            StatusCode::ACCEPTED
        );
        drop(observer);
        running.state.request_shutdown();
    }
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
    let body = json!({"protocolVersion":PROTOCOL_VERSION,"requestId":key,"idempotencyKey":key,"controllerEpoch":epoch,"operation":operation}).to_string();
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
async fn fifty_real_http_websocket_transfers_preserve_authority_and_meet_latency_gate() {
    let running = start_configuration(
        AgentMode::Shared,
        fabrials_agent_host::remote_management::RemoteExperiments {
            control_transfer: true,
            device_panel: true,
            projects: false,
        },
    )
    .await;
    let mut owner_grant = pair_grant(&running).await;
    let mut observer_grant = pair_grant(&running).await;
    let owner_cookie = format!("gl_session={}", owner_grant.session_token.expose());
    let observer_cookie = format!("gl_session={}", observer_grant.session_token.expose());
    let mut owner = connect(&running, Some(&owner_cookie), Some(WS_SUBPROTOCOL))
        .await
        .unwrap();
    let (mut epoch, mut owner_token) = control(&frame(&mut owner).await);
    let mut observer = connect(&running, Some(&observer_cookie), Some(WS_SUBPROTOCOL))
        .await
        .unwrap();
    let mut observer_token = frame(&mut observer).await["event"]["controllerToken"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut samples = Vec::new();
    for trial in 0..50 {
        let (status, requested) = network_command(
            &running,
            &observer_grant,
            0,
            &observer_token,
            json!({"kind":"requestControlTransfer"}),
            &format!("request-{trial}"),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let request_id = requested["result"]["request"]["requestId"]
            .as_str()
            .unwrap();
        let started = std::time::Instant::now();
        let (status, approved) = network_command(
            &running,
            &owner_grant,
            epoch,
            &owner_token,
            json!({"kind":"approveControlTransfer","requestId":request_id}),
            &format!("approve-{trial}"),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(approved["result"].get("epoch").is_none());
        assert!(!approved.to_string().contains(&observer_token));
        loop {
            let received = frame(&mut owner).await;
            if received["event"]["kind"] == "hostStatus" {
                assert_eq!(received["event"]["state"], "watching");
                assert_eq!(received["event"]["controllerToken"], owner_token);
                assert!(!received.to_string().contains(&observer_token));
                break;
            }
        }
        loop {
            let received = frame(&mut observer).await;
            if received["event"]["kind"] == "hostStatus" {
                assert_eq!(control(&received), (epoch + 1, observer_token.clone()));
                assert!(!received.to_string().contains(&owner_token));
                break;
            }
        }
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(
            network_command(
                &running,
                &owner_grant,
                epoch + 1,
                &owner_token,
                json!({"kind":"openWorkspacePicker"}),
                &format!("old-connection-{trial}")
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            network_command(
                &running,
                &observer_grant,
                epoch,
                &observer_token,
                json!({"kind":"openWorkspacePicker"}),
                &format!("stale-epoch-{trial}")
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        std::mem::swap(&mut owner, &mut observer);
        std::mem::swap(&mut owner_grant, &mut observer_grant);
        std::mem::swap(&mut owner_token, &mut observer_token);
        epoch += 1;
    }
    let (_, pending) = network_command(
        &running,
        &observer_grant,
        0,
        &observer_token,
        json!({"kind":"requestControlTransfer"}),
        "request-before-revoke",
    )
    .await;
    let request_id = pending["result"]["request"]["requestId"].as_str().unwrap();
    assert_eq!(
        network_command(
            &running,
            &owner_grant,
            epoch,
            &owner_token,
            json!({"kind":"revokeBrowserPairing","sessionId":observer_grant.session_id}),
            "revoke-observer"
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        network_command(
            &running,
            &owner_grant,
            epoch,
            &owner_token,
            json!({"kind":"approveControlTransfer","requestId":request_id}),
            "approve-revoked"
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        network_command(
            &running,
            &observer_grant,
            epoch,
            &observer_token,
            json!({"kind":"bootstrap"}),
            "revoked-read"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut ordered = samples.clone();
    ordered.sort_by(f64::total_cmp);
    let p95 = ordered[47];
    assert!(
        p95 <= 2000.0,
        "control transfer p95 exceeded two seconds: {p95}"
    );
    if let Some(path) = std::env::var_os("FABRIALS_TRANSFER_EVIDENCE") {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        file.write_all(serde_json::to_string_pretty(&json!({"scope":"real loopback HTTP and WebSocket, ephemeral shared ACP host, no relay or browser","samplesMs":samples,"p95Ms":p95,"transfers":50,"staleEpochRefusals":50,"staleConnectionRefusals":50,"revokedTargetRejected":true,"thresholdMs":2000,"gatePassed":true})).unwrap().as_bytes()).unwrap();
    }
    drop(owner);
    drop(observer);
    running.state.request_shutdown();
}
