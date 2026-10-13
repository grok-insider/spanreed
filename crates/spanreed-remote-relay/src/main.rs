mod control;
mod routing;

use axum::{
    extract::{
        ConnectInfo, State, WebSocketUpgrade,
        ws::{CloseFrame, Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
};
use control::ControlPlane;
use futures_util::{Sink, SinkExt, StreamExt};
use routing::{Identity, Incoming, MAX_FRAME, Peer, QUEUE_SIZE};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Semaphore, mpsc, watch};
use uuid::Uuid;

#[derive(Clone)]
struct App {
    control: ControlPlane,
    routes: Arc<Mutex<routing::Router>>,
    slots: Arc<Semaphore>,
    admission: Arc<Mutex<Admission>>,
    last_ready: Arc<Mutex<Option<Instant>>>,
    shutdown: watch::Sender<bool>,
}

impl App {
    async fn stop(&self) {
        self.shutdown.send_replace(true);
        self.routes.lock().await.shutdown();
    }
}

fn ready_at(last_ready: Option<Instant>, stopping: bool, now: Instant) -> bool {
    !stopping
        && last_ready.is_some_and(|checked| now.duration_since(checked) < Duration::from_secs(10))
}

async fn readiness(State(app): State<App>) -> Response {
    let ready = ready_at(
        *app.last_ready.lock().await,
        *app.shutdown.borrow(),
        Instant::now(),
    );
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        [("cache-control", "no-store")],
        if ready { "ready" } else { "unavailable" },
    )
        .into_response()
}

async fn probe_readiness(app: App) {
    let mut shutdown = app.shutdown.subscribe();
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        if *shutdown.borrow() {
            break;
        }
        tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            _ = interval.tick() => {},
        }
        let ready = tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            ready = app.control.ready() => ready,
        };
        *app.last_ready.lock().await = ready.then(Instant::now);
    }
}

struct Admission {
    ips: HashMap<IpAddr, (Instant, u32)>,
    per_minute: u32,
}
impl Admission {
    fn allow(&mut self, ip: IpAddr, now: Instant) -> bool {
        self.ips
            .retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(60));
        if !self.ips.contains_key(&ip) && self.ips.len() >= 2048 {
            return false;
        }
        let value = self.ips.entry(ip).or_insert((now, 0));
        if value.1 >= self.per_minute {
            return false;
        }
        value.1 += 1;
        true
    }
}

async fn upgrade(
    State(app): State<App>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    uri: Uri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if *app.shutdown.borrow() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if uri.query().is_some()
        || headers
            .get("origin")
            .is_some_and(|origin| origin != "https://remote.fabrials.com")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app
        .admission
        .lock()
        .await
        .allow(address.ip(), Instant::now())
    {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let Ok(permit) = app.slots.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    ws.max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            serve(socket, app).await;
        })
}

async fn send<S: Sink<Message> + Unpin>(socket: &mut S, message: Message) -> bool {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message))
        .await
        .is_ok_and(|result| result.is_ok())
}

async fn close<S: Sink<Message> + Unpin>(socket: &mut S, reason: &'static str) {
    send(
        socket,
        Message::Close(Some(CloseFrame {
            code: 1008,
            reason: reason.into(),
        })),
    )
    .await;
}

async fn authenticate(socket: &mut WebSocket, control: &ControlPlane) -> Result<Identity, ()> {
    let Some(Ok(Message::Text(text))) = socket.recv().await else {
        return Err(());
    };
    let Incoming::Authenticate { ticket } =
        serde_json::from_str::<Incoming>(&text).map_err(|_| ())?
    else {
        return Err(());
    };
    control.consume(&ticket).await
}

async fn active_until_shutdown(
    control: &ControlPlane,
    identity: &Identity,
    shutdown: &mut watch::Receiver<bool>,
) -> bool {
    if *shutdown.borrow() {
        return false;
    }
    tokio::select! {
        biased;
        _ = shutdown.changed() => false,
        active = control.active(identity) => active,
    }
}

