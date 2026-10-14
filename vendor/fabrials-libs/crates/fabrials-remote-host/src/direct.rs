use axum::extract::ws::{Message as AxumMessage, WebSocket};
use base64::{Engine, engine::general_purpose::STANDARD};
use fabrials_agent_host::server::{DirectConnection, HostState};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{fmt, path::Path, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use zeroize::Zeroize;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub installation_id: String,
    pub endpoint: String,
    #[serde(default = "default_web_origin")]
    pub web_origin: String,
    pub public_key: String,
    private_key: Vec<u8>,
}

fn default_web_origin() -> String {
    fabrials_agent_host::origin::PRODUCTION_WEB_ORIGIN.into()
}

impl Drop for Identity {
    fn drop(&mut self) {
        self.private_key.zeroize();
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output
            .debug_struct("DirectIdentity")
            .field("installation_id", &self.installation_id)
            .finish_non_exhaustive()
    }
}

pub fn load(directory: &Path) -> Result<Identity, String> {
    let path = directory.join("direct.json");
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| "direct_not_enabled")?;
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err("direct_identity_invalid".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("direct_identity_not_private".into());
        }
    }
    #[cfg(windows)]
    if !fabrials_agent_host::win_acl::is_owner_only(&path)
        .map_err(|_| "direct_identity_not_private")?
    {
        return Err("direct_identity_not_private".into());
    }
    let bytes =
        zeroize::Zeroizing::new(std::fs::read(path).map_err(|_| "direct_identity_unavailable")?);
    let identity: Identity =
        serde_json::from_slice(&bytes).map_err(|_| "direct_identity_invalid")?;
    if !fabrials_agent_host::origin::is_valid_install_id(&identity.installation_id)
        || fabrials_agent_host::origin::validate_private_origin(&identity.endpoint).is_err()
        || fabrials_agent_host::origin::validate_hosted_web_origin(&identity.web_origin).is_err()
        || identity.private_key.len() != 32
        || identity.public_key.len() != 64
        || !identity
            .public_key
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("direct_identity_invalid".into());
    }
    Ok(identity)
}

pub fn prepare(
    directory: &Path,
    installation_id: &str,
    endpoint: &str,
) -> Result<Identity, String> {
    prepare_for_web_origin(directory, installation_id, endpoint, &default_web_origin())
}

