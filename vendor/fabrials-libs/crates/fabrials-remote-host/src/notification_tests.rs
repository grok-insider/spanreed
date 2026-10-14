use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct TestCredentials;

#[async_trait::async_trait]
impl Credentials for TestCredentials {
    async fn access_token(&self) -> Result<zeroize::Zeroizing<String>, String> {
        Ok(zeroize::Zeroizing::new(
            "synthetic-installation-bearer".into(),
        ))
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    host: Arc<HostState>,
    notifications: Arc<Notifications>,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    event_status: Arc<Mutex<u16>>,
    api: String,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        fabrials_agent_host::instance::ensure_private_directory(directory.path()).unwrap();
        let host = Arc::new(
            HostState::new(
                fabrials_agent_host::origin::LocalOrigin::new(
                    "0123456789abcdef0123456789abcdef",
                    21001,
                )
                .unwrap(),
            )
            .with_agent_mode(fabrials_agent_host::acp::AgentMode::Shared),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = format!(
            "http://{}/api/spanreed/remote",
            listener.local_addr().unwrap()
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let event_status = Arc::new(Mutex::new(202));
        let captured = requests.clone();
        let status = event_status.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let split = loop {
                    let mut chunk = [0; 1024];
                    let read = stream.read(&mut chunk).await.unwrap();
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(split) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break split + 4;
                    }
                    assert!(bytes.len() <= 8192);
                };
                let headers = std::str::from_utf8(&bytes[..split]).unwrap();
                assert!(
                    headers
                        .to_ascii_lowercase()
                        .contains("authorization: bearer synthetic-installation-bearer")
                );
                let route = headers
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse().unwrap())
                    })
                    .unwrap();
                while bytes.len() < split + length {
                    let mut chunk = [0; 1024];
                    let read = stream.read(&mut chunk).await.unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&chunk[..read]);
                }
                let body: Value = serde_json::from_slice(&bytes[split..split + length]).unwrap();
                captured.lock().await.push((route.clone(), body.clone()));
                let code = if route.ends_with("/events") {
                    *status.lock().await
                } else {
                    200
                };
                let response = if route.ends_with("/bind") {
                    assert!(route.starts_with("/api/spanreed/remote/push/subscriptions/"));
                    let mut keys: Vec<_> = body.as_object().unwrap().keys().map(String::as_str).collect();
                    keys.sort();
                    assert_eq!(keys, ["bindingToken", "deviceId", "generation", "hostId"]);
                    let id = route.rsplit('/').nth(1).unwrap();
                    json!({"id":id,"active":true,"expiresAt":fabrials_agent_host::now_ms()+86_400_000,"generation":body["generation"]})
                } else { json!({"ok":true}) }.to_string();
                let message = format!(
                    "HTTP/1.1 {code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                );
                stream.write_all(message.as_bytes()).await.unwrap();
            }
        });
        let notifications = Arc::new(
            Notifications::with_loopback_api(
                host.clone(),
                directory.path().into(),
                "host-1".into(),
                reqwest::Client::new(),
                Arc::new(TestCredentials),
                &api,
            )
            .await
            .unwrap(),
        );
        Self {
            directory,
            host,
            notifications,
            requests,
            event_status,
            api,
            server,
        }
    }

    async fn pair(&self) -> RemoteAccess {
        let nonce = self
            .host
            .pairing
            .lock()
            .await
            .mint_nonce(fabrials_agent_host::now_ms())
            .unwrap();
        RemoteAccess::pair(self.host.clone(), nonce.expose())
            .await
            .unwrap()
            .0
    }

    async fn bind(&self, access: &RemoteAccess, id: &str) {
        let reply = self.notifications.dispatch(access, command(json!({"kind":"configureNotifications","id":id,"bindingToken":"synthetic-token","generation":3}))).await;
        assert_eq!(reply.0, 200, "{}", reply.1);
        assert_eq!(reply.1["result"]["generation"], 3);
    }

    async fn emit_turn(&self) {
        for state in ["running", "idle"] {
            self.host
                .emit_event(
                    Event::SessionStatus {
                        session_id: "synthetic-session".into(),
                        state: state.into(),
                    },
                    None,
                )
                .await;
        }
    }
}