async fn write_socket<S: Sink<Message> + Unpin>(
    mut sink: S,
    peer: Peer,
    mut rx: mpsc::Receiver<routing::Outgoing>,
    mut control_rx: mpsc::Receiver<Message>,
    close_on_channel_end: bool,
) {
    let mut stopped = peer.stop.subscribe();
    let mut ping = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(15),
        Duration::from_secs(15),
    );
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        if *stopped.borrow() {
            break;
        }
        let mut close_after_frame = false;
        let frame = tokio::select! {
            biased;
            _ = stopped.changed() => break,
            _ = peer.drain_expired() => break,
            frame = control_rx.recv() => { let Some(frame) = frame else { break; }; frame },
            _ = ping.tick() => Message::Ping(Vec::new().into()),
            frame = rx.recv() => {
                let Some(frame) = frame else { break; };
                close_after_frame = close_on_channel_end && matches!(frame, routing::Outgoing::Closed { .. });
                let Ok(text) = serde_json::to_string(&frame) else { break; };
                Message::Text(text.into())
            }
        };
        let sent = tokio::select! {
            biased;
            _ = stopped.changed() => false,
            _ = peer.drain_expired() => false,
            sent = send(&mut sink, frame) => sent,
        };
        if !sent || close_after_frame {
            break;
        }
    }
    let interrupted = *stopped.borrow();
    peer.close();
    if interrupted {
        return;
    }
    tokio::select! {
        biased;
        _ = peer.drain_expired() => {},
        _ = close(&mut sink, "connection_closed") => {},
    }
}

