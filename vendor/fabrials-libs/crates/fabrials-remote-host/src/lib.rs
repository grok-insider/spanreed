use base64::{Engine, engine::general_purpose::STANDARD};
use fabrials_agent_host::{journal::ReplayOutcome, remote::RemoteAccess, server::HostState};
use fabrials_remote_noise::{Channel, Handshake};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Semaphore, mpsc};
use tokio_tungstenite::tungstenite::Message;
use zeroize::Zeroizing;

pub mod direct;
pub mod direct_notifications;
pub mod notifications;
pub mod publications;
pub mod shares;
pub mod state;
pub mod status;

#[cfg(test)]
mod event_experiment;
#[cfg(test)]
mod network_fixture;

pub const API: &str = "https://fabrials.com/api/spanreed/remote";
pub const RELAY: &str = "wss://remote-relay.fabrials.com/connect";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum EventDelivery {
    #[default]
    Polling,
    Notified,
}

impl EventDelivery {
    fn configured() -> Result<Self, String> {
        match std::env::var("SPANREED_REMOTE_EXPERIMENT_EVENT_DELIVERY").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("polling") => Ok(Self::Polling),
            Ok("notified") => Ok(Self::Notified),
            _ => Err("invalid_experimental_event_delivery".into()),
        }
    }
}

#[async_trait::async_trait]
pub trait Credentials: Send + Sync {
    async fn access_token(&self) -> Result<Zeroizing<String>, String>;
}

pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "remote_http_unavailable".into())
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum RelayFrame {
    Ready {
        #[serde(rename = "channelId")]
        channel_id: Option<String>,
    },
    Open {
        #[serde(rename = "channelId")]
        channel_id: String,
    },
    Data {
        #[serde(rename = "channelId")]
        channel_id: String,
        payload: String,
    },
    Closed {
        #[serde(rename = "channelId")]
        channel_id: String,
        reason: String,
    },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Authentication {
    Pair(PairAuthentication),
    Resume(ResumeAuthentication),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PairAuthentication {
    nonce: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResumeAuthentication {
    session_token: String,
    csrf_token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: String,
    method: Method,
    #[serde(default)]
    body: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Method {
    Command,
    Subscribe,
    Heartbeat,
}

pub async fn run(
    host: Arc<HostState>,
    directory: PathBuf,
    credentials: Arc<dyn Credentials>,
) -> Result<(), String> {
    let client = http_client()?;
    let notifications: Option<Arc<dyn notifications::NotificationService>> =
        if notifications::configured() {
            let config = state::load(&directory)?;
            Some(Arc::new(
                notifications::Notifications::new(
                    host.clone(),
                    directory.clone(),
                    config.host_id.clone(),
                    client.clone(),
                    credentials.clone(),
                )
                .await?,
            ))
        } else {
            None
        };
    let publisher = Arc::new(shares::Shares::new(
        host.clone(),
        directory.clone(),
        client.clone(),
    )?);
    let publication = publisher.clone();
    let publish_host = host.clone();
    let monitor = Arc::new(status::Monitor::new(&directory)?);
    let publication_monitor = monitor.clone();
    let notification_worker = notifications.as_ref().map(|notifications| {
        let notifications = notifications.clone();
        tokio::spawn(async move { notifications.run().await })
    });
    let worker = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3));
        loop {
            tokio::select! {
                _ = publish_host.shutdown_requested() => break,
                _ = tick.tick() => {
                    let failed = publication.sync().await.is_err();
                    if publication_monitor.publication(failed).await.is_err() {
                        publish_host.request_shutdown();
                        break;
                    }
                }
            }
        }
    });
    let result = async {
        let mut delay = 1;
        loop {
            if host.is_shutting_down() {
                break;
            }
            let config = state::load(&directory)?;
            if !config.enabled {
                break;
            }
            monitor
                .connection(status::Connection::Connecting, None)
                .await?;
            match connect(
                host.clone(),
                &directory,
                &config,
                credentials.clone(),
                &client,
                publisher.clone(),
                &monitor,
                notifications.clone(),
            )
            .await
            {
                Ok(()) => delay = 1,
                Err(error) => {
                    monitor
                        .connection(
                            status::Connection::Retrying,
                            Some(status::Failure::connection(&error)),
                        )
                        .await?;
                    delay = (delay * 2).min(30);
                }
            }
            tokio::select! {
                _ = host.shutdown_requested() => break,
                _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
            }
        }
        Ok::<(), String>(())
    }
    .await;
    worker.abort();
    if let Some(worker) = notification_worker {
        worker.abort();
    }
    let stopped = monitor
        .connection(
            status::Connection::Stopped,
            result
                .as_ref()
                .err()
                .map(|error| status::Failure::connection(error)),
        )
        .await;
    result.and(stopped)
}

