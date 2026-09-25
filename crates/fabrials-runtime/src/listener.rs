//! Bounded HTTP listener. Hosts supply routing, accounting and control-plane policy.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub trait ConnectionHost: Send + Sync + 'static {
    fn bind(&self) -> &str;
    fn log(&self, message: &str);
    fn ready(&self, _address: std::net::SocketAddr) {}
    fn classify(&self, path: &str) -> AdmissionClass;
    fn body_reservation(&self, head: &PreflightHead, class: AdmissionClass) -> usize;
    fn handle(&self, stream: std::net::TcpStream, cursor: &Mutex<usize>) -> Result<(), String>;
    fn maintenance_enabled(&self) -> bool {
        false
    }
    fn maintenance(&self) {}
}

const CONTROL_PLANE_LIMIT: usize = 32;
const HEALTH_PLANE_LIMIT: usize = 8;
const REQUEST_HEAD_PREFLIGHT_LIMIT: usize = 248;
const HEALTH_OVERFLOW_PREFLIGHT_LIMIT: usize = 8;
const HEALTH_OVERFLOW_MAX_HEAD_BYTES: usize = 8 * 1024;
const REQUEST_HEAD_INITIAL_BYTES: usize = 1024;
const REQUEST_HEAD_PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_OVERFLOW_PREFLIGHT_TIMEOUT: Duration = Duration::from_millis(500);
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);
const USAGE_REPLAY_INTERVAL: Duration = Duration::from_secs(30);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
pub const MIB: usize = 1024 * 1024;
pub const DEFAULT_BODY_MEMORY_LIMIT_MIB: usize = 128;
pub const MIN_BODY_MEMORY_LIMIT_MIB: usize = 64;
pub const MAX_BODY_MEMORY_LIMIT_MIB: usize = 1024;

static PROCESS_SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
fn install_process_signal_handlers() -> Result<(), String> {
    use std::sync::OnceLock;

    static INSTALL: OnceLock<Result<(), &'static str>> = OnceLock::new();
    extern "C" fn request_shutdown(_: std::os::raw::c_int) {
        PROCESS_SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
    }
    unsafe extern "C" {
        fn signal(signal: std::os::raw::c_int, handler: usize) -> usize;
    }
    let installed = INSTALL.get_or_init(|| unsafe {
        const SIGINT: std::os::raw::c_int = 2;
        const SIGTERM: std::os::raw::c_int = 15;
        const SIG_ERR: usize = usize::MAX;
        let handler = request_shutdown as *const () as usize;
        if signal(SIGINT, handler) == SIG_ERR {
            return Err("install SIGINT handler");
        }
        if signal(SIGTERM, handler) == SIG_ERR {
            return Err("install SIGTERM handler");
        }
        Ok(())
    });
    installed.map_err(|error| (*error).to_string())
}

#[cfg(not(unix))]
fn install_process_signal_handlers() -> Result<(), String> {
    Ok(())
}

pub fn serve(
    cfg: Arc<dyn ConnectionHost>,
    stop: Arc<AtomicBool>,
    max_hops: usize,
    memory_limit: usize,
) -> Result<(), String> {
    serve_hosts(vec![cfg], stop, max_hops, memory_limit)
}

/// Run related listeners on one runtime, shutting down the group on failure.
pub fn serve_hosts(
    hosts: Vec<Arc<dyn ConnectionHost>>,
    stop: Arc<AtomicBool>,
    max_hops: usize,
    memory_limit: usize,
) -> Result<(), String> {
    serve_hosts_inner(hosts, stop, max_hops, memory_limit, true)
}

/// Embedded hosts own their process lifecycle; do not replace their signal handlers.
pub fn serve_embedded(
    host: Arc<dyn ConnectionHost>,
    stop: Arc<AtomicBool>,
    max_hops: usize,
    memory_limit: usize,
) -> Result<(), String> {
    serve_hosts_inner(vec![host], stop, max_hops, memory_limit, false)
}

