//! Byte-pipe WebSocket upgrade to `api.x.ai` (STT / TTS stream / realtime).
//! Does not parse frames. Coolify TLS terminates in front; this hop is
//! plaintext to the client and TLS to xAI.

use std::collections::HashMap;
use std::fmt;
use std::net::TcpStream;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static RUNTIME: Mutex<Option<tokio::runtime::Handle>> = Mutex::new(None);

pub fn set_runtime_handle(handle: tokio::runtime::Handle) {
    *RUNTIME.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
}

pub fn clear_runtime_handle() {
    *RUNTIME.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_rustls::rustls::pki_types::ServerName;

const WS_HOP_DROP: &[&str] = &[
    "host",
    "content-length",
    "content-type",
    "transfer-encoding",
    "keep-alive",
    "te",
    "trailer",
    "trailers",
    "proxy-authenticate",
    "proxy-authorization",
];

const MAX_RESPONSE_HEAD_BYTES: usize = 64 * 1024;
const MAX_INFORMATIONAL_RESPONSES: usize = 8;
const MAX_CHUNK_LINE_BYTES: usize = 1024;
const MAX_TRAILER_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy)]
struct TunnelTimeouts {
    connect: Duration,
    tls: Duration,
    handshake: Duration,
    rejection_body: Duration,
}

const DEFAULT_TIMEOUTS: TunnelTimeouts = TunnelTimeouts {
    connect: Duration::from_secs(15),
    tls: Duration::from_secs(15),
    handshake: Duration::from_secs(30),
    rejection_body: Duration::from_secs(30),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TunnelResult {
    pub upgraded: bool,
    pub status: u16,
    pub duration_ms: u64,
}

#[derive(Debug)]
pub struct TunnelError {
    detail: String,
    /// True once any upstream response bytes may have reached the caller.
    pub response_started: bool,
    /// The exact upstream status, when a valid response head was received.
    pub status: Option<u16>,
}

impl TunnelError {
    fn before_response(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            response_started: false,
            status: None,
        }
    }

    fn after_response(status: u16, detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            response_started: true,
            status: Some(status),
        }
    }
}

impl fmt::Display for TunnelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for TunnelError {}

pub fn is_websocket_upgrade(method: &str, headers: &HashMap<String, String>) -> bool {
    if !method.eq_ignore_ascii_case("GET") {
        return false;
    }
    let upgrade = header_has_token(headers, "upgrade", "websocket");
    let conn = header_has_token(headers, "connection", "upgrade");
    upgrade && conn
}

/// Streaming voice paths on `api.x.ai` (not Imagine HTTP).
pub fn is_voice_ws_path(path: &str) -> bool {
    let p = path.split('?').next().unwrap_or(path).trim_end_matches('/');
    p == "/v1/stt" || p == "/v1/tts" || p == "/v1/realtime" || p.starts_with("/v1/realtime/")
}

fn header(headers: &HashMap<String, String>, name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.to_ascii_lowercase())
}

fn header_has_token(headers: &HashMap<String, String>, name: &str, expected: &str) -> bool {
    header(headers, name).is_some_and(|value| {
        value
            .split(',')
            .any(|token| token.trim().eq_ignore_ascii_case(expected))
    })
}

fn connection_nominates(headers: &HashMap<String, String>, name: &str) -> bool {
    header_has_token(headers, "connection", name)
}