fn command(operation: Value) -> Value {
    json!({"protocolVersion":4,"requestId":"synthetic-request","idempotencyKey":"synthetic-key","operation":operation})
}

fn permission(id: &str) -> Event {
    Event::PermissionRequest {
        session_id: "synthetic-session".into(),
        request_id: id.into(),
        options: vec!["allow-once".into()],
        title: Some("SECRET TITLE".into()),
        description: Some("SECRET COMMAND".into()),
    }
}

#[test]
fn transitions_do_not_notify_for_snapshots_repeated_permissions_or_interruptions() {
    let mut transitions = Transitions::default();
    let status = |state: &str| Event::SessionStatus {
        session_id: "s".into(),
        state: state.into(),
    };
    assert!(!transitions.observe(&status("idle")));
    assert!(!transitions.observe(&status("running")));
    assert!(!transitions.observe(&status("running")));
    assert!(transitions.observe(&status("idle")));
    assert!(!transitions.observe(&status("idle")));
    assert!(!transitions.observe(&status("running")));
    assert!(!transitions.observe(&status("reconnecting")));
    assert!(!transitions.observe(&status("idle")));
    assert!(transitions.observe(&permission("permission-one")));
    assert!(!transitions.observe(&permission("permission-one")));
    assert!(transitions.observe(&permission("permission-two")));
}

