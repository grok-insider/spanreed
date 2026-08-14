//! Local reverse proxy that captures official Grok/xAI API `usage` objects.
//!
//! Dual listeners (typical setup):
//! - `127.0.0.1:18736` → `https://cli-chat-proxy.grok.com`  (Grok CLI)
//! - `127.0.0.1:18737` → `https://api.x.ai`                 (OpenCode xAI)
//!
//! Upstream HTTPS honors `HTTP(S)_PROXY` so geo/VPN (e.g. sing-box :7897) still
//! applies. Clients talk to localhost in clear HTTP — no one-shot env needed if
//! wrappers/config permanently point at these ports.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::grok_ledger;
use crate::util;

pub const DEFAULT_GROK_CLI_BIND: &str = "127.0.0.1:18736";
pub const DEFAULT_XAI_API_BIND: &str = "127.0.0.1:18737";
pub const UPSTREAM_GROK_CLI: &str = "https://cli-chat-proxy.grok.com";
pub const UPSTREAM_XAI_API: &str = "https://api.x.ai";

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "host",
    "content-length",
];

static SEQ: AtomicU64 = AtomicU64::new(0);

/// One capture listener: local bind → fixed HTTPS upstream.
#[derive(Clone, Debug)]
pub struct ListenerConfig {
    pub bind: String,
    pub upstream: String,
    pub label: String,
}

/// Run dual (or custom) capture listeners until process exit.
pub fn run_capture(listeners: &[ListenerConfig]) -> Result<(), String> {
    if listeners.is_empty() {
        return Err("no capture listeners configured".into());
    }

    let client = build_client(None)?;
    let client = Arc::new(client);

    eprintln!("spanreed capture listening:");
    for l in listeners {
        eprintln!("  {}  http://{}  →  {}", l.label, l.bind, l.upstream);
    }
    eprintln!("ledger: {}", grok_ledger::ledger_path().display());
    eprintln!(
        "log:    {}",
        crate::capture_log::capture_log_path().display()
    );
    eprintln!("upstream HTTP(S)_PROXY: honored from environment (if set)");
    eprintln!();
    crate::capture_log::append(&format!(
        "listening {}",
        listeners
            .iter()
            .map(|l| format!("{}={}", l.label, l.bind))
            .collect::<Vec<_>>()
            .join(" ")
    ));

    let mut handles = Vec::new();
    for l in listeners {
        let cfg = l.clone();
        let client = Arc::clone(&client);
        let listener =
            TcpListener::bind(&cfg.bind).map_err(|e| format!("bind {}: {e}", cfg.bind))?;
        handles.push(std::thread::spawn(move || {
            accept_loop(listener, client, cfg)
        }));
    }

    // Block forever (or until a listener thread dies).
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

/// Single-listener mode (backward-compatible `grok-proxy` CLI).
pub fn run(bind: Option<&str>, upstream: Option<&str>) -> Result<(), String> {
    let bind = bind.unwrap_or(DEFAULT_GROK_CLI_BIND).to_string();
    let upstream = upstream
        .unwrap_or(UPSTREAM_GROK_CLI)
        .trim_end_matches('/')
        .to_string();
    run_capture(&[ListenerConfig {
        bind,
        upstream,
        label: "grok-cli".into(),
    }])
}

fn build_client(extra_proxy: Option<&str>) -> Result<reqwest::blocking::Client, String> {
    // Default: use system/env proxies (HTTP_PROXY/HTTPS_PROXY) so sing-box
    // egress still works when the capture unit sets those env vars.
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("spanreed-capture/0.1 (+{})", std::env::consts::OS));
    if let Some(url) = extra_proxy {
        let p = reqwest::Proxy::all(url).map_err(|e| format!("invalid egress proxy: {e}"))?;
        let no_proxy = reqwest::NoProxy::from_string("localhost,127.0.0.1,::1");
        builder = builder.proxy(p.no_proxy(no_proxy));
    }
    builder.build().map_err(|e| format!("http client: {e}"))
}

fn accept_loop(listener: TcpListener, client: Arc<reqwest::blocking::Client>, cfg: ListenerConfig) {
    for conn in listener.incoming() {
        let stream = match conn {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[{}] accept: {e}", cfg.label);
                continue;
            }
        };
        let client = Arc::clone(&client);
        let cfg = cfg.clone();
        std::thread::spawn(move || {
            if let Err(e) = handle_client(stream, &client, &cfg.upstream, &cfg.label) {
                log::warn!("[{}] request failed: {e}", cfg.label);
            }
        });
    }
}