pub fn tunnel(
    client: &TcpStream,
    method: &str,
    upstream_url: &url::Url,
    client_headers: &HashMap<String, String>,
    inject: &[(String, String)],
    strip_client_authorization: bool,
    prefetched_client_bytes: Vec<u8>,
) -> Result<TunnelResult, TunnelError> {
    let started = std::time::Instant::now();
    let endpoint = UpstreamEndpoint::from_url(upstream_url)?;
    let socket = client
        .try_clone()
        .map_err(|error| TunnelError::before_response(format!("clone client socket: {error}")))?;
    let method = method.to_string();
    let headers = client_headers.clone();
    let inject = inject.to_vec();
    let handle = RUNTIME.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let result = if let Some(handle) = handle {
        handle.block_on(tunnel_async(
            socket,
            &method,
            &headers,
            &inject,
            endpoint,
            strip_client_authorization,
            prefetched_client_bytes,
            DEFAULT_TIMEOUTS,
        ))
    } else {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|error| TunnelError::before_response(format!("websocket runtime: {error}")))?;
        runtime.block_on(tunnel_async(
            socket,
            &method,
            &headers,
            &inject,
            endpoint,
            strip_client_authorization,
            prefetched_client_bytes,
            DEFAULT_TIMEOUTS,
        ))
    };
    // `tokio::net::TcpStream::from_std` requires nonblocking mode, and that
    // flag can be shared by duplicated descriptors on Unix. Restore the
    // retained caller handle before it may synthesize a pre-response 502.
    let _ = client.set_nonblocking(false);
    let _ = client.set_read_timeout(None);
    let _ = client.set_write_timeout(Some(Duration::from_secs(120)));
    result.map(|(upgraded, status)| TunnelResult {
        upgraded,
        status,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpstreamEndpoint {
    https: bool,
    connect_host: String,
    port: u16,
    authority: String,
    target: String,
}

impl UpstreamEndpoint {
    fn from_url(url: &url::Url) -> Result<Self, TunnelError> {
        let https = match url.scheme() {
            "https" => true,
            "http" => false,
            _ => {
                return Err(TunnelError::before_response(
                    "unsupported websocket upstream scheme",
                ))
            }
        };
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(TunnelError::before_response(
                "invalid websocket upstream URL",
            ));
        }
        let host = url
            .host()
            .ok_or_else(|| TunnelError::before_response("websocket upstream has no host"))?;
        let (connect_host, display_host) = match host {
            url::Host::Domain(host) => (host.to_string(), host.to_string()),
            url::Host::Ipv4(host) => (host.to_string(), host.to_string()),
            url::Host::Ipv6(host) => (host.to_string(), format!("[{host}]")),
        };
        let port = url
            .port_or_known_default()
            .ok_or_else(|| TunnelError::before_response("websocket upstream has no port"))?;
        let authority = match url.port() {
            Some(explicit) => format!("{display_host}:{explicit}"),
            None => display_host,
        };
        let mut target = if url.path().is_empty() {
            "/".to_string()
        } else {
            url.path().to_string()
        };
        if let Some(query) = url.query() {
            target.push('?');
            target.push_str(query);
        }
        Ok(Self {
            https,
            connect_host,
            port,
            authority,
            target,
        })
    }
}

#[allow(clippy::too_many_arguments)]
async fn tunnel_async(
    client: TcpStream,
    method: &str,
    client_headers: &HashMap<String, String>,
    inject: &[(String, String)],
    endpoint: UpstreamEndpoint,
    strip_client_authorization: bool,
    prefetched_client_bytes: Vec<u8>,
    timeouts: TunnelTimeouts,
) -> Result<(bool, u16), TunnelError> {
    let _ = client.set_nodelay(true);
    let _ = client.set_read_timeout(None);
    let _ = client.set_write_timeout(None);
    client.set_nonblocking(true).map_err(|error| {
        TunnelError::before_response(format!("configure client socket: {error}"))
    })?;
    let client = tokio::net::TcpStream::from_std(client)
        .map_err(|error| TunnelError::before_response(format!("adopt client socket: {error}")))?;
    tunnel_async_rw(
        client,
        method,
        client_headers,
        inject,
        endpoint,
        strip_client_authorization,
        prefetched_client_bytes,
        timeouts,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn tunnel_async_rw<C>(
    client: C,
    method: &str,
    client_headers: &HashMap<String, String>,
    inject: &[(String, String)],
    endpoint: UpstreamEndpoint,
    strip_client_authorization: bool,
    prefetched_client_bytes: Vec<u8>,
    timeouts: TunnelTimeouts,
) -> Result<(bool, u16), TunnelError>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    let tcp = tokio::time::timeout(
        timeouts.connect,
        tokio::net::TcpStream::connect((endpoint.connect_host.as_str(), endpoint.port)),
    )
    .await
    .map_err(|_| TunnelError::before_response("websocket upstream connect timeout"))?
    .map_err(|error| {
        TunnelError::before_response(format!("websocket upstream connect: {error}"))
    })?;
    let _ = tcp.set_nodelay(true);

    if endpoint.https {
        let tls = tokio::time::timeout(timeouts.tls, tls_connect(tcp, &endpoint.connect_host))
            .await
            .map_err(|_| TunnelError::before_response("websocket upstream TLS timeout"))?
            .map_err(TunnelError::before_response)?;
        pump(
            client,
            tls,
            method,
            &endpoint,
            client_headers,
            inject,
            strip_client_authorization,
            &prefetched_client_bytes,
            timeouts,
        )
        .await
    } else {
        pump(
            client,
            tcp,
            method,
            &endpoint,
            client_headers,
            inject,
            strip_client_authorization,
            &prefetched_client_bytes,
            timeouts,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn pump<C, U>(
    mut client: C,
    mut upstream: U,
    method: &str,
    endpoint: &UpstreamEndpoint,
    client_headers: &HashMap<String, String>,
    inject: &[(String, String)],
    strip_client_authorization: bool,
    prefetched_client_bytes: &[u8],
    timeouts: TunnelTimeouts,
) -> Result<(bool, u16), TunnelError>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    let request = build_handshake(
        method,
        &endpoint.target,
        &endpoint.authority,
        client_headers,
        inject,
        strip_client_authorization,
    )
    .map_err(TunnelError::before_response)?;
    tokio::time::timeout(timeouts.handshake, async {
        upstream.write_all(request.as_bytes()).await?;
        upstream.flush().await
    })
    .await
    .map_err(|_| TunnelError::before_response("websocket handshake write timeout"))?
    .map_err(|error| TunnelError::before_response(format!("write websocket handshake: {error}")))?;

    let response = tokio::time::timeout(timeouts.handshake, read_http_response_head(&mut upstream))
        .await
        .map_err(|_| TunnelError::before_response("websocket handshake response timeout"))?
        .map_err(TunnelError::before_response)?;
    let status = response.parsed.status;
    if status == 101 && !response.parsed.valid_websocket_upgrade {
        return Err(TunnelError::before_response(
            "invalid websocket upgrade response",
        ));
    }

    let client_head = sanitize_response_head(&response.head, status != 101);
    tokio::time::timeout(timeouts.handshake, async {
        client.write_all(&client_head).await?;
        client.flush().await
    })
    .await
    .map_err(|_| TunnelError::after_response(status, "client handshake write timeout"))?
    .map_err(|error| {
        TunnelError::after_response(status, format!("write client handshake: {error}"))
    })?;

    if status != 101 {
        tokio::time::timeout(
            timeouts.rejection_body,
            relay_rejection_body(
                &mut upstream,
                &mut client,
                &response.body_prefix,
                response.parsed.framing,
            ),
        )
        .await
        .map_err(|_| TunnelError::after_response(status, "upstream rejection body timeout"))?
        .map_err(|error| TunnelError::after_response(status, error))?;
        client
            .flush()
            .await
            .map_err(|error| TunnelError::after_response(status, error.to_string()))?;
        return Ok((false, status));
    }

    if !response.body_prefix.is_empty() {
        tokio::time::timeout(timeouts.handshake, client.write_all(&response.body_prefix))
            .await
            .map_err(|_| TunnelError::after_response(status, "initial upstream frame timeout"))?
            .map_err(|error| {
                TunnelError::after_response(
                    status,
                    format!("write initial upstream frame: {error}"),
                )
            })?;
    }
    if !prefetched_client_bytes.is_empty() {
        tokio::time::timeout(timeouts.handshake, async {
            upstream.write_all(prefetched_client_bytes).await?;
            upstream.flush().await
        })
        .await
        .map_err(|_| TunnelError::after_response(status, "initial client frame timeout"))?
        .map_err(|error| {
            TunnelError::after_response(status, format!("write initial client frame: {error}"))
        })?;
    }
    tokio::io::copy_bidirectional(&mut client, &mut upstream)
        .await
        .map_err(|error| {
            TunnelError::after_response(status, format!("websocket tunnel copy: {error}"))
        })?;
    Ok((true, status))
}

async fn tls_connect(
    tcp: tokio::net::TcpStream,
    host: &str,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, String> {
    let name = ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
    let connector = tokio_rustls::TlsConnector::from(tls_config());
    connector
        .connect(name, tcp)
        .await
        .map_err(|e| format!("tls: {e}"))
}

fn tls_config() -> std::sync::Arc<tokio_rustls::rustls::ClientConfig> {
    static CFG: OnceLock<std::sync::Arc<tokio_rustls::rustls::ClientConfig>> = OnceLock::new();
    CFG.get_or_init(|| {
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
        let mut roots = tokio_rustls::rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let cfg = tokio_rustls::rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        std::sync::Arc::new(cfg)
    })
    .clone()
}

fn build_handshake(
    method: &str,
    path: &str,
    host: &str,
    client_headers: &HashMap<String, String>,
    inject: &[(String, String)],
    strip_client_authorization: bool,
) -> Result<String, String> {
    if !valid_header_token(method.as_bytes())
        || path
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        || host
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err("invalid websocket request line".into());
    }
    for (index, (name, value)) in inject.iter().enumerate() {
        if !valid_header_token(name.as_bytes())
            || !valid_header_value(value.as_bytes())
            || name.eq_ignore_ascii_case("connection")
            || name.eq_ignore_ascii_case("upgrade")
            || WS_HOP_DROP
                .iter()
                .any(|reserved| reserved.eq_ignore_ascii_case(name))
        {
            return Err("invalid injected websocket header".into());
        }
        if inject[..index]
            .iter()
            .any(|(previous, _)| previous.eq_ignore_ascii_case(name))
        {
            return Err("duplicate injected websocket header".into());
        }
    }
    let path = if path.is_empty() { "/" } else { path };
    let mut out = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\n");
    let mut skip_auth = strip_client_authorization;
    for (k, _) in inject {
        if k.eq_ignore_ascii_case("authorization") {
            skip_auth = true;
        }
    }
    for (k, v) in client_headers {
        // The incoming upgrade was validated before tunneling. Rebuild these
        // hop headers below so client Connection nominations cannot suppress
        // credentials injected for the upstream handshake.
        if k.eq_ignore_ascii_case("connection") || k.eq_ignore_ascii_case("upgrade") {
            continue;
        }
        if WS_HOP_DROP.iter().any(|h| h.eq_ignore_ascii_case(k)) {
            continue;
        }
        if connection_nominates(client_headers, k)
            && !k.eq_ignore_ascii_case("connection")
            && !k.eq_ignore_ascii_case("upgrade")
        {
            continue;
        }
        if skip_auth && k.eq_ignore_ascii_case("authorization") {
            continue;
        }
        if inject
            .iter()
            .any(|(injected, _)| injected.eq_ignore_ascii_case(k))
        {
            continue;
        }
        if k.eq_ignore_ascii_case("x-xai-token-auth") {
            continue;
        }
        if is_relay_only_header(k) {
            continue;
        }
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    for (k, v) in inject {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("Upgrade: websocket\r\nConnection: Upgrade\r\n");
    out.push_str("\r\n");
    Ok(out)
}

fn is_relay_only_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("cookie")
        || name.eq_ignore_ascii_case("forwarded")
        || name.eq_ignore_ascii_case("x-real-ip")
        || name.eq_ignore_ascii_case("cf-connecting-ip")
        || name.eq_ignore_ascii_case("true-client-ip")
        || name.eq_ignore_ascii_case("referer")
        || name.to_ascii_lowercase().starts_with("x-forwarded-")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseBodyFraming {
    None,
    ContentLength(usize),
    Chunked,
    UntilEof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedResponseHead {
    status: u16,
    valid_websocket_upgrade: bool,
    framing: ResponseBodyFraming,
}

struct UpstreamResponseHead {
    head: Vec<u8>,
    body_prefix: Vec<u8>,
    parsed: ParsedResponseHead,
}

async fn read_http_response_head<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<UpstreamResponseHead, String> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 512];
    let mut informational_responses = 0usize;
    loop {
        let end = loop {
            if let Some(index) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
            if buf.len() > MAX_RESPONSE_HEAD_BYTES {
                return Err("upstream handshake too large".into());
            }
            let n = reader.read(&mut tmp).await.map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("upstream closed during handshake".into());
            }
            buf.extend_from_slice(&tmp[..n]);
        };
        if end > MAX_RESPONSE_HEAD_BYTES {
            return Err("upstream handshake too large".into());
        }
        let remainder = buf.split_off(end);
        let head = std::mem::replace(&mut buf, remainder);
        let parsed = parse_response_head(&head)?;
        // A server may send 100/102/103 before its actual upgrade decision.
        // Consume bounded informational heads so the caller receives and
        // records the final 101 or rejection status rather than closing early.
        if (100..200).contains(&parsed.status) && parsed.status != 101 {
            informational_responses += 1;
            if informational_responses > MAX_INFORMATIONAL_RESPONSES {
                return Err("too many upstream informational responses".into());
            }
            continue;
        }
        return Ok(UpstreamResponseHead {
            head,
            body_prefix: buf,
            parsed,
        });
    }
}

fn parse_response_head(head: &[u8]) -> Result<ParsedResponseHead, String> {
    if !head.ends_with(b"\r\n\r\n") {
        return Err("incomplete upstream response head".into());
    }
    let mut cursor = 0usize;
    let status_line = next_crlf_line(head, &mut cursor)
        .ok_or_else(|| "missing upstream status line".to_string())?;
    let status_line =
        std::str::from_utf8(status_line).map_err(|_| "invalid upstream status line".to_string())?;
    let mut status_parts = status_line.splitn(3, ' ');
    let version = status_parts.next().unwrap_or_default();
    let code = status_parts.next().unwrap_or_default();
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || code.len() != 3
        || !code.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("invalid upstream status line".into());
    }
    let status = code
        .parse::<u16>()
        .ok()
        .filter(|status| (100..=599).contains(status))
        .ok_or_else(|| "invalid upstream status".to_string())?;

    let mut content_length = None;
    let mut saw_transfer_encoding = false;
    let mut transfer_chunked = false;
    let mut connection_upgrade = false;
    let mut upgrade_websocket = false;
    loop {
        let line = next_crlf_line(head, &mut cursor)
            .ok_or_else(|| "incomplete upstream response headers".to_string())?;
        if line.is_empty() {
            break;
        }
        let colon = line
            .iter()
            .position(|byte| *byte == b':')
            .ok_or_else(|| "invalid upstream response header".to_string())?;
        let name = &line[..colon];
        let value = trim_ascii_ows(&line[colon + 1..]);
        if !valid_header_token(name) || !valid_header_value(value) {
            return Err("invalid upstream response header".into());
        }
        if name.eq_ignore_ascii_case(b"content-length") {
            if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
                return Err("invalid upstream content-length".into());
            }
            let length = std::str::from_utf8(value)
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| "upstream content-length overflow".to_string())?;
            if content_length.is_some_and(|existing| existing != length) {
                return Err("conflicting upstream content-length".into());
            }
            content_length = Some(length);
        } else if name.eq_ignore_ascii_case(b"transfer-encoding") {
            saw_transfer_encoding = true;
            let tokens: Vec<&[u8]> = value
                .split(|byte| *byte == b',')
                .map(trim_ascii_ows)
                .collect();
            if tokens.iter().any(|token| token.is_empty()) {
                return Err("invalid upstream transfer-encoding".into());
            }
            transfer_chunked = tokens
                .last()
                .is_some_and(|token| token.eq_ignore_ascii_case(b"chunked"));
        } else if name.eq_ignore_ascii_case(b"connection") {
            connection_upgrade |= comma_value_has_token(value, b"upgrade");
        } else if name.eq_ignore_ascii_case(b"upgrade") {
            upgrade_websocket |= comma_value_has_token(value, b"websocket");
        }
    }
    if saw_transfer_encoding && content_length.is_some() {
        return Err("ambiguous upstream response framing".into());
    }
    let no_body = (100..200).contains(&status) || matches!(status, 204 | 304);
    let framing = if no_body {
        ResponseBodyFraming::None
    } else if saw_transfer_encoding && transfer_chunked {
        ResponseBodyFraming::Chunked
    } else if saw_transfer_encoding {
        ResponseBodyFraming::UntilEof
    } else if let Some(length) = content_length {
        ResponseBodyFraming::ContentLength(length)
    } else {
        ResponseBodyFraming::UntilEof
    };
    Ok(ParsedResponseHead {
        status,
        valid_websocket_upgrade: status == 101 && connection_upgrade && upgrade_websocket,
        framing,
    })
}

fn next_crlf_line<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    let rest = bytes.get(*cursor..)?;
    let end = rest.windows(2).position(|window| window == b"\r\n")?;
    let line = &rest[..end];
    *cursor += end + 2;
    Some(line)
}