#[tokio::test]
async fn authenticated_noise_binding_survives_closed_channel_and_emits_only_minimal_events() {
    let fixture = Fixture::new().await;
    let mut channel = crate::tests::TestChannel::with_notifications(
        fixture.host.clone(),
        crate::EventDelivery::Polling,
        Some(fixture.notifications.clone()),
    )
    .await;
    let principal = channel.paired["body"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    channel.send(json!({"id":"push-bind","method":"command","body":command(json!({"kind":"configureNotifications","id":"subscription-a","bindingToken":"synthetic-token","generation":3}))})).await;
    let reply = channel.receive().await;
    assert_eq!(reply["status"], 200, "{reply}");
    assert_eq!(fixture.requests.lock().await[0].1["deviceId"], principal);
    drop(channel.input);
    tokio::time::timeout(Duration::from_secs(2), channel.task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    fixture
        .host
        .emit_event(permission("permission-one"), None)
        .await;
    fixture.notifications.sync().await.unwrap();
    fixture
        .host
        .emit_event(permission("permission-one"), None)
        .await;
    fixture.notifications.sync().await.unwrap();
    fixture.emit_turn().await;
    fixture.notifications.sync().await.unwrap();
    fixture.notifications.sync().await.unwrap();
    let requests = fixture.requests.lock().await;
    let sent: Vec<_> = requests
        .iter()
        .filter(|(route, _)| route.ends_with("/events"))
        .collect();
    assert_eq!(sent.len(), 2);
    for (_, body) in &sent {
        let mut keys: Vec<_> = body
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort();
        assert_eq!(keys, ["deviceId", "eventId", "generation", "hostId", "id"]);
        assert_eq!(body["deviceId"], principal);
        assert_eq!(body["hostId"], "host-1");
        assert_eq!(body["eventId"].as_str().unwrap().len(), 32);
        assert!(!body.to_string().contains("SECRET"));
    }
    assert_ne!(sent[0].1["eventId"], sent[1].1["eventId"]);
}

#[tokio::test]
async fn status_and_disable_are_scoped_and_disable_allows_later_rebinding() {
    let fixture = Fixture::new().await;
    let a = fixture.pair().await;
    let b = fixture.pair().await;
    fixture.bind(&a, "subscription-a").await;
    fixture.bind(&b, "subscription-b").await;
    let status = fixture
        .notifications
        .dispatch(&a, command(json!({"kind":"getNotificationStatus"})))
        .await;
    assert_eq!(
        status.1["result"]["registrations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        status.1["result"]["registrations"][0]["id"],
        "subscription-a"
    );
    let disable = fixture
        .notifications
        .dispatch(&a, command(json!({"kind":"disableNotifications"})))
        .await;
    assert_eq!(disable.0, 200);
    fixture.emit_turn().await;
    fixture.notifications.sync().await.unwrap();
    let requests = fixture.requests.lock().await;
    assert!(!requests.iter().any(|(route, _)| route.ends_with("/revoke")));
    let sent: Vec<_> = requests
        .iter()
        .filter(|(route, _)| route.ends_with("/events"))
        .collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1["deviceId"], b.session_id());
    drop(requests);
    fixture.bind(&a, "subscription-a").await;
}

#[tokio::test]
async fn persisted_binding_has_no_token_and_revocation_prevents_further_notifications() {
    let fixture = Fixture::new().await;
    let access = fixture.pair().await;
    fixture.bind(&access, "subscription-a").await;
    let bytes = std::fs::read_to_string(
        fixture
            .directory
            .path()
            .join("remote-notifications-host-1.json"),
    )
    .unwrap();
    assert!(!bytes.contains("synthetic-token"));
    assert!(!bytes.contains("synthetic-installation-bearer"));
    let restarted = Notifications::with_loopback_api(
        fixture.host.clone(),
        fixture.directory.path().into(),
        "host-1".into(),
        reqwest::Client::new(),
        Arc::new(TestCredentials),
        &fixture.api,
    )
    .await
    .unwrap();
    let status = restarted
        .dispatch(&access, command(json!({"kind":"getNotificationStatus"})))
        .await;
    assert_eq!(
        status.1["result"]["registrations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fixture
        .host
        .pairing
        .lock()
        .await
        .revoke(access.session_id());
    fixture.emit_turn().await;
    restarted.sync().await.unwrap();
    let requests = fixture.requests.lock().await;
    assert_eq!(
        requests
            .iter()
            .filter(|(route, _)| route.ends_with("/devices/revoke"))
            .count(),
        1
    );
    assert!(!requests.iter().any(|(route, _)| route.ends_with("/events")));
    assert!(restarted.state.lock().await.registrations.is_empty());
}

#[tokio::test]
async fn uncertain_push_is_not_replayed_and_gone_binding_is_removed() {
    let fixture = Fixture::new().await;
    let access = fixture.pair().await;
    fixture.bind(&access, "subscription-a").await;
    *fixture.event_status.lock().await = 503;
    fixture.host.emit_event(permission("one"), None).await;
    fixture.notifications.sync().await.unwrap();
    fixture.notifications.sync().await.unwrap();
    fixture.host.emit_event(permission("one"), None).await;
    fixture.notifications.sync().await.unwrap();
    assert_eq!(
        fixture
            .requests
            .lock()
            .await
            .iter()
            .filter(|(route, _)| route.ends_with("/events"))
            .count(),
        1
    );
    *fixture.event_status.lock().await = 410;
    fixture.host.emit_event(permission("two"), None).await;
    fixture.notifications.sync().await.unwrap();
    assert!(
        fixture
            .notifications
            .state
            .lock()
            .await
            .registrations
            .is_empty()
    );
}

#[tokio::test]
async fn forged_identity_and_missing_effect_key_do_not_contact_backend() {
    let fixture = Fixture::new().await;
    let access = fixture.pair().await;
    let forged = fixture.notifications.dispatch(&access, command(json!({"kind":"configureNotifications","id":"a","bindingToken":"synthetic-token","generation":3,"deviceId":"other"}))).await;
    assert_eq!(forged.0, 400);
    let mut missing = command(
        json!({"kind":"configureNotifications","id":"a","bindingToken":"synthetic-token","generation":3}),
    );
    missing.as_object_mut().unwrap().remove("idempotencyKey");
    assert_eq!(
        fixture.notifications.dispatch(&access, missing).await.0,
        400
    );
    assert!(fixture.requests.lock().await.is_empty());
}

struct DelayedCredentials {
    blocked: std::sync::atomic::AtomicBool,
    arrived: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

struct UnavailableCredentials;

#[async_trait::async_trait]
impl Credentials for UnavailableCredentials {
    async fn access_token(&self) -> Result<zeroize::Zeroizing<String>, String> {
        std::future::pending().await
    }
}

#[tokio::test(start_paused = true)]
async fn unavailable_credential_refresh_has_a_bounded_deadline() {
    let fixture = Fixture::new().await;
    let access = fixture.pair().await;
    let notifications = Notifications::with_loopback_api(
        fixture.host.clone(),
        fixture.directory.path().into(),
        "host-1".into(),
        reqwest::Client::new(),
        Arc::new(UnavailableCredentials),
        &fixture.api,
    )
    .await
    .unwrap();
    let started = tokio::time::Instant::now();
    let reply = notifications.dispatch(&access, command(json!({"kind":"configureNotifications","id":"subscription-a","bindingToken":"synthetic-token","generation":3}))).await;
    assert_eq!(reply.0, 503);
    assert_eq!(reply.1["error"], "notification_login_unavailable");
    assert_eq!(started.elapsed(), Duration::from_secs(5));
    assert!(fixture.requests.lock().await.is_empty());
    assert!(notifications.state.lock().await.registrations.is_empty());
}

#[async_trait::async_trait]
impl Credentials for DelayedCredentials {
    async fn access_token(&self) -> Result<zeroize::Zeroizing<String>, String> {
        if self.blocked.load(std::sync::atomic::Ordering::SeqCst) {
            self.arrived.notify_one();
            self.release.notified().await;
        }
        TestCredentials.access_token().await
    }
}

#[tokio::test]
async fn disable_or_revoke_while_credentials_refresh_prevents_the_pending_http_send() {
    for revoke in [false, true] {
        let fixture = Fixture::new().await;
        let access = fixture.pair().await;
        fixture.bind(&access, "subscription-a").await;
        let credentials = Arc::new(DelayedCredentials {
            blocked: std::sync::atomic::AtomicBool::new(true),
            arrived: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        let notifications = Arc::new(
            Notifications::with_loopback_api(
                fixture.host.clone(),
                fixture.directory.path().into(),
                "host-1".into(),
                reqwest::Client::new(),
                credentials.clone(),
                &fixture.api,
            )
            .await
            .unwrap(),
        );
        fixture.emit_turn().await;
        let serving = notifications.clone();
        let worker = tokio::spawn(async move { serving.sync().await });
        tokio::time::timeout(Duration::from_secs(2), credentials.arrived.notified())
            .await
            .unwrap();
        if revoke {
            fixture
                .host
                .pairing
                .lock()
                .await
                .revoke(access.session_id());
        } else {
            let reply = notifications
                .dispatch(&access, command(json!({"kind":"disableNotifications"})))
                .await;
            assert_eq!(reply.0, 200);
        }
        credentials
            .blocked
            .store(false, std::sync::atomic::Ordering::SeqCst);
        credentials.release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        notifications.sync().await.unwrap();
        let requests = fixture.requests.lock().await;
        assert!(!requests.iter().any(|(route, _)| route.ends_with("/events")));
        assert_eq!(
            requests
                .iter()
                .filter(|(route, _)| route.ends_with("/devices/revoke"))
                .count(),
            usize::from(revoke)
        );
    }
}

#[tokio::test]
async fn local_rate_limit_bounds_attempts_even_when_the_provider_is_unavailable() {
    let fixture = Fixture::new().await;
    let access = fixture.pair().await;
    fixture.bind(&access, "subscription-a").await;
    *fixture.event_status.lock().await = 503;
    for index in 0..65 {
        fixture
            .host
            .emit_event(permission(&format!("permission-{index}")), None)
            .await;
        fixture.notifications.sync().await.unwrap();
    }
    assert_eq!(
        fixture
            .requests
            .lock()
            .await
            .iter()
            .filter(|(route, _)| route.ends_with("/events"))
            .count(),
        30
    );
}

#[tokio::test]
async fn noise_capability_and_configuration_are_explicitly_disabled_by_default() {
    let fixture = Fixture::new().await;
    let mut channel =
        crate::tests::TestChannel::with_state(fixture.host.clone(), crate::EventDelivery::Polling)
            .await;
    channel.send(json!({"id":"capabilities","method":"command","body":command(json!({"kind":"getRemoteCapabilities"}))})).await;
    let capabilities = channel.receive().await;
    assert_eq!(capabilities["body"]["result"]["pushNotifications"], false);
    channel.send(json!({"id":"status","method":"command","body":command(json!({"kind":"getNotificationStatus"}))})).await;
    let status = channel.receive().await;
    assert_eq!(status["body"]["result"]["enabled"], false);
    channel.send(json!({"id":"configure","method":"command","body":command(json!({"kind":"configureNotifications","id":"subscription-a","bindingToken":"synthetic-token","generation":3}))})).await;
    let refused = channel.receive().await;
    assert_eq!(refused["status"], 422);
    assert!(fixture.requests.lock().await.is_empty());
    drop(channel.input);
    channel.task.await.unwrap().unwrap();
}

#[tokio::test]
async fn disabled_remote_configuration_stops_new_http_events_before_worker_shutdown() {
    let fixture = Fixture::new().await;
    let access = fixture.pair().await;
    fixture.bind(&access, "subscription-a").await;
    let identity = fabrials_remote_noise::identity().unwrap();
    let mut config = crate::state::Config {
        enabled: false,
        host_id: "host-1".into(),
        public_key: crate::state::hex(&identity.public),
        private_key: identity.private.to_vec(),
        label: "Synthetic host".into(),
    };
    crate::state::save(fixture.directory.path(), &config).unwrap();
    let notifications = Notifications::with_api(
        fixture.host.clone(),
        fixture.directory.path().into(),
        "host-1".into(),
        reqwest::Client::new(),
        Arc::new(TestCredentials),
        &fixture.api,
        true,
    )
    .await
    .unwrap();
    fixture.emit_turn().await;
    notifications.sync().await.unwrap();
    assert!(
        !fixture
            .requests
            .lock()
            .await
            .iter()
            .any(|(route, _)| route.ends_with("/events"))
    );
    config.enabled = true;
    crate::state::save(fixture.directory.path(), &config).unwrap();
    fixture.emit_turn().await;
    notifications.sync().await.unwrap();
    assert_eq!(
        fixture
            .requests
            .lock()
            .await
            .iter()
            .filter(|(route, _)| route.ends_with("/events"))
            .count(),
        1
    );
}

#[cfg(unix)]
#[tokio::test]
async fn persistent_bindings_reject_broad_permissions_and_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let fixture = Fixture::new().await;
    let access = fixture.pair().await;
    fixture.bind(&access, "subscription-a").await;
    let path = fixture
        .directory
        .path()
        .join("remote-notifications-host-1.json");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let open = || {
        Notifications::with_loopback_api(
            fixture.host.clone(),
            fixture.directory.path().into(),
            "host-1".into(),
            reqwest::Client::new(),
            Arc::new(TestCredentials),
            &fixture.api,
        )
    };
    assert!(matches!(open().await, Err(error) if error == "notification_state_not_private"));
    let moved = fixture.directory.path().join("moved-bindings.json");
    std::fs::rename(&path, &moved).unwrap();
    std::fs::set_permissions(&moved, std::fs::Permissions::from_mode(0o600)).unwrap();
    symlink(moved, path).unwrap();
    assert!(matches!(open().await, Err(error) if error == "invalid_notification_state"));
}