fn serve_hosts_inner(
    hosts: Vec<Arc<dyn ConnectionHost>>,
    stop: Arc<AtomicBool>,
    max_hops: usize,
    memory_limit: usize,
    process_signals: bool,
) -> Result<(), String> {
    if hosts.is_empty() {
        return Err("At least one listener is required".into());
    }
    if process_signals {
        install_process_signal_handlers()?;
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    crate::ws_tunnel::set_runtime_handle(rt.handle().clone());
    let memory_limit = memory_limit.clamp(
        MIN_BODY_MEMORY_LIMIT_MIB * MIB,
        MAX_BODY_MEMORY_LIMIT_MIB * MIB,
    );
    let result = rt.block_on(async {
        let mut tasks = tokio::task::JoinSet::new();
        for host in hosts {
            tasks.spawn(run(
                host,
                stop.clone(),
                max_hops.clamp(1, 256),
                memory_limit,
                process_signals,
            ));
        }
        let mut failure = None;
        while let Some(result) = tasks.join_next().await {
            let result = result.map_err(|e| e.to_string()).and_then(|result| result);
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
            stop.store(true, Ordering::Relaxed);
        }
        failure.map_or(Ok(()), Err)
    });
    crate::ws_tunnel::clear_runtime_handle();
    // `run` performs the graceful wait while the async scheduler and I/O
    // driver are still alive. This final bound only releases a stubborn
    // blocking worker after its permit-backed drain deadline elapsed.
    rt.shutdown_timeout(Duration::from_millis(100));
    result
}

async fn run(
    cfg: Arc<dyn ConnectionHost>,
    stop: Arc<AtomicBool>,
    max_hops: usize,
    body_memory_limit: usize,
    process_signals: bool,
) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(cfg.bind())
        .await
        .map_err(|e| format!("bind {}: {e}", cfg.bind()))?;
    cfg.ready(listener.local_addr().map_err(|error| error.to_string())?);
    cfg.log(&format!("listening on http://{}", cfg.bind()));
    let cursor = Arc::new(Mutex::new(0usize));
    let hop_slots = Arc::new(Semaphore::new(max_hops));
    let control_slots = Arc::new(Semaphore::new(CONTROL_PLANE_LIMIT));
    let health_slots = Arc::new(Semaphore::new(HEALTH_PLANE_LIMIT));
    let body_memory = Arc::new(Semaphore::new(body_memory_limit));
    let preflight_slots = Arc::new(Semaphore::new(REQUEST_HEAD_PREFLIGHT_LIMIT));
    let health_overflow_slots = Arc::new(Semaphore::new(HEALTH_OVERFLOW_PREFLIGHT_LIMIT));
    let replay_active = Arc::new(AtomicBool::new(false));
    let mut next_usage_replay = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed)
            || (process_signals && PROCESS_SHUTDOWN_REQUESTED.load(Ordering::Relaxed))
        {
            break;
        }
        if cfg.maintenance_enabled()
            && Instant::now() >= next_usage_replay
            && !replay_active.swap(true, Ordering::AcqRel)
        {
            next_usage_replay = Instant::now() + USAGE_REPLAY_INTERVAL;
            let host = Arc::clone(&cfg);
            let replay_active = Arc::clone(&replay_active);
            tokio::task::spawn_blocking(move || {
                host.maintenance();
                replay_active.store(false, Ordering::Release);
            });
        }
        let (stream, peer) =
            match tokio::time::timeout(Duration::from_millis(100), listener.accept()).await {
                Err(_) => continue,
                Ok(Ok(accepted)) => accepted,
                Ok(Err(_)) => {
                    cfg.log("listener accept failed; retrying");
                    tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
                    continue;
                }
            };
        let (preflight_permit, health_only) = match Arc::clone(&preflight_slots).try_acquire_owned()
        {
            Ok(permit) => (permit, false),
            Err(_) if peer.ip().is_loopback() => {
                match Arc::clone(&health_overflow_slots).try_acquire_owned() {
                    Ok(permit) => (permit, true),
                    Err(_) => {
                        // Before any bytes arrive there is no way to identify
                        // health traffic. Keep the fallback bounded and short;
                        // continuous connection floods belong at the edge.
                        drop(stream);
                        continue;
                    }
                }
            }
            Err(_) => {
                drop(stream);
                continue;
            }
        };
        let cfg = Arc::clone(&cfg);
        let cursor = Arc::clone(&cursor);
        let hop_slots = Arc::clone(&hop_slots);
        let control_slots = Arc::clone(&control_slots);
        let health_slots = Arc::clone(&health_slots);
        let body_memory = Arc::clone(&body_memory);
        tokio::spawn(async move {
            dispatch_connection(
                stream,
                cfg,
                cursor,
                hop_slots,
                control_slots,
                health_slots,
                body_memory,
                preflight_permit,
                health_only,
            )
            .await;
        });
    }
    drop(listener);
    let deadline = Instant::now() + SHUTDOWN_GRACE;
    loop {
        let drained = preflight_slots.available_permits() == REQUEST_HEAD_PREFLIGHT_LIMIT
            && health_overflow_slots.available_permits() == HEALTH_OVERFLOW_PREFLIGHT_LIMIT
            && hop_slots.available_permits() == max_hops
            && control_slots.available_permits() == CONTROL_PLANE_LIMIT
            && health_slots.available_permits() == HEALTH_PLANE_LIMIT
            && body_memory.available_permits() == body_memory_limit
            && !replay_active.load(Ordering::Acquire);
        if drained {
            break;
        }
        if Instant::now() >= deadline {
            cfg.log("shutdown drain deadline reached");
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionClass {
    Health,
    Provider,
    Control,
}

#[allow(clippy::result_unit_err)]
pub fn reserve_body_bytes(
    budget: Arc<Semaphore>,
    bytes: usize,
) -> Result<Option<OwnedSemaphorePermit>, ()> {
    if bytes == 0 {
        return Ok(None);
    }
    let permits = u32::try_from(bytes).map_err(|_| ())?;
    budget
        .try_acquire_many_owned(permits)
        .map(Some)
        .map_err(|_| ())
}

#[allow(clippy::too_many_arguments)]
pub async fn dispatch_connection(
    mut stream: tokio::net::TcpStream,
    cfg: Arc<dyn ConnectionHost>,
    cursor: Arc<Mutex<usize>>,
    hop_slots: Arc<Semaphore>,
    control_slots: Arc<Semaphore>,
    health_slots: Arc<Semaphore>,
    body_memory: Arc<Semaphore>,
    preflight_permit: OwnedSemaphorePermit,
    health_only: bool,
) {
    let (max_head_bytes, preflight_timeout) = if health_only {
        (
            HEALTH_OVERFLOW_MAX_HEAD_BYTES,
            HEALTH_OVERFLOW_PREFLIGHT_TIMEOUT,
        )
    } else {
        (
            crate::http::MAX_REQUEST_LINE_BYTES
                .saturating_add(crate::http::MAX_HEADER_BYTES)
                .saturating_add(4),
            REQUEST_HEAD_PREFLIGHT_TIMEOUT,
        )
    };
    let head = match await_complete_request_head(&stream, max_head_bytes, preflight_timeout).await {
        Ok(head) => head,
        Err(PreflightError::Closed) => return,
        Err(PreflightError::Timeout) => {
            write_async_status(&mut stream, 408, "Request Timeout", "request_timeout").await;
            return;
        }
        Err(PreflightError::TooLarge) => {
            write_async_status(
                &mut stream,
                431,
                "Request Header Fields Too Large",
                "request_headers_too_large",
            )
            .await;
            return;
        }
        Err(PreflightError::Io) => return,
    };
    if health_only
        && !(head.method.as_deref() == Some("GET")
            && head
                .path
                .as_deref()
                .is_some_and(crate::routes::is_health_path))
    {
        drain_request_head(&mut stream, head.bytes).await;
        write_async_status(
            &mut stream,
            503,
            "Service Unavailable",
            "request_preflight_capacity_exhausted",
        )
        .await;
        return;
    }
    let class = head
        .path
        .as_deref()
        .map(|path| cfg.classify(path))
        .unwrap_or(AdmissionClass::Control);
    let slots = match class {
        AdmissionClass::Health => health_slots,
        AdmissionClass::Provider => hop_slots,
        AdmissionClass::Control => control_slots,
    };
    let permit = match slots.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            let error = match class {
                AdmissionClass::Health => "health_capacity_exhausted",
                AdmissionClass::Provider => "relay_hop_capacity_exhausted",
                AdmissionClass::Control => "control_plane_capacity_exhausted",
            };
            drain_request_head(&mut stream, head.bytes).await;
            write_async_status(&mut stream, 503, "Service Unavailable", error).await;
            return;
        }
    };
    let body_bytes = cfg.body_reservation(&head, class);
    let body_permit = match reserve_body_bytes(body_memory, body_bytes) {
        Ok(permit) => permit,
        Err(()) => {
            drop(permit);
            drain_request_head(&mut stream, head.bytes).await;
            write_async_status(
                &mut stream,
                503,
                "Service Unavailable",
                "request_body_memory_exhausted",
            )
            .await;
            return;
        }
    };
    drop(preflight_permit);
    tokio::task::spawn_blocking(move || {
        run_blocking_connection(stream, cfg, cursor, permit, body_permit)
    });
}