fn valid_header_token(value: &[u8]) -> bool {
    !value.is_empty()
        && value.iter().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn valid_header_value(value: &[u8]) -> bool {
    value
        .iter()
        .all(|byte| *byte == b'\t' || (*byte >= b' ' && *byte != 0x7f))
}

fn trim_ascii_ows(mut value: &[u8]) -> &[u8] {
    while matches!(value.first(), Some(b' ' | b'\t')) {
        value = &value[1..];
    }
    while matches!(value.last(), Some(b' ' | b'\t')) {
        value = &value[..value.len() - 1];
    }
    value
}

fn comma_value_has_token(value: &[u8], expected: &[u8]) -> bool {
    value
        .split(|byte| *byte == b',')
        .map(trim_ascii_ows)
        .any(|token| token.eq_ignore_ascii_case(expected))
}

fn is_sensitive_upstream_header(name: &[u8]) -> bool {
    name.eq_ignore_ascii_case(b"set-cookie")
        || name.eq_ignore_ascii_case(b"set-cookie2")
        || name.eq_ignore_ascii_case(b"clear-site-data")
        || name
            .get(..b"access-control-".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"access-control-"))
}

/// Rebuild an untrusted upstream response head before exposing it on the
/// relay/dashboard origin. Rejections also become connection-closing because
/// this one-request proxy does not parse another request on the socket.
fn sanitize_response_head(head: &[u8], force_close: bool) -> Vec<u8> {
    let mut output = Vec::with_capacity(head.len() + 24);
    let mut cursor = 0usize;
    if let Some(status_line) = next_crlf_line(head, &mut cursor) {
        output.extend_from_slice(status_line);
        output.extend_from_slice(b"\r\n");
    }
    while let Some(line) = next_crlf_line(head, &mut cursor) {
        if line.is_empty() {
            break;
        }
        let name = line
            .iter()
            .position(|byte| *byte == b':')
            .map(|colon| &line[..colon]);
        if name.is_some_and(is_sensitive_upstream_header) {
            continue;
        }
        if force_close
            && name.is_some_and(|name| {
                name.eq_ignore_ascii_case(b"connection")
                    || name.eq_ignore_ascii_case(b"keep-alive")
                    || name.eq_ignore_ascii_case(b"proxy-connection")
            })
        {
            continue;
        }
        output.extend_from_slice(line);
        output.extend_from_slice(b"\r\n");
    }
    if force_close {
        output.extend_from_slice(b"Connection: close\r\n");
    }
    output.extend_from_slice(b"\r\n");
    output
}