async fn serve(mut socket: WebSocket, app: App) {
    let mut shutdown = app.shutdown.subscribe();
    if *shutdown.borrow() {
        close(&mut socket, "shutting_down").await;
        return;
    }
    let authenticated = tokio::select! {
        biased;
        _ = shutdown.changed() => { close(&mut socket, "shutting_down").await; return; },
        result = tokio::time::timeout(Duration::from_secs(5), authenticate(&mut socket, &app.control)) => result,
    };
    let Ok(Ok(identity)) = authenticated else {
        close(&mut socket, "authentication_failed").await;
        return;
    };
    if !active_until_shutdown(&app.control, &identity, &mut shutdown).await {
        close(&mut socket, "authorization_unavailable").await;
        return;
    }
    let (tx, rx) = mpsc::channel::<routing::Outgoing>(QUEUE_SIZE);
    let (stop, mut stopped) = watch::channel(false);
    let peer = Peer {
        id: Uuid::new_v4(),
        tx,
        stop,
        draining: watch::channel(None).0,
    };
    let admitted = app.routes.lock().await.admit(&identity, peer.clone());
    match admitted {
        Err(reason) => {
            close(&mut socket, reason).await;
            return;
        }
        Ok(Some(notification)) => {
            if notification.send().await.is_err() {
                let _ = app.routes.lock().await.disconnect(&identity, peer.id);
                peer.close();
                close(&mut socket, "host_unavailable").await;
                return;
            }
        }
        _ => {}
    }
    let (sink, mut stream) = socket.split();
    let (control_tx, control_rx) = mpsc::channel::<Message>(2);
    let close_on_channel_end = identity.role == routing::Role::Browser;
    let mut writer = tokio::spawn(write_socket(
        sink,
        peer.clone(),
        rx,
        control_rx,
        close_on_channel_end,
    ));
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(15),
        Duration::from_secs(15),
    );
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut introspection = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(10),
        Duration::from_secs(10),
    );
    introspection.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_received = Instant::now();
    let _reason = loop {
        if *stopped.borrow() {
            break "connection_replaced";
        }
        tokio::select! {
            biased;
            _ = stopped.changed() => { break "connection_closed"; }
            _ = introspection.tick() => {
                if !app.control.active(&identity).await { break "authorization_revoked"; }
            }
            _ = heartbeat.tick() => {
                if last_received.elapsed() > Duration::from_secs(45) { break "heartbeat_timeout"; }
            }
            next = stream.next() => {
                last_received = Instant::now();
                match next {
                    Some(Ok(Message::Text(text))) => {
                        let result = match serde_json::from_str::<Incoming>(&text) {
                            Ok(Incoming::Data { channel_id, payload }) => {
                                let delivery = app.routes.lock().await.delivery(&identity, peer.id, &channel_id, payload);
                                match delivery {
                                    Ok(Some(delivery)) => {
                                        match delivery.send().await {
                                            Ok(()) => Ok(()),
                                            Err(_) if identity.role == routing::Role::Host => {
                                                let _ = app.routes.lock().await.close_channel(&identity, peer.id, &channel_id);
                                                Ok(())
                                            }
                                            Err(error) => Err(error),
                                        }
                                    }
                                    Ok(None) => Ok(()),
                                    Err(error) => Err(error),
                                }
                            },
                            Ok(Incoming::Closed { channel_id, .. }) => {
                                let closing = app.routes.lock().await.close_channel(&identity, peer.id, &channel_id);
                                match closing {
                                    Ok(Some(delivery)) => { let _ = delivery.send().await; Ok(()) },
                                    Ok(None) => Ok(()),
                                    Err(error) => Err(error),
                                }
                            },
                            _ => Err("invalid_frame"),
                        };
                        if let Err(reason) = result
                            && !peer.is_draining()
                        { break reason; }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if control_tx.try_send(Message::Pong(payload)).is_err() { break "control_backpressure"; }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    _ => break "disconnected",
                }
            }
        }
    };
    let notification = app.routes.lock().await.disconnect(&identity, peer.id);
    peer.close();
    if let Some(notification) = notification {
        let _ = notification.send().await;
    }
    if tokio::time::timeout(Duration::from_secs(11), &mut writer)
        .await
        .is_err()
    {
        writer.abort();
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let secret = std::env::var("SPANREED_REMOTE_RELAY_SECRET")
        .map_err(|_| "SPANREED_REMOTE_RELAY_SECRET is required")?;
    let base = std::env::var("SPANREED_REMOTE_CONTROL_URL")
        .unwrap_or_else(|_| "https://fabrials.com".into());
    let control = ControlPlane::new(&base, secret)?;
    let address: SocketAddr = std::env::var("SPANREED_REMOTE_RELAY_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8791".into())
        .parse()?;
    let rate = std::env::var("SPANREED_REMOTE_CONNECTS_PER_MINUTE")
        .ok()
        .map(|v| v.parse::<u32>())
        .transpose()?
        .unwrap_or(30);
    if !(1..=10_000).contains(&rate) {
        return Err("connection rate must be between 1 and 10000".into());
    }
    let app = App {
        control,
        routes: Arc::new(Mutex::new(routing::Router::default())),
        slots: Arc::new(Semaphore::new(1024)),
        admission: Arc::new(Mutex::new(Admission {
            ips: HashMap::new(),
            per_minute: rate,
        })),
        last_ready: Arc::new(Mutex::new(None)),
        shutdown: watch::channel(false).0,
    };
    let stopping = app.clone();
    let probe = tokio::spawn(probe_readiness(app.clone()));
    let router = axum::Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ready", get(readiness))
        .route("/connect", get(upgrade))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(address).await?;
    eprintln!("spanreed remote relay listening on {address}");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        stopping.stop().await;
    })
    .await?;
    probe.abort();
    let _ = probe.await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::Outgoing;

    #[tokio::test]
    async fn shutdown_cancels_pending_initial_introspection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let control =
            ControlPlane::loopback_fixture(&format!("http://{address}"), "x".repeat(32)).unwrap();
        let identity = Identity {
            account_id: "account".into(),
            host_id: "host".into(),
            role: routing::Role::Host,
            generation: 1,
        };
        let (shutdown, mut stopped) = watch::channel(false);
        let request =
            tokio::spawn(
                async move { active_until_shutdown(&control, &identity, &mut stopped).await },
            );
        let (socket, _) = tokio::time::timeout(Duration::from_secs(1), listener.accept())
            .await
            .unwrap()
            .unwrap();
        shutdown.send_replace(true);
        assert!(
            !tokio::time::timeout(Duration::from_millis(500), request)
                .await
                .unwrap()
                .unwrap()
        );
        drop(socket);
    }

    #[test]
    fn readiness_expires_and_shutdown_overrides_a_fresh_probe() {
        let now = Instant::now();
        assert!(!ready_at(None, false, now));
        assert!(ready_at(Some(now), false, now));
        assert!(ready_at(
            Some(now),
            false,
            now + Duration::from_millis(9999)
        ));
        assert!(!ready_at(Some(now), false, now + Duration::from_secs(10)));
        assert!(!ready_at(Some(now), true, now));
    }

    #[tokio::test]
    async fn drain_deadline_bounds_continuous_control_traffic() {
        let (tx, rx) = mpsc::channel(QUEUE_SIZE);
        let (stop, stopped) = watch::channel(false);
        let peer = Peer {
            id: Uuid::new_v4(),
            tx,
            stop,
            draining: watch::channel(Some(
                Instant::now() - routing::DRAIN_TIMEOUT + Duration::from_millis(50),
            ))
            .0,
        };
        let (control_tx, control_rx) = mpsc::channel(2);
        let traffic = tokio::spawn(async move {
            while control_tx
                .send(Message::Pong(Vec::new().into()))
                .await
                .is_ok()
            {}
        });
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel();
        let sink = Box::pin(futures_util::sink::unfold((), move |(), frame| {
            seen_tx.send(frame).unwrap();
            async { Ok::<_, std::convert::Infallible>(()) }
        }));
        tokio::time::timeout(
            Duration::from_millis(500),
            write_socket(sink, peer, rx, control_rx, true),
        )
        .await
        .unwrap();
        traffic.await.unwrap();
        assert!(seen_rx.try_recv().is_ok());
        assert!(*stopped.borrow());
    }

    #[tokio::test]
    async fn drain_deadline_and_revocation_interrupt_in_flight_writes() {
        for revoked in [false, true] {
            let (tx, rx) = mpsc::channel(QUEUE_SIZE);
            let (stop, stopped) = watch::channel(false);
            let peer = Peer {
                id: Uuid::new_v4(),
                tx,
                stop,
                draining: watch::channel(None).0,
            };
            peer.send(Outgoing::Ready { channel_id: None });
            let (_control_tx, control_rx) = mpsc::channel(2);
            let (seen_tx, mut seen_rx) = mpsc::unbounded_channel();
            let sink = Box::pin(futures_util::sink::unfold((), move |(), frame| {
                seen_tx.send(frame).unwrap();
                async {
                    std::future::pending::<()>().await;
                    Ok::<_, std::convert::Infallible>(())
                }
            }));
            let writer = tokio::spawn(write_socket(sink, peer.clone(), rx, control_rx, true));
            tokio::time::timeout(Duration::from_millis(500), seen_rx.recv())
                .await
                .unwrap()
                .unwrap();
            peer.draining.send_replace(Some(if revoked {
                Instant::now()
            } else {
                Instant::now() - routing::DRAIN_TIMEOUT
            }));
            if revoked {
                peer.close();
            }
            tokio::time::timeout(Duration::from_millis(500), writer)
                .await
                .unwrap()
                .unwrap();
            assert!(*stopped.borrow());
        }
    }

    #[tokio::test]
    async fn managed_relay_qualification_fixture() {
        let Ok(directory) = std::env::var("FABRIALS_REMOTE_RELAY_QA") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        assert!(directory.is_absolute() && directory.is_dir());
        assert!(!directory.join("stop").exists());
        let base = std::env::var("FABRIALS_REMOTE_RELAY_QA_CONTROL_URL").unwrap();
        let secret = std::env::var("FABRIALS_REMOTE_RELAY_QA_SECRET").unwrap();
        let app = App {
            control: ControlPlane::loopback_fixture(&base, secret).unwrap(),
            routes: Arc::new(Mutex::new(routing::Router::default())),
            slots: Arc::new(Semaphore::new(1024)),
            admission: Arc::new(Mutex::new(Admission {
                ips: HashMap::new(),
                per_minute: 1000,
            })),
            last_ready: Arc::new(Mutex::new(None)),
            shutdown: watch::channel(false).0,
        };
        let stopping = app.clone();
        let probe = tokio::spawn(probe_readiness(app.clone()));
        let router = axum::Router::new()
            .route("/health", get(|| async { "ok" }))
            .route("/ready", get(readiness))
            .route(
                "/qa/stats",
                get(|State(app): State<App>| async move {
                    let mut stats = app.routes.lock().await.qualification_stats();
                    stats["allocatedSockets"] =
                        serde_json::json!(1024 - app.slots.available_permits());
                    axum::Json(stats)
                }),
            )
            .route("/connect", get(upgrade))
            .with_state(app);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let ready = serde_json::json!({"url": format!("ws://{address}/connect")});
        std::fs::write(
            directory.join("relay-ready.tmp"),
            serde_json::to_vec(&ready).unwrap(),
        )
        .unwrap();
        std::fs::rename(
            directory.join("relay-ready.tmp"),
            directory.join("relay-ready.json"),
        )
        .unwrap();
        let stop = directory.join("stop");
        let graceful_stop = stop.clone();
        let duration = std::env::var("FABRIALS_REMOTE_RELAY_QA_DURATION_SEC")
            .ok()
            .map(|value| value.parse::<u64>().unwrap())
            .unwrap_or(120);
        assert!((1..=7200).contains(&duration));
        let serve = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let deadline = Instant::now() + Duration::from_secs(duration);
            while !graceful_stop.exists() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            stopping.stop().await;
        });
        tokio::time::timeout(Duration::from_secs(duration + 15), serve.into_future())
            .await
            .unwrap()
            .unwrap();
        probe.abort();
        let _ = probe.await;
        assert!(
            stop.exists(),
            "managed relay qualification did not finish within its deadline"
        );
    }

    #[test]
    fn admission_is_bounded_and_recovers_after_window() {
        let now = Instant::now();
        let ip = "127.0.0.1".parse().unwrap();
        let mut limiter = Admission {
            ips: HashMap::new(),
            per_minute: 2,
        };
        assert!(limiter.allow(ip, now));
        assert!(limiter.allow(ip, now));
        assert!(!limiter.allow(ip, now));
        assert!(limiter.allow(ip, now + Duration::from_secs(60)));
    }
    #[test]
    fn wire_frames_reject_unknown_fields_and_wrong_types() {
        assert!(
            serde_json::from_str::<Incoming>(r#"{"type":"authenticate","ticket":"x","extra":"x"}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<Incoming>(r#"{"type":"data","channelId":"x","payload":[]}"#)
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(Outgoing::Ready {
                channel_id: Some("channel".into())
            })
            .unwrap(),
            serde_json::json!({"type":"ready","channelId":"channel"})
        );
    }
}