pub fn validate_configuration(
    directory: &Path,
    endpoint: &str,
    web_origin: &str,
) -> Result<(), String> {
    fabrials_agent_host::origin::validate_private_origin(endpoint)
        .map_err(|_| "invalid_private_origin")?;
    fabrials_agent_host::origin::validate_hosted_web_origin(web_origin)
        .map_err(|_| "invalid_hosted_web_origin")?;
    match load(directory) {
        Ok(identity) if identity.endpoint != endpoint || identity.web_origin != web_origin => {
            Err("direct_origin_mismatch: use the configured endpoint and web origin; changing either requires explicit migration and re-pairing".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error == "direct_not_enabled" && !directory.join("direct.json").exists() => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn prepare_for_web_origin(
    directory: &Path,
    installation_id: &str,
    endpoint: &str,
    web_origin: &str,
) -> Result<Identity, String> {
    validate_configuration(directory, endpoint, web_origin)?;
    match load(directory) {
        Ok(identity) => {
            if identity.installation_id != installation_id
                || identity.endpoint != endpoint
                || identity.web_origin != web_origin
            {
                return Err("direct_identity_mismatch".into());
            }
            Ok(identity)
        }
        Err(error) if error == "direct_not_enabled" && !directory.join("direct.json").exists() => {
            let key =
                fabrials_remote_noise::identity().map_err(|_| "direct_identity_unavailable")?;
            let identity = Identity {
                installation_id: installation_id.into(),
                endpoint: endpoint.into(),
                web_origin: web_origin.into(),
                public_key: crate::state::hex(&key.public),
                private_key: key.private.to_vec(),
            };
            crate::state::write_private_json(directory, "direct.json", &identity)?;
            Ok(identity)
        }
        Err(error) => Err(error),
    }
}

pub struct DirectHost {
    identity: Identity,
    publisher: Arc<crate::shares::Shares>,
    notifications: Option<Arc<dyn crate::notifications::NotificationService>>,
}

impl fmt::Debug for DirectHost {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output
            .debug_struct("DirectHost")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl DirectHost {
    pub async fn new(
        directory: &Path,
        identity: Identity,
        host: &Arc<HostState>,
    ) -> Result<Self, String> {
        let publisher = Arc::new(crate::shares::Shares::direct(
            host,
            directory,
            &identity.public_key,
            &identity.web_origin,
        )?);
        let worker = publisher.clone();
        let owner = Arc::downgrade(host);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                let Some(host) = owner.upgrade() else {
                    break;
                };
                tokio::select! {
                    _ = host.shutdown_requested() => break,
                    _ = interval.tick() => { let _ = worker.sync().await; }
                }
            }
        });
        let notifications: Option<Arc<dyn crate::notifications::NotificationService>> =
            match crate::direct_notifications::DirectNotifications::new(
                host,
                directory,
                &identity.endpoint,
            )
            .await
            {
                Ok(service) => {
                    let service: Arc<dyn crate::notifications::NotificationService> =
                        Arc::new(service);
                    let worker = service.clone();
                    tokio::spawn(async move { worker.run().await });
                    Some(service)
                }
                Err(_) => {
                    eprintln!(
                        "Direct notifications are unavailable: repair this computer's private notification state and restart the host."
                    );
                    None
                }
            };
        Ok(Self {
            identity,
            publisher,
            notifications,
        })
    }
}

#[async_trait::async_trait]
impl DirectConnection for DirectHost {
    async fn serve(&self, host: Arc<HostState>, socket: WebSocket) {
        let _ = self.drive(host, socket).await;
    }
}

impl DirectHost {
    async fn drive(&self, host: Arc<HostState>, socket: WebSocket) -> Result<(), String> {
        let mut entropy = [0u8; 32];
        getrandom::fill(&mut entropy).map_err(|_| "entropy_unavailable")?;
        let channel_id = crate::state::hex(&entropy);
        let (mut sink, mut stream) = socket.split();
        tokio::time::timeout(
            Duration::from_secs(5),
            sink.send(AxumMessage::Text(
                json!({"type":"ready","channelId":channel_id})
                    .to_string()
                    .into(),
            )),
        )
        .await
        .map_err(|_| "direct_timeout")?
        .map_err(|_| "direct_closed")?;
        let (incoming, receiver) = mpsc::channel(16);
        let (output, mut outgoing) = mpsc::channel(64);
        let worker = crate::drive_channel_with_delivery(
            host.clone(),
            &self.identity.installation_id,
            &channel_id,
            &self.identity.private_key,
            receiver,
            output,
            self.publisher.clone(),
            crate::EventDelivery::Notified,
            self.notifications.clone(),
        );
        tokio::pin!(worker);
        let mut last_seen = tokio::time::Instant::now();
        let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
        let mut completed = false;
        let result = loop {
            tokio::select! {
                result = &mut worker => { completed = true; break result; },
                _ = host.shutdown_requested() => break Ok(()),
                Some(message) = outgoing.recv() => {
                    let Message::Text(text) = message else { break Err("direct_protocol".into()); };
                    if !matches!(tokio::time::timeout(Duration::from_secs(5), sink.send(AxumMessage::Text(text.as_str().into()))).await, Ok(Ok(()))) { break Err("direct_closed".into()); }
                }
                _ = heartbeat.tick() => {
                    if last_seen.elapsed() > Duration::from_secs(30) { break Err("direct_timeout".into()); }
                    if !matches!(tokio::time::timeout(Duration::from_secs(5), sink.send(AxumMessage::Ping(Vec::new().into()))).await, Ok(Ok(()))) { break Err("direct_closed".into()); }
                }
                message = stream.next() => {
                    match message {
                        Some(Ok(AxumMessage::Text(text))) => {
                            last_seen = tokio::time::Instant::now();
                            let Ok(frame) = serde_json::from_str::<crate::RelayFrame>(&text) else { break Err("direct_protocol".into()); };
                            let crate::RelayFrame::Data { channel_id: id, payload } = frame else { break Err("direct_protocol".into()); };
                            if id != channel_id || payload.len() > 87_380 { break Err("direct_protocol".into()); }
                            let Ok(bytes) = STANDARD.decode(payload) else { break Err("direct_protocol".into()); };
                            if bytes.len() > 65_535 || incoming.try_send(bytes).is_err() { break Err("direct_backpressure".into()); }
                        }
                        Some(Ok(AxumMessage::Pong(_))) => last_seen = tokio::time::Instant::now(),
                        Some(Ok(AxumMessage::Ping(bytes))) => {
                            if !matches!(tokio::time::timeout(Duration::from_secs(5), sink.send(AxumMessage::Pong(bytes))).await, Ok(Ok(()))) { break Err("direct_closed".into()); }
                        }
                        _ => break Ok(()),
                    }
                }
            }
        };
        let close_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        drop(incoming);
        if completed {
            while let Ok(Message::Text(text)) = outgoing.try_recv() {
                if !matches!(
                    tokio::time::timeout_at(
                        close_deadline,
                        sink.send(AxumMessage::Text(text.as_str().into()))
                    )
                    .await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
        }
        drop(outgoing);
        if !completed {
            let _ = tokio::time::timeout_at(close_deadline, worker).await;
        }
        close_transport(sink, stream, close_deadline).await;
        result
    }
}

async fn close_transport(
    mut sink: futures_util::stream::SplitSink<WebSocket, AxumMessage>,
    mut stream: futures_util::stream::SplitStream<WebSocket>,
    deadline: tokio::time::Instant,
) {
    let _ = tokio::time::timeout_at(deadline, async {
        if sink.close().await.is_err() {
            return;
        }
        let mut received = 0;
        for _ in 0..64 {
            match stream.next().await {
                Some(Ok(AxumMessage::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(message)) => {
                    received += message.into_data().len();
                    if received >= 2 * 1024 * 1024 {
                        break;
                    }
                }
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabrials_remote_noise::{Channel, Handshake};
    use serde_json::Value;
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream, tungstenite::client::IntoClientRequest,
    };

    type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

    async fn closing_socket() -> (
        Socket,
        tokio::sync::oneshot::Receiver<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (finished, receiver) = tokio::sync::oneshot::channel();
        let finished = Arc::new(std::sync::Mutex::new(Some(finished)));
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(move |upgrade: axum::extract::WebSocketUpgrade| {
                let finished = finished.lock().unwrap().take().unwrap();
                async move {
                    upgrade
                        .max_message_size(90 * 1024)
                        .max_frame_size(90 * 1024)
                        .on_upgrade(move |socket| async move {
                            let (sink, stream) = socket.split();
                            close_transport(
                                sink,
                                stream,
                                tokio::time::Instant::now() + Duration::from_secs(5),
                            )
                            .await;
                            let _ = finished.send(());
                        })
                }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/"))
            .await
            .unwrap();
        (socket, receiver, server)
    }

    async fn expect_tcp_eof(socket: &mut Socket) {
        use tokio::io::AsyncReadExt;
        let MaybeTlsStream::Plain(stream) = socket.get_mut() else {
            panic!("expected the loopback TCP transport");
        };
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), stream.read(&mut [0]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn close_handshake_waits_for_peer_after_draining_pending_frames() {
        let (mut socket, mut finished, server) = closing_socket().await;
        for _ in 0..8 {
            socket.send(Message::Text("pending".into())).await.unwrap();
        }
        assert!(matches!(socket.next().await, Some(Ok(Message::Close(_)))));
        assert!(matches!(
            finished.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        socket.flush().await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), finished)
            .await
            .unwrap()
            .unwrap();
        expect_tcp_eof(&mut socket).await;
        assert!(socket.next().await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn close_handshake_has_a_fixed_deadline_without_peer_acknowledgement() {
        let (mut socket, mut finished, server) = closing_socket().await;
        assert!(matches!(socket.next().await, Some(Ok(Message::Close(_)))));
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(4)).await;
        assert!(matches!(
            finished.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        tokio::time::timeout(Duration::from_secs(2), finished)
            .await
            .unwrap()
            .unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn close_handshake_caps_unacknowledged_input() {
        for (frames, bytes) in [(64, 1), (24, 90 * 1024)] {
            let (mut socket, finished, server) = closing_socket().await;
            for _ in 0..frames {
                if socket
                    .send(Message::Binary(vec![0; bytes].into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            tokio::time::timeout(Duration::from_secs(1), finished)
                .await
                .unwrap()
                .unwrap();
            server.abort();
        }
    }

    async fn receive(socket: &mut Socket) -> Value {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            match frame {
                Message::Text(value) => return serde_json::from_str(&value).unwrap(),
                Message::Ping(value) => socket.send(Message::Pong(value)).await.unwrap(),
                other => panic!("unexpected frame {other:?}"),
            }
        }
    }

    async fn send(socket: &mut Socket, id: &str, data: Vec<u8>) {
        socket
            .send(Message::Text(
                json!({"type":"data","channelId":id,"payload":STANDARD.encode(data)})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
    }

    async fn plain(socket: &mut Socket, channel: &mut Channel) -> Value {
        loop {
            let value = receive(socket).await;
            assert_eq!(value["type"], "data");
            if let Some(bytes) = channel
                .decrypt(&STANDARD.decode(value["payload"].as_str().unwrap()).unwrap())
                .unwrap()
            {
                return serde_json::from_slice(&bytes).unwrap();
            }
        }
    }

    async fn connect(
        port: u16,
        origin: &str,
    ) -> Result<Socket, tokio_tungstenite::tungstenite::Error> {
        let mut request = format!("ws://127.0.0.1:{port}/remote")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("host", "host.example:8443".parse().unwrap());
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
        request.headers_mut().insert(
            "sec-websocket-protocol",
            "fabrials.remote.v1".parse().unwrap(),
        );
        tokio_tungstenite::connect_async(request)
            .await
            .map(|(socket, _)| socket)
    }

    #[tokio::test]
    async fn direct_noise_pairs_commands_and_revokes_without_cloud() {
        exercise_direct_noise(fabrials_agent_host::origin::PRODUCTION_WEB_ORIGIN, false).await;
    }

    #[tokio::test]
    async fn qualification_origin_pairs_without_accepting_other_hosted_origins() {
        exercise_direct_noise(fabrials_agent_host::origin::QUALIFICATION_WEB_ORIGIN, false).await;
    }

    #[tokio::test]
    async fn self_revocation_acknowledges_before_close_with_pending_commands() {
        for _ in 0..32 {
            exercise_direct_noise(fabrials_agent_host::origin::QUALIFICATION_WEB_ORIGIN, true)
                .await;
        }
    }

    async fn exercise_direct_noise(web_origin: &str, forget: bool) {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("state");
        fabrials_agent_host::instance::ensure_private_directory(&directory).unwrap();
        let identity = fabrials_agent_host::state::load_or_create(&directory).unwrap();
        let direct = prepare_for_web_origin(
            &directory,
            &identity.install_id,
            "https://host.example:8443",
            web_origin,
        )
        .unwrap();
        let key: [u8; 32] = (0..32)
            .map(|i| u8::from_str_radix(&direct.public_key[i * 2..i * 2 + 2], 16).unwrap())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let host = Arc::new(
            HostState::new(identity.origin().unwrap())
                .with_allowed_origins(vec![web_origin.into()])
                .with_agent_mode(if forget {
                    fabrials_agent_host::acp::AgentMode::Shared
                } else {
                    fabrials_agent_host::acp::AgentMode::Isolated
                })
                .with_private_origin(Some(direct.endpoint.clone()))
                .unwrap(),
        );
        host.enable_direct_connection(Arc::new(
            DirectHost::new(&directory, direct, &host).await.unwrap(),
        ))
        .unwrap();
        let listeners = fabrials_agent_host::server::bind(&host.origin)
            .await
            .unwrap();
        let server = tokio::spawn(fabrials_agent_host::server::serve(listeners, host.clone()));
        assert!(
            connect(host.origin.port(), "https://desktop.grok.me.evil.example")
                .await
                .is_err()
        );
        for rejected in [
            "https://grok.fabrials.com.evil.example",
            "https://grok.fabrials.com:8443",
            "http://grok.fabrials.com",
            if web_origin == fabrials_agent_host::origin::PRODUCTION_WEB_ORIGIN {
                fabrials_agent_host::origin::QUALIFICATION_WEB_ORIGIN
            } else {
                fabrials_agent_host::origin::PRODUCTION_WEB_ORIGIN
            },
        ] {
            assert!(
                connect(host.origin.port(), rejected).await.is_err(),
                "{rejected}"
            );
        }
        let nonce = host
            .pairing
            .lock()
            .await
            .mint_nonce(fabrials_agent_host::now_ms())
            .unwrap();
        let mut socket = connect(host.origin.port(), web_origin).await.unwrap();
        let ready = receive(&mut socket).await;
        assert_eq!(ready["type"], "ready");
        let channel_id = ready["channelId"].as_str().unwrap();
        let browser = fabrials_remote_noise::identity().unwrap();
        let mut handshake = Handshake::new(
            &browser.private,
            Some(key),
            &identity.install_id,
            channel_id,
        )
        .unwrap();
        send(&mut socket, channel_id, handshake.write(&[]).unwrap()).await;
        let response = receive(&mut socket).await;
        handshake
            .read(
                &STANDARD
                    .decode(response["payload"].as_str().unwrap())
                    .unwrap(),
            )
            .unwrap();
        send(
            &mut socket,
            channel_id,
            handshake
                .write(&serde_json::to_vec(&json!({"nonce":nonce.expose()})).unwrap())
                .unwrap(),
        )
        .await;
        let mut channel = handshake.finish().unwrap();
        let paired = plain(&mut socket, &mut channel).await;
        assert_eq!(paired["type"], "paired");
        assert_eq!(paired["body"]["installationId"], identity.install_id);
        let session_id = paired["body"]["sessionId"].as_str().unwrap();
        let request = json!({"id":"req1","method":"command","body":{"protocolVersion":2,"requestId":"request1","operation":{"kind":"getRemoteCapabilities"}}});
        for frame in channel
            .encrypt(&serde_json::to_vec(&request).unwrap())
            .unwrap()
        {
            send(&mut socket, channel_id, frame).await;
        }
        let response = plain(&mut socket, &mut channel).await;
        assert_eq!(response["type"], "response");
        assert_eq!(response["status"], 202);
        let preserved_pairing = if forget {
            let mut pairing = host.pairing.lock().await;
            let now = fabrials_agent_host::now_ms();
            let nonce = pairing.mint_nonce(now).unwrap();
            Some(
                pairing
                    .redeem_nonce(nonce.expose(), now)
                    .unwrap()
                    .session_id,
            )
        } else {
            None
        };
        if forget {
            for request in [
                json!({"id":"subscribe","method":"subscribe","body":{"afterSequence":0,"protocolVersion":4}}),
                json!({"id":"forget","method":"command","body":{"protocolVersion":4,"requestId":"forget","idempotencyKey":"forget","operation":{"kind":"forgetBrowserPairing"}}}),
                json!({"id":"late-effect","method":"command","body":{"protocolVersion":4,"requestId":"late-effect","idempotencyKey":"late-effect","operation":{"kind":"revokeBrowserPairing","sessionId":preserved_pairing}}}),
            ] {
                for frame in channel
                    .encrypt(&serde_json::to_vec(&request).unwrap())
                    .unwrap()
                {
                    send(&mut socket, channel_id, frame).await;
                }
            }
            for n in 0..8 {
                let request = json!({"id":format!("late-{n}"),"method":"command","body":{"protocolVersion":4,"requestId":format!("late-{n}"),"operation":{"kind":"bootstrap"}}});
                for frame in channel
                    .encrypt(&serde_json::to_vec(&request).unwrap())
                    .unwrap()
                {
                    send(&mut socket, channel_id, frame).await;
                }
            }
            loop {
                let response = plain(&mut socket, &mut channel).await;
                assert_ne!(
                    response["type"], "closed",
                    "closed before revocation acknowledgement"
                );
                if response["id"] == "forget" {
                    assert_eq!(response["status"], 202);
                    assert_eq!(response["body"]["result"]["outcome"], "acknowledged");
                    break;
                }
            }
        } else {
            host.pairing.lock().await.revoke(session_id);
        }
        let revoked = plain(&mut socket, &mut channel).await;
        assert_eq!(revoked, json!({"type":"closed","reason":"revoked"}));
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap(),
            Some(Ok(Message::Close(_)))
        ));
        socket.flush().await.unwrap();
        expect_tcp_eof(&mut socket).await;
        assert!(
            tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap()
                .is_none()
        );
        if let Some(preserved_pairing) = preserved_pairing {
            assert!(
                host.pairing
                    .lock()
                    .await
                    .has_session(&preserved_pairing, fabrials_agent_host::now_ms())
            );
        }
        host.request_shutdown();
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[test]
    fn direct_identity_survives_restart_and_refuses_endpoint_or_installation_changes() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("state");
        let install = "0123456789abcdef0123456789abcdef";
        let first = prepare(&directory, install, "https://host.example:8443").unwrap();
        let second = prepare(&directory, install, "https://host.example:8443").unwrap();
        assert_eq!(first.public_key, second.public_key);
        assert_eq!(first.private_key, second.private_key);
        assert!(prepare(&directory, install, "https://other.example").is_err());
        assert!(
            prepare(
                &directory,
                "fedcba9876543210fedcba9876543210",
                "https://host.example:8443"
            )
            .is_err()
        );
        std::fs::write(directory.join("direct.json"), b"invalid").unwrap();
        assert!(prepare(&directory, install, "https://host.example:8443").is_err());
    }

    #[test]
    fn configured_web_origin_is_durable_and_legacy_state_stays_canonical() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("state");
        let install = "0123456789abcdef0123456789abcdef";
        let endpoint = "https://host.example:8443";
        let origin = fabrials_agent_host::origin::QUALIFICATION_WEB_ORIGIN;
        let first = prepare_for_web_origin(&directory, install, endpoint, origin).unwrap();
        let persisted = std::fs::read(directory.join("direct.json")).unwrap();
        assert_eq!(first.web_origin, origin);
        assert_eq!(load(&directory).unwrap().web_origin, origin);
        assert_eq!(
            prepare_for_web_origin(&directory, install, endpoint, origin)
                .unwrap()
                .public_key,
            first.public_key
        );
        assert!(prepare(&directory, install, endpoint).is_err());
        assert_eq!(
            std::fs::read(directory.join("direct.json")).unwrap(),
            persisted
        );
        let mut legacy: Value = serde_json::from_slice(&persisted).unwrap();
        legacy.as_object_mut().unwrap().remove("webOrigin");
        crate::state::write_private_json(&directory, "direct.json", &legacy).unwrap();
        let loaded = prepare(&directory, install, endpoint).unwrap();
        assert_eq!(
            loaded.web_origin,
            fabrials_agent_host::origin::PRODUCTION_WEB_ORIGIN
        );
        assert_eq!(loaded.public_key, first.public_key);
        assert!(prepare_for_web_origin(&directory, install, endpoint, origin).is_err());
        legacy["webOrigin"] = json!("https://grok.fabrials.com.evil.example");
        crate::state::write_private_json(&directory, "direct.json", &legacy).unwrap();
        assert!(load(&directory).is_err());
    }

    #[test]
    fn invalid_web_origin_is_rejected_without_creating_state() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("state");
        assert!(
            prepare_for_web_origin(
                &directory,
                "0123456789abcdef0123456789abcdef",
                "https://host.example:8443",
                "https://grok.fabrials.com/",
            )
            .is_err()
        );
        assert!(!directory.exists());
    }

    #[tokio::test]
    async fn damaged_optional_push_state_does_not_prevent_direct_connections() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("state");
        fabrials_agent_host::instance::ensure_private_directory(&directory).unwrap();
        let identity = fabrials_agent_host::state::load_or_create(&directory).unwrap();
        let direct = prepare(
            &directory,
            &identity.install_id,
            "https://host.example:8443",
        )
        .unwrap();
        crate::state::write_private_json(&directory, "direct-vapid.json", &vec![0u8]).unwrap();
        let host = Arc::new(
            HostState::new(identity.origin().unwrap())
                .with_private_origin(Some(direct.endpoint.clone()))
                .unwrap(),
        );
        let connection = DirectHost::new(&directory, direct, &host).await.unwrap();
        assert!(connection.notifications.is_none());
        host.enable_direct_connection(Arc::new(connection)).unwrap();
        assert_eq!(
            std::fs::read_to_string(directory.join("direct-vapid.json"))
                .unwrap()
                .trim(),
            "[0]"
        );
    }
}