async fn relay_rejection_body<U, C>(
    upstream: &mut U,
    client: &mut C,
    body_prefix: &[u8],
    framing: ResponseBodyFraming,
) -> Result<(), String>
where
    U: AsyncRead + Unpin,
    C: AsyncWrite + Unpin,
{
    match framing {
        ResponseBodyFraming::None => Ok(()),
        ResponseBodyFraming::ContentLength(length) => {
            let prefix_len = body_prefix.len().min(length);
            client
                .write_all(&body_prefix[..prefix_len])
                .await
                .map_err(|error| format!("write rejection body: {error}"))?;
            let mut remaining = length - prefix_len;
            let mut buffer = [0u8; 8192];
            while remaining > 0 {
                let want = remaining.min(buffer.len());
                let read = upstream
                    .read(&mut buffer[..want])
                    .await
                    .map_err(|error| format!("read rejection body: {error}"))?;
                if read == 0 {
                    return Err("upstream closed before rejection body completed".into());
                }
                client
                    .write_all(&buffer[..read])
                    .await
                    .map_err(|error| format!("write rejection body: {error}"))?;
                remaining -= read;
            }
            Ok(())
        }
        ResponseBodyFraming::Chunked => relay_chunked_body(upstream, client, body_prefix).await,
        ResponseBodyFraming::UntilEof => {
            client
                .write_all(body_prefix)
                .await
                .map_err(|error| format!("write rejection body: {error}"))?;
            tokio::io::copy(upstream, client)
                .await
                .map(|_| ())
                .map_err(|error| format!("copy rejection body: {error}"))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkDecodeState {
    Size,
    Data(u64),
    DataCr,
    DataLf,
    Trailer,
    Done,
}

struct ChunkedBodyDecoder {
    state: ChunkDecodeState,
    line: Vec<u8>,
    trailer_bytes: usize,
}

impl ChunkedBodyDecoder {
    fn new() -> Self {
        Self {
            state: ChunkDecodeState::Size,
            line: Vec::new(),
            trailer_bytes: 0,
        }
    }

    /// Return the number of raw bytes belonging to this chunked message.
    fn feed(&mut self, bytes: &[u8]) -> Result<usize, String> {
        for (index, byte) in bytes.iter().copied().enumerate() {
            match self.state {
                ChunkDecodeState::Size => {
                    if self.push_line_byte(byte, MAX_CHUNK_LINE_BYTES)? {
                        let line = std::mem::take(&mut self.line);
                        let size = parse_chunk_size(&line)?;
                        self.state = if size == 0 {
                            ChunkDecodeState::Trailer
                        } else {
                            ChunkDecodeState::Data(size)
                        };
                    }
                }
                ChunkDecodeState::Data(remaining) => {
                    self.state = if remaining == 1 {
                        ChunkDecodeState::DataCr
                    } else {
                        ChunkDecodeState::Data(remaining - 1)
                    };
                }
                ChunkDecodeState::DataCr => {
                    if byte != b'\r' {
                        return Err("invalid upstream chunk delimiter".into());
                    }
                    self.state = ChunkDecodeState::DataLf;
                }
                ChunkDecodeState::DataLf => {
                    if byte != b'\n' {
                        return Err("invalid upstream chunk delimiter".into());
                    }
                    self.state = ChunkDecodeState::Size;
                }
                ChunkDecodeState::Trailer => {
                    self.trailer_bytes = self.trailer_bytes.saturating_add(1);
                    if self.trailer_bytes > MAX_TRAILER_BYTES {
                        return Err("upstream chunk trailers too large".into());
                    }
                    if self.push_line_byte(byte, MAX_CHUNK_LINE_BYTES)? {
                        let line = std::mem::take(&mut self.line);
                        if line.is_empty() {
                            self.state = ChunkDecodeState::Done;
                            return Ok(index + 1);
                        }
                        let colon = line
                            .iter()
                            .position(|byte| *byte == b':')
                            .ok_or_else(|| "invalid upstream chunk trailer".to_string())?;
                        if !valid_header_token(&line[..colon])
                            || !valid_header_value(trim_ascii_ows(&line[colon + 1..]))
                        {
                            return Err("invalid upstream chunk trailer".into());
                        }
                    }
                }
                ChunkDecodeState::Done => return Ok(index),
            }
        }
        Ok(bytes.len())
    }

    fn push_line_byte(&mut self, byte: u8, max: usize) -> Result<bool, String> {
        if self.line.last() == Some(&b'\r') && byte != b'\n' {
            return Err("invalid upstream chunk line ending".into());
        }
        self.line.push(byte);
        if self.line.len() > max {
            return Err("upstream chunk line too large".into());
        }
        if byte == b'\n' {
            if self.line.len() < 2 || self.line[self.line.len() - 2] != b'\r' {
                return Err("invalid upstream chunk line ending".into());
            }
            self.line.truncate(self.line.len() - 2);
            return Ok(true);
        }
        Ok(false)
    }

    fn done(&self) -> bool {
        self.state == ChunkDecodeState::Done
    }
}

fn parse_chunk_size(line: &[u8]) -> Result<u64, String> {
    let (size, extension) = match line.iter().position(|byte| *byte == b';') {
        Some(index) => (&line[..index], &line[index + 1..]),
        None => (line, &[][..]),
    };
    if size.is_empty() || !size.iter().all(u8::is_ascii_hexdigit) {
        return Err("invalid upstream chunk size".into());
    }
    if extension
        .iter()
        .any(|byte| *byte < b' ' || *byte == 0x7f || !byte.is_ascii())
    {
        return Err("invalid upstream chunk extension".into());
    }
    let size = std::str::from_utf8(size).map_err(|_| "invalid upstream chunk size")?;
    u64::from_str_radix(size, 16).map_err(|_| "upstream chunk size overflow".into())
}

async fn relay_chunked_body<U, C>(
    upstream: &mut U,
    client: &mut C,
    body_prefix: &[u8],
) -> Result<(), String>
where
    U: AsyncRead + Unpin,
    C: AsyncWrite + Unpin,
{
    let mut decoder = ChunkedBodyDecoder::new();
    let mut buffer = [0u8; 8192];
    let mut current = body_prefix;
    loop {
        if !current.is_empty() {
            let consumed = decoder.feed(current)?;
            client
                .write_all(&current[..consumed])
                .await
                .map_err(|error| format!("write chunked rejection body: {error}"))?;
            if decoder.done() {
                return Ok(());
            }
        }
        let read = upstream
            .read(&mut buffer)
            .await
            .map_err(|error| format!("read chunked rejection body: {error}"))?;
        if read == 0 {
            return Err("upstream closed before chunked rejection body completed".into());
        }
        current = &buffer[..read];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct FailAfterResponseHead {
        response: &'static [u8],
        offset: usize,
    }

    impl AsyncRead for FailAfterResponseHead {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buffer: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if self.offset == self.response.len() {
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "simulated tunnel reset",
                )));
            }
            let remaining = &self.response[self.offset..];
            let count = remaining.len().min(buffer.remaining());
            buffer.put_slice(&remaining[..count]);
            self.offset += count;
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for FailAfterResponseHead {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buffer: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Ok(buffer.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn upgrade_detection() {
        let mut h = HashMap::new();
        h.insert("Upgrade".into(), "websocket".into());
        h.insert("Connection".into(), "keep-alive, Upgrade".into());
        assert!(is_websocket_upgrade("GET", &h));
        assert!(!is_websocket_upgrade("POST", &h));
        h.insert("Upgrade".into(), "notwebsocket".into());
        assert!(!is_websocket_upgrade("GET", &h));
        h.insert("Upgrade".into(), "websocket".into());
        h.insert("Connection".into(), "xupgrade".into());
        assert!(!is_websocket_upgrade("GET", &h));
        h.remove("Upgrade");
        assert!(!is_websocket_upgrade("GET", &h));
    }

    #[test]
    fn voice_paths() {
        assert!(is_voice_ws_path("/v1/stt"));
        assert!(is_voice_ws_path("/v1/stt?sample_rate=16000"));
        assert!(is_voice_ws_path("/v1/tts"));
        assert!(is_voice_ws_path("/v1/realtime?model=grok-voice-latest"));
        assert!(!is_voice_ws_path("/v1/images/generations"));
        assert!(!is_voice_ws_path("/v1/responses"));
    }

    #[test]
    fn handshake_omits_cli_token_auth() {
        let mut h = HashMap::new();
        h.insert("upgrade".into(), "websocket".into());
        h.insert("connection".into(), "Upgrade".into());
        h.insert("authorization".into(), "Bearer client".into());
        h.insert("x-xai-token-auth".into(), "xai-grok-cli".into());
        h.insert("sec-websocket-key".into(), "abc".into());
        let raw = build_handshake(
            "GET",
            "/v1/stt?encoding=pcm",
            "api.x.ai",
            &h,
            &[
                ("Authorization".into(), "Bearer tok".into()),
                ("x-grok-client-identifier".into(), "grok-cli".into()),
            ],
            false,
        )
        .unwrap();
        assert!(raw.starts_with("GET /v1/stt?encoding=pcm HTTP/1.1\r\n"));
        assert!(raw.contains("Authorization: Bearer tok"));
        assert!(!raw.to_ascii_lowercase().contains("x-xai-token-auth"));
        assert!(!raw.contains("Bearer client"));
        assert!(raw.contains("sec-websocket-key: abc") || raw.contains("Sec-WebSocket-Key: abc"));
    }

    #[test]
    fn handshake_can_strip_relay_authorization_without_an_injected_header() {
        let mut h = HashMap::new();
        h.insert("upgrade".into(), "websocket".into());
        h.insert("connection".into(), "Upgrade".into());
        h.insert("authorization".into(), "Bearer relay-virtual-key".into());
        h.insert("sec-websocket-key".into(), "abc".into());
        h.insert("cookie".into(), "relay_sid=secret".into());
        h.insert("x-forwarded-for".into(), "203.0.113.1".into());

        let raw = build_handshake("GET", "/v1/realtime", "api.x.ai", &h, &[], true).unwrap();

        assert!(!raw.contains("relay-virtual-key"));
        assert!(!raw.to_ascii_lowercase().contains("authorization:"));
        assert!(!raw.to_ascii_lowercase().contains("cookie:"));
        assert!(!raw.to_ascii_lowercase().contains("x-forwarded-for:"));
        assert!(raw.contains("sec-websocket-key: abc") || raw.contains("Sec-WebSocket-Key: abc"));
    }

    #[test]
    fn handshake_strips_headers_nominated_by_connection() {
        let mut h = HashMap::new();
        h.insert("upgrade".into(), "websocket".into());
        h.insert("connection".into(), "Upgrade, X-Hop, Authorization".into());
        h.insert("x-hop".into(), "must-not-cross".into());
        h.insert("sec-websocket-key".into(), "abc".into());

        let raw = build_handshake(
            "GET",
            "/v1/realtime",
            "api.x.ai",
            &h,
            &[("Authorization".into(), "Bearer injected".into())],
            false,
        )
        .unwrap();
        assert!(!raw.contains("must-not-cross"));
        assert!(raw.contains("Connection: Upgrade"));
        assert!(!raw.contains("Upgrade, X-Hop"));
        assert!(raw.contains("Authorization: Bearer injected"));
    }

    #[test]
    fn endpoint_preserves_path_query_port_and_ipv6_authority() {
        let url = url::Url::parse("http://127.0.0.1:8317/gateway/v1/realtime?model=voice").unwrap();
        let endpoint = UpstreamEndpoint::from_url(&url).unwrap();
        assert!(!endpoint.https);
        assert_eq!(endpoint.connect_host, "127.0.0.1");
        assert_eq!(endpoint.port, 8317);
        assert_eq!(endpoint.authority, "127.0.0.1:8317");
        assert_eq!(endpoint.target, "/gateway/v1/realtime?model=voice");

        let ipv6 = url::Url::parse("http://[::1]:9123/ws").unwrap();
        let endpoint = UpstreamEndpoint::from_url(&ipv6).unwrap();
        assert_eq!(endpoint.connect_host, "::1");
        assert_eq!(endpoint.port, 9123);
        assert_eq!(endpoint.authority, "[::1]:9123");
    }

    #[test]
    fn response_status_is_exact_and_upgrade_headers_are_required() {
        let rejection =
            parse_response_head(b"HTTP/1.1 400 reason mentions 101\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        assert_eq!(rejection.status, 400);
        assert!(!rejection.valid_websocket_upgrade);
        assert_eq!(rejection.framing, ResponseBodyFraming::ContentLength(0));

        let malformed =
            parse_response_head(b"HTTP/1.1 101 Switching Protocols\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        assert_eq!(malformed.status, 101);
        assert!(!malformed.valid_websocket_upgrade);

        let valid = parse_response_head(
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\n\r\n",
        )
        .unwrap();
        assert!(valid.valid_websocket_upgrade);
    }

    #[test]
    fn upstream_response_heads_cannot_mutate_or_relax_the_relay_origin() {
        let upgrade = sanitize_response_head(
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSet-Cookie: relay_sid=stolen\r\nClear-Site-Data: \"cookies\"\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
            false,
        );
        let upgrade = String::from_utf8(upgrade).unwrap();
        assert!(upgrade.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
        assert!(upgrade.contains("Upgrade: websocket\r\n"));
        assert!(upgrade.contains("Connection: Upgrade\r\n"));
        assert!(!upgrade.to_ascii_lowercase().contains("set-cookie"));
        assert!(!upgrade.to_ascii_lowercase().contains("clear-site-data"));
        assert!(!upgrade.to_ascii_lowercase().contains("access-control-"));

        let rejection = sanitize_response_head(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 4\r\nConnection: keep-alive\r\nSet-Cookie: relay_sid=stolen\r\nAccess-Control-Expose-Headers: authorization\r\n\r\n",
            true,
        );
        let rejection = String::from_utf8(rejection).unwrap();
        assert!(rejection.starts_with("HTTP/1.1 403 Forbidden\r\n"));
        assert!(rejection.contains("Content-Length: 4\r\n"));
        assert!(rejection.contains("Connection: close\r\n"));
        assert!(!rejection.to_ascii_lowercase().contains("set-cookie"));
        assert!(!rejection.to_ascii_lowercase().contains("access-control-"));
        assert!(!rejection.contains("Connection: keep-alive"));
    }

    #[tokio::test]
    async fn informational_response_is_consumed_before_final_upgrade() {
        let mut response = &b"HTTP/1.1 103 Early Hints\r\nLink: </voice>\r\n\r\nHTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\nfirst-frame"[..];
        let parsed = read_http_response_head(&mut response).await.unwrap();
        assert_eq!(parsed.parsed.status, 101);
        assert!(parsed.parsed.valid_websocket_upgrade);
        assert!(parsed.head.starts_with(b"HTTP/1.1 101"));
        assert_eq!(parsed.body_prefix, b"first-frame");
    }

    #[test]
    fn injected_headers_replace_client_values_and_reject_line_breaks() {
        let mut headers = HashMap::new();
        headers.insert("connection".into(), "Upgrade".into());
        headers.insert("upgrade".into(), "websocket".into());
        headers.insert("x-grok-client-version".into(), "client-value".into());
        let raw = build_handshake(
            "GET",
            "/v1/realtime",
            "api.x.ai",
            &headers,
            &[("X-Grok-Client-Version".into(), "relay-value".into())],
            false,
        )
        .unwrap();
        assert!(!raw.contains("client-value"));
        assert_eq!(raw.matches("relay-value").count(), 1);
        assert!(build_handshake(
            "GET",
            "/v1/realtime",
            "api.x.ai",
            &headers,
            &[("X-Test".into(), "ok\r\nInjected: yes".into())],
            false,
        )
        .is_err());
        assert!(build_handshake(
            "GET",
            "/v1/realtime",
            "api.x.ai",
            &headers,
            &[("Host".into(), "attacker.invalid".into())],
            false,
        )
        .is_err());
    }

    fn test_endpoint() -> UpstreamEndpoint {
        UpstreamEndpoint {
            https: false,
            connect_host: "127.0.0.1".into(),
            port: 80,
            authority: "upstream.test".into(),
            target: "/v1/realtime".into(),
        }
    }

    fn test_timeouts() -> TunnelTimeouts {
        TunnelTimeouts {
            connect: Duration::from_secs(1),
            tls: Duration::from_secs(1),
            handshake: Duration::from_secs(1),
            rejection_body: Duration::from_secs(1),
        }
    }

    async fn read_request_head<R: AsyncRead + Unpin>(reader: &mut R) {
        let mut bytes = Vec::new();
        let mut byte = [0u8; 1];
        while !bytes.ends_with(b"\r\n\r\n") {
            reader.read_exact(&mut byte).await.unwrap();
            bytes.push(byte[0]);
        }
    }

    #[tokio::test]
    async fn complete_content_length_rejection_is_relayed_with_actual_status() {
        let (relay_client, mut caller) = tokio::io::duplex(4096);
        let (relay_upstream, mut fake_upstream) = tokio::io::duplex(4096);
        let fake = tokio::spawn(async move {
            read_request_head(&mut fake_upstream).await;
            fake_upstream
                .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 11\r\n\r\n")
                .await
                .unwrap();
            tokio::task::yield_now().await;
            fake_upstream.write_all(b"not allowed").await.unwrap();
            fake_upstream.shutdown().await.unwrap();
        });
        let caller_read = tokio::spawn(async move {
            caller.shutdown().await.unwrap();
            let mut response = Vec::new();
            caller.read_to_end(&mut response).await.unwrap();
            response
        });
        let result = pump(
            relay_client,
            relay_upstream,
            "GET",
            &test_endpoint(),
            &HashMap::new(),
            &[],
            false,
            &[],
            test_timeouts(),
        )
        .await
        .unwrap();
        fake.await.unwrap();
        let response = caller_read.await.unwrap();
        assert_eq!(result, (false, 401));
        assert!(response.starts_with(b"HTTP/1.1 401 Unauthorized\r\n"));
        assert!(response
            .windows(b"Connection: close".len())
            .any(|window| window == b"Connection: close"));
        assert!(response.ends_with(b"not allowed"));
    }

    #[tokio::test]
    async fn complete_chunked_rejection_is_relayed() {
        let (relay_client, mut caller) = tokio::io::duplex(4096);
        let (relay_upstream, mut fake_upstream) = tokio::io::duplex(4096);
        let fake = tokio::spawn(async move {
            read_request_head(&mut fake_upstream).await;
            fake_upstream
                .write_all(b"HTTP/1.1 429 Too Many Requests\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nslow\r\n")
                .await
                .unwrap();
            tokio::task::yield_now().await;
            fake_upstream
                .write_all(b"5\r\ndown!\r\n0\r\nX-Reason: quota\r\n\r\n")
                .await
                .unwrap();
            fake_upstream.shutdown().await.unwrap();
        });
        let caller_read = tokio::spawn(async move {
            caller.shutdown().await.unwrap();
            let mut response = Vec::new();
            caller.read_to_end(&mut response).await.unwrap();
            response
        });
        let result = pump(
            relay_client,
            relay_upstream,
            "GET",
            &test_endpoint(),
            &HashMap::new(),
            &[],
            false,
            &[],
            test_timeouts(),
        )
        .await
        .unwrap();
        fake.await.unwrap();
        let response = caller_read.await.unwrap();
        assert_eq!(result, (false, 429));
        assert!(response.ends_with(b"4\r\nslow\r\n5\r\ndown!\r\n0\r\nX-Reason: quota\r\n\r\n"));
    }

    #[tokio::test]
    async fn complete_eof_framed_rejection_is_relayed() {
        let (relay_client, mut caller) = tokio::io::duplex(4096);
        let (relay_upstream, mut fake_upstream) = tokio::io::duplex(4096);
        let fake = tokio::spawn(async move {
            read_request_head(&mut fake_upstream).await;
            fake_upstream
                .write_all(b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\n\r\ntry ")
                .await
                .unwrap();
            tokio::task::yield_now().await;
            fake_upstream.write_all(b"later").await.unwrap();
            fake_upstream.shutdown().await.unwrap();
        });
        let caller_read = tokio::spawn(async move {
            caller.shutdown().await.unwrap();
            let mut response = Vec::new();
            caller.read_to_end(&mut response).await.unwrap();
            response
        });
        let result = pump(
            relay_client,
            relay_upstream,
            "GET",
            &test_endpoint(),
            &HashMap::new(),
            &[],
            false,
            &[],
            test_timeouts(),
        )
        .await
        .unwrap();
        fake.await.unwrap();
        let response = caller_read.await.unwrap();
        assert_eq!(result, (false, 503));
        assert!(response.starts_with(b"HTTP/1.1 503 Service Unavailable\r\n"));
        assert!(response.ends_with(b"try later"));
    }

    #[tokio::test]
    async fn prefetched_client_and_upstream_bytes_survive_upgrade_handoff() {
        let (relay_client, mut caller) = tokio::io::duplex(4096);
        let (relay_upstream, mut fake_upstream) = tokio::io::duplex(4096);
        let fake = tokio::spawn(async move {
            read_request_head(&mut fake_upstream).await;
            fake_upstream
                .write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\nupstream-first")
                .await
                .unwrap();
            let mut initial = [0u8; 12];
            fake_upstream.read_exact(&mut initial).await.unwrap();
            fake_upstream.shutdown().await.unwrap();
            initial
        });
        let caller_read = tokio::spawn(async move {
            caller.shutdown().await.unwrap();
            let mut response = Vec::new();
            caller.read_to_end(&mut response).await.unwrap();
            response
        });
        let result = pump(
            relay_client,
            relay_upstream,
            "GET",
            &test_endpoint(),
            &HashMap::new(),
            &[],
            false,
            b"client-first",
            test_timeouts(),
        )
        .await
        .unwrap();
        let received = fake.await.unwrap();
        let response = caller_read.await.unwrap();
        assert_eq!(result, (true, 101));
        assert_eq!(&received, b"client-first");
        assert!(response.ends_with(b"upstream-first"));
    }

    #[tokio::test]
    async fn handshake_response_has_a_deadline() {
        let (relay_client, _caller) = tokio::io::duplex(4096);
        let (relay_upstream, _silent_upstream) = tokio::io::duplex(4096);
        let mut timeouts = test_timeouts();
        timeouts.handshake = Duration::from_millis(10);
        let error = pump(
            relay_client,
            relay_upstream,
            "GET",
            &test_endpoint(),
            &HashMap::new(),
            &[],
            false,
            &[],
            timeouts,
        )
        .await
        .unwrap_err();
        assert!(!error.response_started);
        assert!(error.to_string().contains("timeout"));
    }

    #[tokio::test]
    async fn established_tunnel_copy_errors_are_propagated() {
        let (relay_client, _caller) = tokio::io::duplex(4096);
        let upstream = FailAfterResponseHead {
            response: b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
            offset: 0,
        };
        let error = pump(
            relay_client,
            upstream,
            "GET",
            &test_endpoint(),
            &HashMap::new(),
            &[],
            false,
            &[],
            test_timeouts(),
        )
        .await
        .unwrap_err();
        assert!(error.response_started);
        assert_eq!(error.status, Some(101));
        assert!(error.to_string().contains("simulated tunnel reset"));
    }
}