#[allow(clippy::too_many_arguments)]
async fn connect(
    host: Arc<HostState>,
    directory: &std::path::Path,
    config: &state::Config,
    credentials: Arc<dyn Credentials>,
    client: &reqwest::Client,
    publisher: Arc<shares::Shares>,
    monitor: &status::Monitor,
    notifications: Option<Arc<dyn notifications::NotificationService>>,
) -> Result<(), String> {
    let bearer = credentials
        .access_token()
        .await
        .map_err(|_| "installation_login_unavailable")?;
    let response = client
        .post(format!("{API}/tickets"))
        .bearer_auth(&*bearer)
        .json(&json!({"hostId":config.host_id,"role":"host"}))
        .send()
        .await
        .map_err(|_| "ticket_unavailable")?;
    if matches!(response.status().as_u16(), 401 | 403 | 404 | 410) {
        return Err("host_registration_revoked".into());
    }
    if !response.status().is_success() {
        return Err("ticket_unavailable".into());
    }
    let ticket: Value = bounded_json(response, 8192).await?;
    if ticket.get("relayUrl").and_then(Value::as_str) != Some(RELAY) {
        return Err("relay_origin_rejected".into());
    }
    let token = ticket
        .get("ticket")
        .and_then(Value::as_str)
        .filter(|v| v.len() <= 4096)
        .ok_or("invalid_ticket")?;
    let websocket_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(90 * 1024))
        .max_frame_size(Some(90 * 1024));
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async_with_config(RELAY, Some(websocket_config), false),
    )
    .await
    .map_err(|_| "relay_timeout")?
    .map_err(|_| "relay_unavailable")?;
    serve_socket(
        host,
        directory,
        config,
        publisher,
        monitor,
        socket,
        token,
        notifications,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn serve_socket(
    host: Arc<HostState>,
    directory: &std::path::Path,
    config: &state::Config,
    publisher: Arc<shares::Shares>,
    monitor: &status::Monitor,
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    token: &str,
    notifications: Option<Arc<dyn notifications::NotificationService>>,
) -> Result<(), String> {
    let (mut sink, mut stream) = socket.split();
    tokio::time::timeout(
        Duration::from_secs(5),
        sink.send(Message::Text(
            json!({"type":"authenticate","ticket":token})
                .to_string()
                .into(),
        )),
    )
    .await
    .map_err(|_| "relay_timeout")?
    .map_err(|_| "relay_closed")?;
    let ready = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .map_err(|_| "relay_timeout")?
        .ok_or("relay_closed")?
        .map_err(|_| "relay_closed")?;
    let Message::Text(ready) = ready else {
        return Err("relay_protocol".into());
    };
    if !matches!(
        serde_json::from_str::<RelayFrame>(&ready),
        Ok(RelayFrame::Ready { channel_id: None })
    ) {
        return Err("relay_protocol".into());
    }
    monitor
        .connection(status::Connection::Connected, None)
        .await?;
    let (outbound, mut output) = mpsc::channel::<Message>(64);
    let mut channels: HashMap<String, mpsc::Sender<Vec<u8>>> = HashMap::new();
    let mut workers = tokio::task::JoinSet::new();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    let mut last_seen = tokio::time::Instant::now();
    let result = loop {
        tokio::select! {
            _ = host.shutdown_requested() => break Ok(()),
            _ = heartbeat.tick() => {
                if !state::load(directory).is_ok_and(|current| current.enabled && current.host_id == config.host_id && current.public_key == config.public_key) { break Ok(()); }
                if last_seen.elapsed() > Duration::from_secs(30) { break Err("relay_timeout".into()); }
                if let Err(error) = monitor.connection(status::Connection::Connected, None).await { break Err(error); }
                if !matches!(tokio::time::timeout(Duration::from_secs(5),sink.send(Message::Ping(Vec::new().into()))).await, Ok(Ok(()))) { break Err("relay_closed".into()); }
            }
            Some(message) = output.recv() => {
                if !matches!(tokio::time::timeout(Duration::from_secs(5),sink.send(message)).await, Ok(Ok(()))) { break Err("relay_closed".into()); }
            }
            Some(done) = workers.join_next(), if !workers.is_empty() => {
                if let Ok(id) = done { channels.remove(&id); }
            }
            message = stream.next() => {
                let Some(Ok(message)) = message else { break Err("relay_closed".into()); };
                last_seen = tokio::time::Instant::now();
                let text = match message {
                    Message::Text(text) => text,
                    Message::Ping(data) => { if !matches!(tokio::time::timeout(Duration::from_secs(5),sink.send(Message::Pong(data))).await, Ok(Ok(()))) { break Err("relay_closed".into()); } continue; },
                    Message::Pong(_) => continue,
                    _ => break Err("relay_protocol".into()),
                };
                let Ok(frame) = serde_json::from_str::<RelayFrame>(&text) else { break Err("relay_protocol".into()); };
                match frame {
                    RelayFrame::Open { channel_id } => {
                        if channels.len() >= 64 || !opaque(&channel_id) || channels.contains_key(&channel_id) { break Err("relay_capacity".into()); }
                        let (tx, rx) = mpsc::channel(16);
                        channels.insert(channel_id.clone(), tx);
                        let state = host.clone();
                        let private = Zeroizing::new(config.private_key.clone());
                        let host_id = config.host_id.clone();
                        let output = outbound.clone();
                        let publication = publisher.clone();
                        let notifications = notifications.clone();
                        workers.spawn(async move {
                            let _ = drive_channel(state, &host_id, &channel_id, &private, rx, output.clone(), publication, notifications).await;
                            let _ = output.try_send(Message::Text(json!({"type":"closed","channelId":channel_id,"reason":"channel_closed"}).to_string().into()));
                            channel_id
                        });
                    }
                    RelayFrame::Data { channel_id, payload } => {
                        let frame = if payload.len() <= 87_380 { STANDARD.decode(payload).ok().filter(|v| v.len() <= 65_535) } else { None };
                        if let Some(tx) = channels.get(&channel_id)
                            && frame.is_none_or(|frame| tx.try_send(frame).is_err()) {
                            channels.remove(&channel_id);
                            let _ = outbound.try_send(Message::Text(json!({"type":"closed","channelId":channel_id,"reason":"backpressure"}).to_string().into()));
                        }
                    }
                    RelayFrame::Closed { channel_id, reason } => { let _ = reason; channels.remove(&channel_id); }
                    RelayFrame::Ready { .. } => break Err("relay_protocol".into()),
                }
            }
        }
    };
    channels.clear();
    while let Some(_done) = workers.join_next().await {}
    let _ = tokio::time::timeout(Duration::from_secs(5), sink.close()).await;
    result
}

fn opaque(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

async fn emit(
    output: &mpsc::Sender<Message>,
    channel_id: &str,
    frame: Vec<u8>,
) -> Result<(), String> {
    tokio::time::timeout(
        Duration::from_secs(5),
        output.send(Message::Text(
            json!({"type":"data","channelId":channel_id,"payload":STANDARD.encode(frame)})
                .to_string()
                .into(),
        )),
    )
    .await
    .map_err(|_| "remote_backpressure")?
    .map_err(|_| "remote_backpressure".into())
}

async fn encrypted(
    output: &mpsc::Sender<Message>,
    id: &str,
    channel: &mut Channel,
    message: Value,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(&message).map_err(|_| "invalid_response")?;
    for frame in channel.encrypt(&bytes).map_err(|_| "encryption_failed")? {
        emit(output, id, frame).await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn drive_channel(
    host: Arc<HostState>,
    host_id: &str,
    id: &str,
    private: &[u8],
    incoming: mpsc::Receiver<Vec<u8>>,
    output: mpsc::Sender<Message>,
    publisher: Arc<shares::Shares>,
    notifications: Option<Arc<dyn notifications::NotificationService>>,
) -> Result<(), String> {
    drive_channel_with_delivery(
        host,
        host_id,
        id,
        private,
        incoming,
        output,
        publisher,
        EventDelivery::configured()?,
        notifications,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn drive_channel_with_delivery(
    host: Arc<HostState>,
    host_id: &str,
    id: &str,
    private: &[u8],
    mut incoming: mpsc::Receiver<Vec<u8>>,
    output: mpsc::Sender<Message>,
    publisher: Arc<shares::Shares>,
    delivery: EventDelivery,
    notifications: Option<Arc<dyn notifications::NotificationService>>,
) -> Result<(), String> {
    let established = tokio::time::timeout(Duration::from_secs(15), async {
        let mut handshake = Handshake::new(private, None, host_id, id).map_err(|_| "handshake_failed")?;
        let initial = incoming.recv().await.ok_or("channel_closed")?;
        if !handshake.read(&initial).map_err(|_| "handshake_failed")?.is_empty() { return Err("handshake_failed"); }
        emit(&output, id, handshake.write(&[]).map_err(|_| "handshake_failed")?).await.map_err(|_| "channel_closed")?;
        let final_frame = incoming.recv().await.ok_or("channel_closed")?;
        let payload = handshake.read(&final_frame).map_err(|_| "handshake_failed")?;
        let remote_key = handshake.remote_static().ok_or("handshake_failed")?;
        let auth: Authentication = serde_json::from_slice(&payload).map_err(|_| "authentication_failed")?;
        let authorized = async { match auth {
            Authentication::Pair(value) => {
                let (access, pair) = RemoteAccess::pair_device(host.clone(), &value.nonce, remote_key).await.map_err(|_| "pairing_rejected")?;
                Ok((access, serde_json::to_value(pair).map_err(|_| "authentication_failed")?))
            }
            Authentication::Resume(value) => {
                let access = RemoteAccess::authenticate_device(host.clone(), value.session_token.clone(), value.csrf_token.clone(), remote_key).await.map_err(|_| "revoked")?;
                let reply = json!({"installationId":host.origin.install_id(),"sessionId":access.session_id(),"sessionToken":value.session_token,"csrfToken":value.csrf_token,"protocolVersion":2});
                Ok((access, reply))
            }
        } }.await;
        let mut channel = handshake.finish().map_err(|_| "handshake_failed")?;
        match authorized {
            Ok((access, pair)) => Ok((access, channel, pair)),
            Err(reason) => {
                encrypted(&output,id,&mut channel,json!({"type":"closed","reason":"revoked"})).await.map_err(|_| "channel_closed")?;
                Err(reason)
            }
        }
    }).await.map_err(|_| "handshake_timeout")?.map_err(str::to_owned)?;
    let (access, mut channel, pair) = established;
    let access = Arc::new(access);
    encrypted(
        &output,
        id,
        &mut channel,
        json!({"type":"paired","body":pair}),
    )
    .await?;
    let (reply_tx, mut replies) = mpsc::channel(16);
    let permits = Arc::new(Semaphore::new(16));
    let mut subscribed = false;
    let mut control_epoch = None;
    let mut cursor = 0;
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    let result = async {
        loop {
            if incoming.is_closed() || host.is_shutting_down() {
                break;
            }
            if subscribed && access.control_transfer_enabled() {
                let current = access.control_epoch().await.map_err(str::to_owned)?;
                if current != control_epoch {
                    let status = current.map_or_else(|| "watching".into(), |value| format!("controlling:{value}"));
                    encrypted(&output,id,&mut channel,json!({"type":"event","event":{"protocolVersion":2,"eventSequence":0,"event":{"kind":"hostStatus","state":status}}})).await?;
                    control_epoch = current;
                }
            }
            let notification = access.event_notification();
            tokio::pin!(notification);
            if delivery == EventDelivery::Notified {
                notification.as_mut().enable();
                if subscribed {
                    drain_events(&access, &output, id, &mut channel, &mut cursor, &host, &incoming).await?;
                }
            }
            tokio::select! {
                frame = incoming.recv() => {
                    let Some(frame) = frame else { break; };
                    let Some(plain) = channel.decrypt(&frame).map_err(|_| "invalid_ciphertext")? else { continue; };
                    if plain.len() > 256 * 1024 { return Err("request_too_large".into()); }
                    let request: Request = serde_json::from_slice(&plain).map_err(|_| "invalid_request")?;
                    if !opaque(&request.id) { return Err("invalid_request".into()); }
                    match request.method {
                        Method::Subscribe => {
                            cursor = request.body.get("afterSequence").and_then(Value::as_u64).ok_or("invalid_cursor")?;
                            if !subscribed {
                                control_epoch = match access.connect().await { Ok(epoch) => Some(epoch), Err("controller_held") => None, Err(_) => return Err("revoked".into()) };
                                let status = control_epoch.map_or_else(|| "watching".into(), |epoch| format!("controlling:{epoch}"));
                                encrypted(&output,id,&mut channel,json!({"type":"event","event":{"protocolVersion":2,"eventSequence":0,"event":{"kind":"hostStatus","state":status}}})).await?;
                                subscribed = true;
                            }
                            if cursor == 0 {
                                encrypted(&output,id,&mut channel,json!({"type":"event","event":{"protocolVersion":2,"eventSequence":0,"event":{"kind":"replayReset"}}})).await?;
                                cursor = access.snapshot().await.map_err(str::to_owned)?;
                            }
                            encrypted(&output,id,&mut channel,json!({"type":"response","id":request.id,"status":200,"body":{}})).await?;
                        }
                        Method::Heartbeat => {
                            let epoch = request.body.get("epoch").and_then(Value::as_u64).ok_or("invalid_epoch")?;
                            if let Err(error) = access.heartbeat(epoch).await
                                && (!access.control_transfer_enabled() || error != "controller_required") {
                                return Err(error.into());
                            }
                        }
                        Method::Command => {
                            if matches!(request.body.pointer("/operation/kind").and_then(Value::as_str), Some("forgetBrowserPairing" | "revokeBrowserPairing")) {
                                let reply = dispatch(access.clone(), publisher.clone(), notifications.clone(), request.body).await;
                                encrypted(&output,id,&mut channel,json!({"type":"response","id":request.id,"status":reply.0,"body":reply.1})).await?;
                                continue;
                            }
                            let permit = permits.clone().try_acquire_owned().map_err(|_| "too_many_commands")?;
                            let state = access.clone();
                            let replies = reply_tx.clone();
                            let publication = publisher.clone();
                            let notifications = notifications.clone();
                            tokio::spawn(async move {
                                let _permit = permit;
                                let reply = dispatch(state, publication, notifications, request.body).await;
                                let _ = replies.send(json!({"type":"response","id":request.id,"status":reply.0,"body":reply.1})).await;
                            });
                        }
                    }
                }
                Some(reply) = replies.recv() => { encrypted(&output,id,&mut channel,reply).await?; }
                _ = &mut notification, if delivery == EventDelivery::Notified => {}
                _ = tick.tick() => {
                    access.verify_access().await.map_err(str::to_owned)?;
                    if !subscribed { continue; }
                    if delivery == EventDelivery::Polling {
                        drain_events(&access, &output, id, &mut channel, &mut cursor, &host, &incoming).await?;
                    }
                }
            }
        }
        Ok(())
    }.await;
    if matches!(
        result.as_ref().err().map(String::as_str),
        Some("revoked" | "session_invalid")
    ) {
        let _ = encrypted(
            &output,
            id,
            &mut channel,
            json!({"type":"closed","reason":"revoked"}),
        )
        .await;
    }
    access.disconnect().await;
    result
}

async fn drain_events(
    access: &RemoteAccess,
    output: &mpsc::Sender<Message>,
    id: &str,
    channel: &mut Channel,
    cursor: &mut u64,
    host: &HostState,
    incoming: &mpsc::Receiver<Vec<u8>>,
) -> Result<(), String> {
    match access.events(*cursor).await.map_err(str::to_owned)? {
        ReplayOutcome::Replay(events) => {
            for envelope in events {
                if incoming.is_closed() || host.is_shutting_down() {
                    return Err("channel_closed".into());
                }
                access.verify_access().await.map_err(str::to_owned)?;
                tokio::time::timeout(
                    Duration::from_secs(5),
                    encrypted(
                        output,
                        id,
                        channel,
                        json!({"type":"event","event":envelope}),
                    ),
                )
                .await
                .map_err(|_| "remote_backpressure")??;
                *cursor = envelope.event_sequence;
            }
        }
        ReplayOutcome::SnapshotRequired => {
            encrypted(output, id, channel, json!({"type":"event","event":{"protocolVersion":2,"eventSequence":0,"event":{"kind":"replayReset"}}})).await?;
            *cursor = access.snapshot().await.map_err(str::to_owned)?;
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
#[allow(clippy::enum_variant_names)]
enum ShareOperation {
    #[serde(rename_all = "camelCase")]
    PreviewShare { session_id: String },
    #[serde(rename_all = "camelCase")]
    StartShare {
        session_id: String,
        share_id: String,
        generation: u64,
        upload_token: String,
    },
    #[serde(rename_all = "camelCase")]
    StopShare {
        session_id: String,
        share_id: String,
    },
}

async fn dispatch(
    access: Arc<RemoteAccess>,
    publisher: Arc<shares::Shares>,
    notifications: Option<Arc<dyn notifications::NotificationService>>,
    body: Value,
) -> (u16, Value) {
    if access.verify_access().await.is_err() {
        return (403, json!({"error":"revoked"}));
    }
    let kind = body
        .pointer("/operation/kind")
        .and_then(Value::as_str)
        .unwrap_or("");
    if publications::handles(kind) {
        return match &publisher.publications {
            Some(publications) => publications.dispatch(&access, body).await,
            None => (422, json!({"error":"publications_unavailable"})),
        };
    }
    if publisher.direct_mode && matches!(kind, "startShare" | "stopShare") {
        return (422, json!({"error":"legacy_publications_disabled"}));
    }
    if notifications::handles(kind) || direct_notifications::handles(kind) {
        return match notifications {
            Some(notifications) => notifications.dispatch(&access, body).await,
            None if kind == "getNotificationStatus" => (
                200,
                json!({"result":{"outcome":"notificationStatus","enabled":false,"registrations":[]}}),
            ),
            None => (422, json!({"error":"remote_experiment_disabled"})),
        };
    }
    let include_notifications = kind == "getRemoteCapabilities";
    if matches!(kind, "previewShare" | "startShare" | "stopShare") {
        if body.get("protocolVersion").and_then(Value::as_u64) != Some(2)
            || body
                .get("requestId")
                .and_then(Value::as_str)
                .is_none_or(|id| !opaque(id))
        {
            return (400, json!({"error":"invalid_request"}));
        }
        let Ok(operation) = serde_json::from_value::<ShareOperation>(body["operation"].clone())
        else {
            return (400, json!({"error":"invalid_share_request"}));
        };
        if !matches!(operation, ShareOperation::PreviewShare { .. }) {
            let Some(epoch) = body.get("controllerEpoch").and_then(Value::as_u64) else {
                return (409, json!({"error":"controller_required"}));
            };
            if access.authorize_control(epoch).await.is_err() {
                return (409, json!({"error":"controller_required"}));
            }
            if body
                .get("idempotencyKey")
                .and_then(Value::as_str)
                .is_none_or(|id| !opaque(id))
            {
                return (400, json!({"error":"idempotency_key_required"}));
            }
        }
        let result = match operation {
            ShareOperation::PreviewShare { session_id } => publisher
                .preview(&session_id)
                .await
                .map(|document| json!({"outcome":"sharePreview","document":document})),
            ShareOperation::StartShare {
                session_id,
                share_id,
                generation,
                upload_token,
            } => publisher
                .start(&session_id, &share_id, generation, &upload_token)
                .await
                .map(|()| json!({"outcome":"acknowledged"})),
            ShareOperation::StopShare {
                session_id,
                share_id,
            } => {
                if !opaque(&session_id) {
                    return (400, json!({"error":"invalid_session"}));
                }
                publisher
                    .stop(&share_id)
                    .await
                    .map(|()| json!({"outcome":"acknowledged"}))
            }
        };
        return match result {
            Ok(value) => (200, json!({"result":value})),
            Err(error) => (422, json!({"error":error})),
        };
    }
    let Ok(envelope) = serde_json::from_value(body) else {
        return (400, json!({"error":"unsupported_operation"}));
    };
    let mut reply = access.command(envelope).await;
    if include_notifications
        && reply.status < 300
        && let Some(result) = reply.body.get_mut("result").and_then(Value::as_object_mut)
    {
        result.insert("pushNotifications".into(), json!(notifications.is_some()));
        result.insert(
            "publicationsV2".into(),
            json!(publisher.publications.is_some()),
        );
    }
    (reply.status, reply.body)
}

pub async fn bounded_json(mut response: reqwest::Response, limit: usize) -> Result<Value, String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "response_unavailable")? {
        if bytes.len() + chunk.len() > limit {
            return Err("response_too_large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid_response".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) struct TestChannel {
        pub(super) state: Arc<HostState>,
        pub(super) input: mpsc::Sender<Vec<u8>>,
        output: mpsc::Receiver<Message>,
        cipher: Channel,
        pub(super) task: tokio::task::JoinHandle<Result<(), String>>,
        pub(super) paired: Value,
        _directory: tempfile::TempDir,
    }

    async fn frame(output: &mut mpsc::Receiver<Message>) -> Vec<u8> {
        let message = tokio::time::timeout(Duration::from_secs(2), output.recv())
            .await
            .unwrap()
            .unwrap();
        let Message::Text(text) = message else {
            panic!("expected relay text");
        };
        let envelope: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(envelope["type"], "data");
        STANDARD
            .decode(envelope["payload"].as_str().unwrap())
            .unwrap()
    }

    impl TestChannel {
        async fn new() -> Self {
            Self::with_delivery(EventDelivery::Polling).await
        }

        pub(super) async fn with_delivery(delivery: EventDelivery) -> Self {
            let state = Arc::new(
                HostState::new(
                    fabrials_agent_host::origin::LocalOrigin::new(
                        "0123456789abcdef0123456789abcdef",
                        21001,
                    )
                    .unwrap(),
                )
                .with_agent_mode(fabrials_agent_host::acp::AgentMode::Shared),
            );
            Self::with_state(state, delivery).await
        }

        pub(super) async fn with_state(state: Arc<HostState>, delivery: EventDelivery) -> Self {
            Self::with_notifications(state, delivery, None).await
        }

        pub(super) async fn with_notifications(
            state: Arc<HostState>,
            delivery: EventDelivery,
            notifications: Option<Arc<dyn notifications::NotificationService>>,
        ) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let nonce = state
                .pairing
                .lock()
                .await
                .mint_nonce(fabrials_agent_host::now_ms())
                .unwrap();
            let host = fabrials_remote_noise::identity().unwrap();
            let browser = fabrials_remote_noise::identity().unwrap();
            let mut handshake = Handshake::new(
                &browser.private,
                Some(host.public.as_slice().try_into().unwrap()),
                "host-1",
                "channel-1",
            )
            .unwrap();
            let publisher = Arc::new(
                shares::Shares::new(
                    state.clone(),
                    directory.path().into(),
                    http_client().unwrap(),
                )
                .unwrap(),
            );
            let (input, incoming) = mpsc::channel(16);
            let (outgoing, mut output) = mpsc::channel(16);
            let serving = state.clone();
            let task = tokio::spawn(async move {
                drive_channel_with_delivery(
                    serving,
                    "host-1",
                    "channel-1",
                    &host.private,
                    incoming,
                    outgoing,
                    publisher,
                    delivery,
                    notifications,
                )
                .await
            });
            input.send(handshake.write(&[]).unwrap()).await.unwrap();
            assert!(
                handshake
                    .read(&frame(&mut output).await)
                    .unwrap()
                    .is_empty()
            );
            input
                .send(
                    handshake
                        .write(
                            serde_json::to_string(&json!({"nonce":nonce.expose()}))
                                .unwrap()
                                .as_bytes(),
                        )
                        .unwrap(),
                )
                .await
                .unwrap();
            let mut value = Self {
                state,
                input,
                output,
                cipher: handshake.finish().unwrap(),
                task,
                paired: Value::Null,
                _directory: directory,
            };
            value.paired = value.receive().await;
            assert_eq!(value.paired["type"], "paired");
            assert_eq!(
                value.paired["body"]["installationId"],
                value.state.origin.install_id()
            );
            value
        }

        pub(super) async fn send(&mut self, body: Value) {
            for frame in self
                .cipher
                .encrypt(&serde_json::to_vec(&body).unwrap())
                .unwrap()
            {
                self.input.send(frame).await.unwrap();
            }
        }

        pub(super) async fn receive(&mut self) -> Value {
            loop {
                let frame = frame(&mut self.output).await;
                if let Some(plain) = self.cipher.decrypt(&frame).unwrap() {
                    return serde_json::from_slice(&plain).unwrap();
                }
            }
        }
    }

    #[tokio::test]
    async fn control_transfer_sends_each_noise_channel_only_its_own_authority() {
        let state = Arc::new(
            HostState::new(
                fabrials_agent_host::origin::LocalOrigin::new(
                    "0123456789abcdef0123456789abcdef",
                    21001,
                )
                .unwrap(),
            )
            .with_agent_mode(fabrials_agent_host::acp::AgentMode::Shared)
            .with_remote_experiments(
                fabrials_agent_host::remote_management::RemoteExperiments {
                    control_transfer: true,
                    device_panel: true,
                    projects: false,
                },
            ),
        );
        let mut owner = TestChannel::with_state(state.clone(), EventDelivery::Notified).await;
        let mut observer = TestChannel::with_state(state.clone(), EventDelivery::Notified).await;
        for (channel, expected) in [(&mut owner, "controlling:1"), (&mut observer, "watching")] {
            channel
                .send(json!({"id":"subscribe", "method":"subscribe", "body":{"afterSequence":0}}))
                .await;
            assert_eq!(channel.receive().await["event"]["event"]["state"], expected);
            while channel.receive().await["id"] != "subscribe" {}
        }
        observer.send(json!({"id":"request", "method":"command", "body":{"protocolVersion":2,"requestId":"request","idempotencyKey":"request","operation":{"kind":"requestControlTransfer"}}})).await;
        let request_id = loop {
            let reply = observer.receive().await;
            if reply["id"] == "request" {
                assert_eq!(reply["status"], 202, "{reply}");
                break reply["body"]["result"]["request"]["requestId"]
                    .as_str()
                    .unwrap()
                    .to_owned();
            }
        };
        owner.send(json!({"id":"approve", "method":"command", "body":{"protocolVersion":2,"requestId":"approve","idempotencyKey":"approve","controllerEpoch":1,"operation":{"kind":"approveControlTransfer","requestId":request_id}}})).await;
        let mut acknowledged = false;
        let mut watching = false;
        while !acknowledged || !watching {
            let reply = owner.receive().await;
            if reply["id"] == "approve" {
                assert_eq!(reply["status"], 202, "{reply}");
                acknowledged = true;
            }
            if reply["event"]["event"]["kind"] == "hostStatus" {
                assert_eq!(reply["event"]["event"]["state"], "watching");
                assert!(reply["event"]["event"].get("controllerToken").is_none());
                watching = true;
            }
            assert!(!reply.to_string().contains("controlling:2"));
        }
        loop {
            let reply = observer.receive().await;
            if reply["event"]["event"]["kind"] == "hostStatus" {
                assert_eq!(reply["event"]["event"]["state"], "controlling:2");
                break;
            }
        }
        owner
            .send(json!({"id":"old-heartbeat", "method":"heartbeat", "body":{"epoch":1}}))
            .await;
        owner.send(json!({"id":"still-watching", "method":"command", "body":{"protocolVersion":2,"requestId":"still-watching","operation":{"kind":"bootstrap"}}})).await;
        loop {
            let reply = owner.receive().await;
            if reply["id"] == "still-watching" {
                assert_eq!(reply["status"], 202);
                break;
            }
        }
        if let ReplayOutcome::Replay(events) = state.journal.lock().await.replay_after(0) {
            assert!(events.iter().all(|envelope| !matches!(&envelope.event, fabrials_agent_host::protocol::Event::HostStatus { state, .. } if state.starts_with("controlling:"))));
        }
        drop(owner.input);
        drop(observer.input);
        assert!(owner.task.await.unwrap().is_ok());
        assert!(observer.task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn real_noise_pairing_carries_commands_subscriptions_and_events() {
        let mut channel = TestChannel::new().await;
        channel
            .send(json!({"id":"subscribe", "method":"subscribe", "body":{"afterSequence":0}}))
            .await;
        let hello = channel.receive().await;
        assert_eq!(hello["event"]["event"]["state"], "controlling:1");
        loop {
            if channel.receive().await["id"] == "subscribe" {
                break;
            }
        }
        channel.send(json!({"id":"bootstrap", "method":"command", "body":{"protocolVersion":2,"requestId":"boot","operation":{"kind":"bootstrap"}}})).await;
        loop {
            let response = channel.receive().await;
            if response["id"] == "bootstrap" {
                assert_eq!(response["status"], 202);
                assert_eq!(response["body"]["result"]["outcome"], "workspaces");
                break;
            }
        }
        channel
            .state
            .emit_event(
                fabrials_agent_host::protocol::Event::WorkspacesChanged,
                None,
            )
            .await;
        loop {
            let response = channel.receive().await;
            if response["event"]["event"]["kind"] == "workspacesChanged" {
                break;
            }
        }
        drop(channel.input);
        assert!(
            tokio::time::timeout(Duration::from_secs(2), channel.task)
                .await
                .unwrap()
                .unwrap()
                .is_ok()
        );
    }

    #[tokio::test]
    async fn altered_ciphertext_is_refused_before_dispatch() {
        let mut channel = TestChannel::new().await;
        let mut encrypted = channel
            .cipher
            .encrypt(br#"{"id":"forged","method":"command","body":{}}"#)
            .unwrap()
            .remove(0);
        encrypted[20] ^= 1;
        channel.input.send(encrypted).await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), channel.task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result, Err("invalid_ciphertext".into()));
        assert!(channel.state.session.lock().await.sessions.is_empty());
    }

    #[tokio::test]
    async fn revocation_closes_even_an_idle_unsubscribed_channel() {
        let mut channel = TestChannel::new().await;
        let session_id = channel.paired["body"]["sessionId"].as_str().unwrap();
        assert!(channel.state.pairing.lock().await.revoke(session_id));
        assert_eq!(
            channel.receive().await,
            json!({"type":"closed","reason":"revoked"})
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(2), channel.task)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
    }

    #[tokio::test]
    async fn forgetting_pairing_acknowledges_before_authenticated_close() {
        let mut channel = TestChannel::new().await;
        let nonce = channel
            .state
            .pairing
            .lock()
            .await
            .mint_nonce(fabrials_agent_host::now_ms())
            .unwrap();
        let (other, _) = RemoteAccess::pair(channel.state.clone(), nonce.expose())
            .await
            .unwrap();
        let epoch = other.connect().await.unwrap();
        channel.send(json!({"id":"forget", "method":"command", "body":{"protocolVersion":2,"requestId":"forget","operation":{"kind":"forgetBrowserPairing"}}})).await;
        let response = channel.receive().await;
        assert_eq!(response["type"], "response");
        assert_eq!(response["id"], "forget");
        assert_eq!(response["status"], 202);
        assert_eq!(response["body"]["result"]["outcome"], "acknowledged");
        assert_eq!(
            channel.receive().await,
            json!({"type":"closed","reason":"revoked"})
        );
        assert!(other.heartbeat(epoch).await.is_ok());
        assert!(
            tokio::time::timeout(Duration::from_secs(2), channel.task)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
    }

    #[tokio::test]
    async fn noise_resume_binds_the_authenticated_browser_static_key() {
        for (remote_grant, same_key) in [(true, true), (true, false), (false, true)] {
            let directory = tempfile::tempdir().unwrap();
            let state = Arc::new(HostState::new(
                fabrials_agent_host::origin::LocalOrigin::new(
                    "0123456789abcdef0123456789abcdef",
                    21001,
                )
                .unwrap(),
            ));
            let host = fabrials_remote_noise::identity().unwrap();
            let browser = fabrials_remote_noise::identity().unwrap();
            let original = if same_key {
                browser.public.as_slice().try_into().unwrap()
            } else {
                [99; 32]
            };
            let nonce = state
                .pairing
                .lock()
                .await
                .mint_nonce(fabrials_agent_host::now_ms())
                .unwrap();
            let (_, paired) = if remote_grant {
                RemoteAccess::pair_device(state.clone(), nonce.expose(), original)
                    .await
                    .unwrap()
            } else {
                RemoteAccess::pair(state.clone(), nonce.expose())
                    .await
                    .unwrap()
            };
            let publisher = Arc::new(
                shares::Shares::new(
                    state.clone(),
                    directory.path().into(),
                    http_client().unwrap(),
                )
                .unwrap(),
            );
            let mut handshake = Handshake::new(
                &browser.private,
                Some(host.public.as_slice().try_into().unwrap()),
                "host-1",
                "channel-1",
            )
            .unwrap();
            let (input, incoming) = mpsc::channel(16);
            let (outgoing, mut output) = mpsc::channel(16);
            let task = tokio::spawn(async move {
                drive_channel(
                    state,
                    "host-1",
                    "channel-1",
                    &host.private,
                    incoming,
                    outgoing,
                    publisher,
                    None,
                )
                .await
            });
            input.send(handshake.write(&[]).unwrap()).await.unwrap();
            handshake.read(&frame(&mut output).await).unwrap();
            input.send(handshake.write(&serde_json::to_vec(&json!({"sessionToken":paired.session_token,"csrfToken":paired.csrf_token})).unwrap()).unwrap()).await.unwrap();
            if remote_grant && same_key {
                let mut channel = handshake.finish().unwrap();
                let response = channel.decrypt(&frame(&mut output).await).unwrap().unwrap();
                let reply: Value = serde_json::from_slice(&response).unwrap();
                assert_eq!(reply["type"], "paired");
                assert_eq!(
                    reply["body"]["installationId"],
                    "0123456789abcdef0123456789abcdef"
                );
                drop(input);
                assert!(
                    tokio::time::timeout(Duration::from_secs(2), task)
                        .await
                        .unwrap()
                        .unwrap()
                        .is_ok()
                );
            } else {
                let mut channel = handshake.finish().unwrap();
                let response = channel.decrypt(&frame(&mut output).await).unwrap().unwrap();
                let reply: Value = serde_json::from_slice(&response).unwrap();
                assert_eq!(reply, json!({"type":"closed","reason":"revoked"}));
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(2), task)
                        .await
                        .unwrap()
                        .unwrap(),
                    Err("revoked".into())
                );
            }
        }
    }
}