fn run_blocking_connection(
    stream: tokio::net::TcpStream,
    cfg: Arc<dyn ConnectionHost>,
    cursor: Arc<Mutex<usize>>,
    _permit: OwnedSemaphorePermit,
    _body_permit: Option<OwnedSemaphorePermit>,
) {
    let std = match stream.into_std() {
        Ok(stream) => stream,
        Err(error) => {
            cfg.log(&format!("request failed: {error}"));
            return;
        }
    };
    let _ = std.set_nonblocking(false);
    let finish = std.try_clone().ok();
    if let Err(error) = cfg.handle(std, &cursor) {
        cfg.log(&format!("request failed: {error}"));
    }
    if let Some(mut stream) = finish {
        finish_response(&mut stream);
    }
}

// An early rejection can leave body bytes in the receive queue. Closing that
// socket immediately sends RST and may discard the response at the peer.
// Send FIN first, then drain a bounded amount within one total deadline.
fn finish_response(stream: &mut std::net::TcpStream) {
    use std::io::Read;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let deadline = Instant::now() + Duration::from_millis(50);
    let mut remaining = 64 * 1024;
    let mut buffer = [0; 4096];
    while remaining > 0 {
        let timeout = deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() || stream.set_read_timeout(Some(timeout)).is_err() {
            break;
        }
        let wanted = remaining.min(buffer.len());
        match stream.read(&mut buffer[..wanted]) {
            Ok(0) | Err(_) => break,
            Ok(count) => remaining -= count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreflightError {
    Closed,
    Timeout,
    TooLarge,
    Io,
}

pub struct PreflightHead {
    pub method: Option<String>,
    pub path: Option<String>,
    pub bytes: usize,
    pub parsed: Option<crate::http::HttpRequestHead>,
}

async fn await_complete_request_head(
    stream: &tokio::net::TcpStream,
    max: usize,
    timeout: Duration,
) -> Result<PreflightHead, PreflightError> {
    let mut bytes = vec![0_u8; REQUEST_HEAD_INITIAL_BYTES.min(max.saturating_add(1))];
    let read = async {
        loop {
            let count = stream
                .peek(&mut bytes)
                .await
                .map_err(|_| PreflightError::Io)?;
            if count == 0 {
                return Err(PreflightError::Closed);
            }
            if let Some(end) = bytes[..count]
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
            {
                let head_end = end + 4;
                if head_end > max {
                    return Err(PreflightError::TooLarge);
                }
                let parsed = crate::http::parse_http_request_head_bytes(&bytes[..head_end]).ok();
                let (method, path) = parsed
                    .as_ref()
                    .map(|head| (Some(head.method.clone()), Some(head.path.clone())))
                    .unwrap_or_else(|| {
                        preflight_request_line(&bytes[..head_end])
                            .map(|(method, path)| (Some(method), Some(path)))
                            .unwrap_or((None, None))
                    });
                return Ok(PreflightHead {
                    method,
                    path,
                    bytes: head_end,
                    parsed,
                });
            }
            if count > max {
                return Err(PreflightError::TooLarge);
            }
            if count == bytes.len() {
                let next = bytes.len().saturating_mul(2).min(max.saturating_add(1));
                if next == bytes.len() {
                    return Err(PreflightError::TooLarge);
                }
                bytes.resize(next, 0);
                continue;
            }
            // MSG_PEEK leaves the existing bytes readable, so yielding on
            // readiness alone would spin until the peer supplies more data.
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(timeout, read)
        .await
        .map_err(|_| PreflightError::Timeout)?
}

async fn drain_request_head(stream: &mut tokio::net::TcpStream, mut remaining: usize) {
    let mut buffer = [0_u8; 1024];
    while remaining > 0 {
        let wanted = remaining.min(buffer.len());
        match tokio::time::timeout(
            Duration::from_secs(1),
            stream.read_exact(&mut buffer[..wanted]),
        )
        .await
        {
            Ok(Ok(_)) => remaining -= wanted,
            _ => return,
        }
    }
}

pub fn preflight_request_line(head: &[u8]) -> Option<(String, String)> {
    let line_end = head.windows(2).position(|window| window == b"\r\n")?;
    if line_end > crate::http::MAX_REQUEST_LINE_BYTES {
        return None;
    }
    let line = std::str::from_utf8(&head[..line_end]).ok()?;
    let mut parts = line.split(' ');
    let method = parts.next()?;
    let path = parts.next()?;
    let version = parts.next()?;
    if method.is_empty()
        || path.is_empty()
        || version != "HTTP/1.1"
        || parts.next().is_some()
        || !path.starts_with('/')
    {
        return None;
    }
    Some((method.to_string(), path.to_string()))
}

async fn write_async_status(
    stream: &mut tokio::net::TcpStream,
    code: u16,
    reason: &str,
    error: &str,
) {
    let body = format!("{{\"error\":\"{error}\"}}");
    let response = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let wrote = tokio::time::timeout(Duration::from_secs(2), async {
        stream.write_all(response.as_bytes()).await?;
        stream.flush().await?;
        stream.shutdown().await
    })
    .await;
    let _ = wrote;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct LocalHost {
        calls: Arc<AtomicUsize>,
    }
    impl ConnectionHost for LocalHost {
        fn bind(&self) -> &str {
            "127.0.0.1:0"
        }
        fn log(&self, _: &str) {}
        fn classify(&self, _: &str) -> AdmissionClass {
            AdmissionClass::Provider
        }
        fn body_reservation(&self, _: &PreflightHead, _: AdmissionClass) -> usize {
            32
        }
        fn handle(&self, mut stream: std::net::TcpStream, _: &Mutex<usize>) -> Result<(), String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut reader = crate::http::HttpRequestReader::new(stream.try_clone().unwrap());
            crate::http::read_http_request_head(&mut reader).map_err(|error| error.to_string())?;
            crate::http::write_status(&mut stream, 200, "{\"host\":\"local\"}")
        }
    }

    async fn request(memory: usize) -> (String, usize) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        client
            .write_all(b"GET /v1/models HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let preflight = Arc::new(Semaphore::new(1)).acquire_owned().await.unwrap();
        dispatch_connection(
            server,
            Arc::new(LocalHost {
                calls: calls.clone(),
            }),
            Arc::new(Mutex::new(0)),
            Arc::new(Semaphore::new(1)),
            Arc::new(Semaphore::new(1)),
            Arc::new(Semaphore::new(1)),
            Arc::new(Semaphore::new(memory)),
            preflight,
            false,
        )
        .await;
        let mut response = String::new();
        tokio::time::timeout(Duration::from_secs(2), client.read_to_string(&mut response))
            .await
            .unwrap()
            .unwrap();
        (response, calls.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn local_host_uses_shared_listener_without_hosted_dependencies() {
        let (response, calls) = request(32).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("\"host\":\"local\""));
        assert_eq!(calls, 1);
    }

    #[tokio::test]
    async fn admission_rejects_before_invoking_local_provider() {
        let (response, calls) = request(31).await;
        assert!(response.starts_with("HTTP/1.1 503"), "{response}");
        assert!(response.contains("request_body_memory_exhausted"));
        assert_eq!(calls, 0);
    }
}
