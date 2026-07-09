//! Local reverse proxy that captures official Grok/xAI API `usage` objects.
//!
//! Listens on `127.0.0.1` and forwards to `https://cli-chat-proxy.grok.com`,
//! streaming the response through while extracting usage from completed
//! Responses events into the grok ledger.
//!
//! Enable for the Grok CLI:
//! ```text
//! spanreed grok-proxy
//! GROK_CLI_CHAT_PROXY_BASE_URL=http://127.0.0.1:18736/v1 grok
//! ```

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::grok_ledger;
use crate::util;

const DEFAULT_BIND: &str = "127.0.0.1:18736";
const DEFAULT_UPSTREAM: &str = "https://cli-chat-proxy.grok.com";
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
    "content-length", // re-set from body
];

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Run the proxy until the process is killed. Never returns Ok on success path
/// (blocks forever); Err on bind failure.
pub fn run(bind: Option<&str>, upstream: Option<&str>) -> Result<(), String> {
    let bind = bind.unwrap_or(DEFAULT_BIND);
    let upstream = upstream.unwrap_or(DEFAULT_UPSTREAM).trim_end_matches('/');
    let listener = TcpListener::bind(bind).map_err(|e| format!("bind {bind}: {e}"))?;
    eprintln!("spanreed grok-proxy listening on http://{bind}");
    eprintln!("upstream: {upstream}");
    eprintln!("ledger:   {}", grok_ledger::ledger_path().display());
    eprintln!();
    eprintln!("Point Grok at this proxy:");
    eprintln!("  GROK_CLI_CHAT_PROXY_BASE_URL=http://{bind}/v1 grok");
    eprintln!();

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!(
            "spanreed-grok-proxy/0.1 (+{})",
            std::env::consts::OS
        ))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    for conn in listener.incoming() {
        let stream = match conn {
            Ok(s) => s,
            Err(e) => {
                log::warn!("accept: {e}");
                continue;
            }
        };
        let client = client.clone();
        let upstream = upstream.to_string();
        std::thread::spawn(move || {
            if let Err(e) = handle_client(stream, &client, &upstream) {
                log::warn!("proxy request failed: {e}");
            }
        });
    }
    Ok(())
}

fn handle_client(
    mut client: TcpStream,
    http: &reqwest::blocking::Client,
    upstream_base: &str,
) -> Result<(), String> {
    client.set_read_timeout(Some(Duration::from_secs(600))).ok();
    client
        .set_write_timeout(Some(Duration::from_secs(600)))
        .ok();

    let mut reader = BufReader::new(client.try_clone().map_err(|e| e.to_string())?);
    let (method, path, headers, body) = read_http_request(&mut reader)?;

    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
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

    // Capture full body while streaming to client (needed for SSE usage parse).
    let mut captured = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    // Write status + headers first with Transfer-Encoding: chunked so we can stream.
    {
        write!(
            client,
            "HTTP/1.1 {} {}\r\n",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        )
        .map_err(|e| e.to_string())?;
        for (k, v) in &resp_headers {
            write!(client, "{k}: {v}\r\n").map_err(|e| e.to_string())?;
        }
        write!(client, "Transfer-Encoding: chunked\r\n\r\n").map_err(|e| e.to_string())?;
    }

    loop {
        match upstream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                captured.extend_from_slice(&buf[..n]);
                // chunked encode
                write!(client, "{n:x}\r\n").map_err(|e| e.to_string())?;
                client.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                client.write_all(b"\r\n").map_err(|e| e.to_string())?;
                let _ = client.flush();
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                log::warn!("upstream read: {e}");
                break;
            }
        }
    }
    // end chunked
    let _ = write!(client, "0\r\n\r\n");
    let _ = client.flush();

    let text = String::from_utf8_lossy(&captured);
    if let Some(partial) = grok_ledger::usage_from_response_body(&text) {
        let rec = partial.into_record(util::now_ms(), session_id);
        if let Err(e) = grok_ledger::append(&rec) {
            log::warn!("ledger append: {e}");
        } else {
            log::info!(
                "captured usage #{seq}: in={} out={} total={} ticks={}",
                rec.input_tokens,
                rec.output_tokens,
                rec.total_tokens,
                rec.cost_usd_ticks
            );
        }
    }

    Ok(())
}

type HttpRequest = (String, String, HashMap<String, String>, Vec<u8>);

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