fn handle_client(
    mut client: TcpStream,
    http: &reqwest::blocking::Client,
    upstream_base: &str,
    label: &str,
) -> Result<(), String> {
    client.set_read_timeout(Some(Duration::from_secs(600))).ok();
    client
        .set_write_timeout(Some(Duration::from_secs(600)))
        .ok();

    let mut reader = BufReader::new(client.try_clone().map_err(|e| e.to_string())?);
    let (method, path, headers, body) = match read_http_request(&mut reader) {
        Ok(r) => r,
        // Bare TCP connect (port probe) sends no HTTP — not an error.
        Err(e) if e.contains("empty request line") => return Ok(()),
        Err(e) => return Err(e),
    };

    // Local health (not forwarded). Used by ops / `capture status` checks.
    if is_local_health_path(&path) {
        let _ = method;
        return write_health_response(&mut client, label);
    }

    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let upstream_base = upstream_base.trim_end_matches('/');
    let url = format!("{upstream_base}{path}");

    let mut req = http.request(
        method.parse().map_err(|_| format!("bad method {method}"))?,
        &url,
    );
    for (k, v) in &headers {
        if HOP_BY_HOP.iter().any(|h| h.eq_ignore_ascii_case(k)) {
            continue;
        }
        req = req.header(k.as_str(), v.as_str());
    }
    if let Some(host) = upstream_base
        .strip_prefix("https://")
        .or_else(|| upstream_base.strip_prefix("http://"))
    {
        req = req.header("Host", host.split('/').next().unwrap_or(host));
    }
    if !body.is_empty() {
        req = req.body(body.clone());
    }

    let session_id = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-grok-session-id"))
        .map(|(_, v)| v.clone());

    let mut upstream = req.send().map_err(|e| format!("upstream: {e}"))?;
    let status = upstream.status();
    let resp_headers: Vec<(String, String)> = upstream
        .headers()
        .iter()
        .filter(|(k, _)| {
            let name = k.as_str();
            !HOP_BY_HOP.iter().any(|h| h.eq_ignore_ascii_case(name))
                && !name.eq_ignore_ascii_case("content-length")
                && !name.eq_ignore_ascii_case("transfer-encoding")
        })
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let mut captured = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    // If the client disconnects mid-stream (Broken pipe), keep draining upstream
    // into `captured` so we can still parse response.completed usage.
    let mut client_ok = true;
    {
        if write!(
            client,
            "HTTP/1.1 {} {}\r\n",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        )
        .is_err()
        {
            client_ok = false;
        }
        if client_ok {
            for (k, v) in &resp_headers {
                if write!(client, "{k}: {v}\r\n").is_err() {
                    client_ok = false;
                    break;
                }
            }
        }
        if client_ok && write!(client, "Transfer-Encoding: chunked\r\n\r\n").is_err() {
            client_ok = false;
        }
    }

    loop {
        match upstream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                captured.extend_from_slice(&buf[..n]);
                if client_ok {
                    let write_ok = write!(client, "{n:x}\r\n")
                        .and_then(|_| client.write_all(&buf[..n]))
                        .and_then(|_| client.write_all(b"\r\n"))
                        .and_then(|_| client.flush())
                        .is_ok();
                    if !write_ok {
                        client_ok = false;
                        log::warn!(
                            "[{label}] client disconnected mid-stream; draining upstream for usage"
                        );
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                log::warn!("[{label}] upstream read: {e}");
                break;
            }
        }
    }
    if client_ok {
        let _ = write!(client, "0\r\n\r\n");
        let _ = client.flush();
    }

    record_usage_from_capture(label, seq, &captured, session_id, !client_ok);
    Ok(())
}

/// Best-effort ledger write from whatever body bytes we already read.
fn record_usage_from_capture(
    label: &str,
    seq: u64,
    captured: &[u8],
    session_id: Option<String>,
    client_aborted: bool,
) {
    let text = String::from_utf8_lossy(captured);
    let Some(partial) = grok_ledger::usage_from_response_body(&text) else {
        if client_aborted {
            log::warn!(
                "[{label}] client aborted #{seq}: no usage in {} body bytes (lost)",
                captured.len()
            );
        } else if !captured.is_empty() {
            log::debug!(
                "[{label}] #{seq}: {} body bytes, no usage object",
                captured.len()
            );
        }
        return;
    };
    let rec = partial.into_record(util::now_ms(), session_id);
    if let Err(e) = grok_ledger::append(&rec) {
        log::warn!("[{label}] ledger append: {e}");
        return;
    }
    if client_aborted {
        log::warn!(
            "[{label}] captured #{seq} after client abort: in={} out={} total={} ticks={}",
            rec.input_tokens,
            rec.output_tokens,
            rec.total_tokens,
            rec.cost_usd_ticks
        );
    } else {
        log::info!(
            "[{label}] captured #{seq}: in={} out={} total={} ticks={}",
            rec.input_tokens,
            rec.output_tokens,
            rec.total_tokens,
            rec.cost_usd_ticks
        );
    }
}

type HttpRequest = (String, String, HashMap<String, String>, Vec<u8>);

fn is_local_health_path(path: &str) -> bool {
    let p = path.split('?').next().unwrap_or(path);
    p == "/__spanreed/health" || p == "/__spanreed/health/" || p.ends_with("/__spanreed/health")
}

fn write_health_response(client: &mut TcpStream, label: &str) -> Result<(), String> {
    let body = format!("{{\"ok\":true,\"service\":\"spanreed-capture\",\"label\":\"{label}\"}}");
    write!(
        client,
        "HTTP/1.1 200 OK\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    )
    .map_err(|e| format!("write health: {e}"))?;
    Ok(())
}

fn read_http_request(reader: &mut BufReader<TcpStream>) -> Result<HttpRequest, String> {
    let mut first = String::new();
    reader
        .read_line(&mut first)
        .map_err(|e| format!("read request line: {e}"))?;
    let first = first.trim_end_matches(['\r', '\n']);
    let mut parts = first.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "empty request line".to_string())?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| "missing path".to_string())?
        .to_string();

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("read header: {e}"))?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_string(), v.trim().to_string());
        }
    }

    let mut body = Vec::new();
    if let Some(cl) = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
    {
        body.resize(cl, 0);
        reader
            .read_exact(&mut body)
            .map_err(|e| format!("read body: {e}"))?;
    }

    Ok((method, path, headers, body))
}
