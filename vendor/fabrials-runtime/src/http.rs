//! Parse inbound HTTP and copy upstream bytes to the client.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

pub struct HttpResponse {
    pub code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub content_type: &'static str,
}

thread_local! {
    static REQUEST_ID: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn set_request_id(id: impl Into<String>) {
    REQUEST_ID.with(|c| *c.borrow_mut() = Some(id.into()));
}

fn request_id_header() -> String {
    REQUEST_ID.with(|c| {
        c.borrow()
            .as_ref()
            .map(|id| format!("x-request-id: {id}\r\n"))
            .unwrap_or_default()
    })
}

pub const HOP_BY_HOP: &[&str] = &[
    "connection",
    "expect",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "host",
    "content-length",
];

pub fn is_hop_by_hop_header(name: &str, headers: &HashMap<String, String>) -> bool {
    HOP_BY_HOP.iter().any(|hop| hop.eq_ignore_ascii_case(name))
        || headers
            .get("connection")
            .is_some_and(|value| comma_tokens(value).any(|token| token.eq_ignore_ascii_case(name)))
}

/// Conservative HTTP/1 request limits. The relay buffers inbound bodies today,
/// so these bounds protect both the parser and each hop worker before any large
/// allocation occurs. 64 MiB still accommodates the documented media routes.
pub const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024;
pub const MAX_HEADER_LINE_BYTES: usize = 16 * 1024;
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_HEADER_COUNT: usize = 100;
pub const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
/// Rolling response tail retained for usage parsing and bounded model-list
/// rewriting. Two MiB comfortably contains a terminal SSE event and the
/// validated model catalog while keeping concurrent captures predictable.
pub const MAX_CAPTURE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CHUNK_BYTES: usize = MAX_BODY_BYTES;
pub const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_CHUNK_LINE_BYTES: usize = 256;
const MAX_CHUNK_COUNT: usize = 16 * 1024;
const MAX_CHUNK_FRAMING_BYTES: usize = 1024 * 1024;
const MAX_TRAILER_BYTES: usize = 16 * 1024;
const MAX_TRAILER_COUNT: usize = 32;

/// A buffered connection whose socket timeout is continuously shortened to a
/// single absolute deadline. Trickle traffic therefore cannot reset the clock.
pub struct HttpRequestReader {
    inner: BufReader<TcpStream>,
    deadline: Instant,
}

impl HttpRequestReader {
    pub fn new(stream: TcpStream) -> Self {
        Self::with_timeout(stream, REQUEST_READ_TIMEOUT)
    }

    fn with_timeout(stream: TcpStream, timeout: Duration) -> Self {
        Self {
            inner: BufReader::new(stream),
            deadline: Instant::now() + timeout,
        }
    }

    fn prepare_read(&self) -> io::Result<()> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "request deadline exceeded"))?;
        self.inner.get_ref().set_read_timeout(Some(remaining))
    }
}

impl Read for HttpRequestReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.prepare_read()?;
        self.inner.read(buf)
    }
}

impl BufRead for HttpRequestReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.prepare_read()?;
        self.inner.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyFraming {
    None,
    ContentLength(usize),
    Chunked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequestHead {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    framing: BodyFraming,
}

impl HttpRequestHead {
    /// Exact payload bytes when the framing declares them. `None` means the
    /// payload is chunked, so admission must reserve the route maximum.
    pub fn buffered_body_bytes_hint(&self) -> Option<usize> {
        match self.framing {
            BodyFraming::None => Some(0),
            BodyFraming::ContentLength(bytes) => Some(bytes),
            BodyFraming::Chunked => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpReadErrorKind {
    ConnectionClosed,
    BadRequest,
    RequestTimeout,
    UriTooLong,
    PayloadTooLarge,
    ExpectationFailed,
    HeadersTooLarge,
}

/// A client-safe parse failure. Callers should return `status_code` and
/// `error_code`, but keep the detailed message in local diagnostics only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReadError {
    kind: HttpReadErrorKind,
    detail: String,
}

impl HttpReadError {
    fn new(kind: HttpReadErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    fn from_io(context: &str, error: io::Error) -> Self {
        let kind = if matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ) {
            HttpReadErrorKind::RequestTimeout
        } else {
            HttpReadErrorKind::BadRequest
        };
        Self::new(kind, format!("{context}: {error}"))
    }

    pub fn is_connection_closed(&self) -> bool {
        self.kind == HttpReadErrorKind::ConnectionClosed
    }

    pub fn status_code(&self) -> u16 {
        match self.kind {
            HttpReadErrorKind::ConnectionClosed | HttpReadErrorKind::BadRequest => 400,
            HttpReadErrorKind::RequestTimeout => 408,
            HttpReadErrorKind::UriTooLong => 414,
            HttpReadErrorKind::PayloadTooLarge => 413,
            HttpReadErrorKind::ExpectationFailed => 417,
            HttpReadErrorKind::HeadersTooLarge => 431,
        }
    }

    pub fn error_code(&self) -> &'static str {
        match self.kind {
            HttpReadErrorKind::ConnectionClosed | HttpReadErrorKind::BadRequest => "bad_request",
            HttpReadErrorKind::RequestTimeout => "request_timeout",
            HttpReadErrorKind::UriTooLong => "request_target_too_large",
            HttpReadErrorKind::PayloadTooLarge => "payload_too_large",
            HttpReadErrorKind::ExpectationFailed => "expectation_failed",
            HttpReadErrorKind::HeadersTooLarge => "request_headers_too_large",
        }
    }
}

impl fmt::Display for HttpReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for HttpReadError {}

pub fn write_response(client: &mut TcpStream, resp: HttpResponse) -> Result<(), String> {
    let reason = match resp.code {
        200 => "OK",
        302 => "Found",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        503 => "Unavailable",
        _ => "Error",
    };
    let extra: String = resp
        .headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}\r\n"))
        .collect();
    write!(
        client,
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
        resp.code,
        resp.content_type,
        resp.body.len(),
    )
    .map_err(|e| e.to_string())?;
    client.write_all(&resp.body).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn write_upstream_bytes(
    client: &mut TcpStream,
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
) -> Result<(), String> {
    write!(
        client,
        "HTTP/1.1 {} {}\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    )
    .map_err(|e| e.to_string())?;
    for (k, v) in headers {
        let name = k.as_str();
        if should_drop_upstream_header(headers, name)
            || name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("transfer-encoding")
        {
            continue;
        }
        write!(client, "{name}: {}\r\n", v.to_str().unwrap_or("")).map_err(|e| e.to_string())?;
    }
    write!(
        client,
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .map_err(|e| e.to_string())?;
    client.write_all(body).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn write_status(client: &mut TcpStream, code: u16, body: &str) -> Result<(), String> {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        417 => "Expectation Failed",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Error",
    };
    write!(
        client,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n{body}",
        body.len(),
        request_id_header(),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Copy upstream to the client as HTTP/1.1 chunked. When `capture` is true,
/// also retain the body (chat usage parse). Media hops pass `capture = false`.
#[derive(Debug)]
pub struct PipeUpstreamError {
    message: String,
    /// Bytes already received from the provider before forwarding failed.
    pub captured: Vec<u8>,
}

impl PipeUpstreamError {
    fn without_capture(error: impl ToString) -> Self {
        Self {
            message: error.to_string(),
            captured: Vec::new(),
        }
    }

    fn with_capture(error: impl ToString, captured: &VecDeque<u8>) -> Self {
        Self {
            message: error.to_string(),
            captured: captured.iter().copied().collect(),
        }
    }
}

impl fmt::Display for PipeUpstreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PipeUpstreamError {}

pub fn pipe_upstream(
    client: &mut TcpStream,
    upstream: &mut reqwest::blocking::Response,
    capture: bool,
) -> Result<Vec<u8>, PipeUpstreamError> {
    let status = upstream.status();
    write!(
        client,
        "HTTP/1.1 {} {}\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    )
    .map_err(PipeUpstreamError::without_capture)?;
    for (k, v) in upstream.headers() {
        let name = k.as_str();
        if should_drop_upstream_header(upstream.headers(), name)
            || name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("transfer-encoding")
        {
            continue;
        }
        write!(client, "{name}: {}\r\n", v.to_str().unwrap_or(""))
            .map_err(PipeUpstreamError::without_capture)?;
    }
    write!(client, "{}", request_id_header()).map_err(PipeUpstreamError::without_capture)?;
    write!(
        client,
        "Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
    )
    .map_err(PipeUpstreamError::without_capture)?;
    copy_chunked(client, upstream, capture)
}

/// Copy `src` as HTTP chunks onto `client`. `capture = false` for Imagine/TTS audio.
pub fn copy_chunked(
    client: &mut impl Write,
    src: &mut impl Read,
    capture: bool,
) -> Result<Vec<u8>, PipeUpstreamError> {
    // Only parsing metadata from the tail is required (the completed SSE
    // event is last). A rolling bound prevents arbitrarily large upstream
    // streams from becoming an in-memory copy.
    let mut captured = VecDeque::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) => {
                return Err(PipeUpstreamError::with_capture(
                    format!("upstream response read failed: {error}"),
                    &captured,
                ));
            }
        };
        if capture {
            let overflow = captured
                .len()
                .saturating_add(n)
                .saturating_sub(MAX_CAPTURE_BYTES);
            if overflow > 0 {
                captured.drain(..overflow.min(captured.len()));
            }
            captured.extend(&buf[..n]);
        }
        if let Err(error) = write!(client, "{:x}\r\n", n) {
            return Err(PipeUpstreamError::with_capture(error, &captured));
        }
        if let Err(error) = client.write_all(&buf[..n]) {
            return Err(PipeUpstreamError::with_capture(error, &captured));
        }
        if let Err(error) = client.write_all(b"\r\n") {
            return Err(PipeUpstreamError::with_capture(error, &captured));
        }
    }
    if let Err(error) = client.write_all(b"0\r\n\r\n") {
        return Err(PipeUpstreamError::with_capture(error, &captured));
    }
    Ok(captured.into_iter().collect())
}

pub fn read_http_request_head(
    reader: &mut HttpRequestReader,
) -> Result<HttpRequestHead, HttpReadError> {
    read_http_request_head_with(reader)
}

/// Apply the same strict parser used by the blocking worker to bytes observed
/// with `MSG_PEEK`. Admission uses this only as a memory-sizing hint; the
/// worker still parses the request from the socket before trusting it.
pub fn parse_http_request_head_bytes(bytes: &[u8]) -> Result<HttpRequestHead, HttpReadError> {
    let mut reader = std::io::Cursor::new(bytes);
    read_http_request_head_with(&mut reader)
}

pub fn read_http_request_body(
    reader: &mut HttpRequestReader,
    head: &HttpRequestHead,
    max_body_bytes: usize,
) -> Result<Vec<u8>, HttpReadError> {
    read_http_request_body_with(reader, head, max_body_bytes.min(MAX_BODY_BYTES))
}

fn read_http_request_head_with<R: BufRead>(
    reader: &mut R,
) -> Result<HttpRequestHead, HttpReadError> {
    let first = read_crlf_line(
        reader,
        MAX_REQUEST_LINE_BYTES,
        HttpReadErrorKind::UriTooLong,
        "request line",
        true,
    )?;
    let first = std::str::from_utf8(&first).map_err(|_| {
        HttpReadError::new(HttpReadErrorKind::BadRequest, "request line is not UTF-8")
    })?;
    let mut parts = first.split(' ');
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if method.is_empty()
        || path.is_empty()
        || parts.next().is_some()
        || !is_token(method.as_bytes())
        || version != "HTTP/1.1"
        || method.len() > 32
        || method.bytes().any(|b| b.is_ascii_lowercase())
        || !path.starts_with('/')
        || path.bytes().any(|b| b.is_ascii_control())
    {
        return Err(HttpReadError::new(
            HttpReadErrorKind::BadRequest,
            "malformed request line",
        ));
    }

    let mut headers: HashMap<String, String> = HashMap::new();
    let mut header_bytes = 0usize;
    let mut header_count = 0usize;
    loop {
        let line = read_crlf_line(
            reader,
            MAX_HEADER_LINE_BYTES,
            HttpReadErrorKind::HeadersTooLarge,
            "header",
            false,
        )?;
        header_bytes = header_bytes
            .checked_add(line.len() + 2)
            .ok_or_else(headers_too_large)?;
        if header_bytes > MAX_HEADER_BYTES {
            return Err(headers_too_large());
        }
        if line.is_empty() {
            break;
        }
        header_count += 1;
        if header_count > MAX_HEADER_COUNT {
            return Err(headers_too_large());
        }
        if matches!(line.first(), Some(b' ' | b'\t')) {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                "obsolete folded header",
            ));
        }
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                "header is missing colon",
            ));
        };
        let name = &line[..colon];
        let value = trim_ows(&line[colon + 1..]);
        if !is_token(name) || !valid_header_value(value) {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                "invalid header field",
            ));
        }
        let name = std::str::from_utf8(name)
            .expect("HTTP token is ASCII")
            .to_ascii_lowercase();
        let value = std::str::from_utf8(value).map_err(|_| {
            HttpReadError::new(HttpReadErrorKind::BadRequest, "header value is not UTF-8")
        })?;

        if matches!(
            name.as_str(),
            "content-length" | "transfer-encoding" | "host"
        ) && headers.contains_key(&name)
        {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                format!("duplicate {name}"),
            ));
        }
        match headers.get_mut(&name) {
            Some(existing) => {
                let separator = if name == "cookie" { "; " } else { ", " };
                existing.push_str(separator);
                existing.push_str(value);
            }
            None => {
                headers.insert(name, value.to_string());
            }
        }
    }

    if headers.contains_key("content-length") && headers.contains_key("transfer-encoding") {
        return Err(HttpReadError::new(
            HttpReadErrorKind::BadRequest,
            "content-length conflicts with transfer-encoding",
        ));
    }

    if headers
        .get("connection")
        .is_some_and(|value| !valid_comma_tokens(value))
    {
        return Err(HttpReadError::new(
            HttpReadErrorKind::BadRequest,
            "invalid connection header",
        ));
    }

    let Some(host) = headers.get("host") else {
        return Err(HttpReadError::new(
            HttpReadErrorKind::BadRequest,
            "HTTP/1.1 requires host",
        ));
    };
    if !valid_host(host) {
        return Err(HttpReadError::new(
            HttpReadErrorKind::BadRequest,
            "invalid host header",
        ));
    }
    if headers.contains_key("expect") {
        return Err(HttpReadError::new(
            HttpReadErrorKind::ExpectationFailed,
            "expect is not supported",
        ));
    }

    let content_length = headers
        .get("content-length")
        .map(|value| parse_content_length(value))
        .transpose()?;
    let chunked = match headers.get("transfer-encoding") {
        Some(value) if value.eq_ignore_ascii_case("chunked") => true,
        Some(_) => {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                "unsupported transfer-encoding",
            ));
        }
        None => false,
    };

    let framing = if chunked {
        BodyFraming::Chunked
    } else if let Some(len) = content_length {
        BodyFraming::ContentLength(len)
    } else {
        BodyFraming::None
    };

    Ok(HttpRequestHead {
        method: method.to_string(),
        path: path.to_string(),
        headers,
        framing,
    })
}

fn read_http_request_body_with<R: BufRead>(
    reader: &mut R,
    head: &HttpRequestHead,
    max_body_bytes: usize,
) -> Result<Vec<u8>, HttpReadError> {
    match head.framing {
        BodyFraming::Chunked => read_chunked_body(reader, max_body_bytes),
        BodyFraming::ContentLength(len) => {
            if len > max_body_bytes {
                return Err(payload_too_large());
            }
            let mut body = Vec::new();
            body.try_reserve_exact(len).map_err(|_| {
                HttpReadError::new(HttpReadErrorKind::PayloadTooLarge, "body allocation failed")
            })?;
            body.resize(len, 0);
            if len > 0 {
                reader
                    .read_exact(&mut body)
                    .map_err(|e| HttpReadError::from_io("read body", e))?;
            }
            Ok(body)
        }
        BodyFraming::None => Ok(Vec::new()),
    }
}

fn parse_content_length(value: &str) -> Result<usize, HttpReadError> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(HttpReadError::new(
            HttpReadErrorKind::BadRequest,
            "invalid content-length",
        ));
    }
    let len = value.parse::<u64>().map_err(|_| {
        HttpReadError::new(
            HttpReadErrorKind::PayloadTooLarge,
            "content-length overflow",
        )
    })?;
    if len > MAX_BODY_BYTES as u64 {
        return Err(HttpReadError::new(
            HttpReadErrorKind::PayloadTooLarge,
            "content-length exceeds limit",
        ));
    }
    usize::try_from(len).map_err(|_| {
        HttpReadError::new(
            HttpReadErrorKind::PayloadTooLarge,
            "content-length overflow",
        )
    })
}

fn read_chunked_body<R: BufRead>(
    reader: &mut R,
    max_body_bytes: usize,
) -> Result<Vec<u8>, HttpReadError> {
    let mut body = Vec::new();
    let mut chunk_count = 0usize;
    let mut framing_bytes = 0usize;
    loop {
        let line = read_crlf_line(
            reader,
            MAX_CHUNK_LINE_BYTES,
            HttpReadErrorKind::BadRequest,
            "chunk size",
            false,
        )?;
        framing_bytes = framing_bytes
            .checked_add(line.len() + 4)
            .ok_or_else(payload_too_large)?;
        if framing_bytes > MAX_CHUNK_FRAMING_BYTES {
            return Err(payload_too_large());
        }
        let size_hex = line.split(|b| *b == b';').next().unwrap_or_default();
        if size_hex.is_empty() || !size_hex.iter().all(u8::is_ascii_hexdigit) {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                "invalid chunk size",
            ));
        }
        let extension = &line[size_hex.len()..];
        if extension
            .iter()
            .any(|b| *b < b' ' || *b == 0x7f || !b.is_ascii())
        {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                "invalid chunk extension",
            ));
        }
        let size_hex = std::str::from_utf8(size_hex).expect("hex digits are ASCII");
        let n = u64::from_str_radix(size_hex, 16).map_err(|_| {
            HttpReadError::new(HttpReadErrorKind::PayloadTooLarge, "chunk size overflow")
        })?;
        if n > MAX_CHUNK_BYTES as u64 {
            return Err(HttpReadError::new(
                HttpReadErrorKind::PayloadTooLarge,
                "chunk exceeds limit",
            ));
        }
        let n = usize::try_from(n).map_err(|_| {
            HttpReadError::new(HttpReadErrorKind::PayloadTooLarge, "chunk size overflow")
        })?;
        if n == 0 {
            read_trailers(reader)?;
            break;
        }
        chunk_count += 1;
        if chunk_count > MAX_CHUNK_COUNT {
            return Err(payload_too_large());
        }
        let new_len = body.len().checked_add(n).ok_or_else(payload_too_large)?;
        if new_len > max_body_bytes {
            return Err(payload_too_large());
        }
        body.try_reserve_exact(n).map_err(|_| payload_too_large())?;
        let start = body.len();
        body.resize(new_len, 0);
        reader
            .read_exact(&mut body[start..])
            .map_err(|e| HttpReadError::from_io("read chunk", e))?;
        let mut crlf = [0u8; 2];
        reader
            .read_exact(&mut crlf)
            .map_err(|e| HttpReadError::from_io("read chunk delimiter", e))?;
        if crlf != *b"\r\n" {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                "invalid chunk delimiter",
            ));
        }
    }
    Ok(body)
}

fn read_trailers<R: BufRead>(reader: &mut R) -> Result<(), HttpReadError> {
    let mut bytes = 0usize;
    let mut count = 0usize;
    loop {
        let line = read_crlf_line(
            reader,
            MAX_HEADER_LINE_BYTES,
            HttpReadErrorKind::HeadersTooLarge,
            "trailer",
            false,
        )?;
        bytes = bytes
            .checked_add(line.len() + 2)
            .ok_or_else(headers_too_large)?;
        if bytes > MAX_TRAILER_BYTES {
            return Err(headers_too_large());
        }
        if line.is_empty() {
            return Ok(());
        }
        count += 1;
        if count > MAX_TRAILER_COUNT {
            return Err(headers_too_large());
        }
        if matches!(line.first(), Some(b' ' | b'\t')) {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                "obsolete folded trailer",
            ));
        }
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                "trailer is missing colon",
            ));
        };
        let name = &line[..colon];
        let value = trim_ows(&line[colon + 1..]);
        if !is_token(name) || !valid_header_value(value) {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                "invalid trailer field",
            ));
        }
        if matches_ignore_ascii_case(
            name,
            &["content-length", "transfer-encoding", "host", "trailer"],
        ) {
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                "forbidden trailer field",
            ));
        }
    }
}

fn read_crlf_line<R: BufRead>(
    reader: &mut R,
    max: usize,
    too_large: HttpReadErrorKind,
    context: &str,
    allow_clean_eof: bool,
) -> Result<Vec<u8>, HttpReadError> {
    let mut line = Vec::with_capacity(128.min(max));
    loop {
        let available = reader
            .fill_buf()
            .map_err(|e| HttpReadError::from_io(&format!("read {context}"), e))?;
        if available.is_empty() {
            if allow_clean_eof && line.is_empty() {
                return Err(HttpReadError::new(
                    HttpReadErrorKind::ConnectionClosed,
                    "connection closed before request",
                ));
            }
            return Err(HttpReadError::new(
                HttpReadErrorKind::BadRequest,
                format!("unexpected EOF in {context}"),
            ));
        }
        let take = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |index| index + 1);
        if line.len().saturating_add(take) > max {
            return Err(HttpReadError::new(
                too_large,
                format!("{context} exceeds limit"),
            ));
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if line.last() == Some(&b'\n') {
            if !line.ends_with(b"\r\n") {
                return Err(HttpReadError::new(
                    HttpReadErrorKind::BadRequest,
                    format!("{context} must end with CRLF"),
                ));
            }
            line.truncate(line.len() - 2);
            return Ok(line);
        }
    }
}

fn is_token(value: &[u8]) -> bool {
    !value.is_empty()
        && value.iter().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
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

fn trim_ows(mut value: &[u8]) -> &[u8] {
    while matches!(value.first(), Some(b' ' | b'\t')) {
        value = &value[1..];
    }
    while matches!(value.last(), Some(b' ' | b'\t')) {
        value = &value[..value.len() - 1];
    }
    value
}

fn valid_header_value(value: &[u8]) -> bool {
    value
        .iter()
        .all(|b| *b == b'\t' || (*b >= b' ' && *b != 0x7f))
        && std::str::from_utf8(value).is_ok()
}

fn valid_host(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 512
        || value
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
    {
        return false;
    }

    let (host, port) = if value.starts_with('[') {
        let Some(closing) = value.find(']') else {
            return false;
        };
        let host = &value[..=closing];
        let suffix = &value[closing + 1..];
        let port = if suffix.is_empty() {
            None
        } else {
            let Some(port) = suffix.strip_prefix(':') else {
                return false;
            };
            Some(port)
        };
        (host, port)
    } else {
        match value.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, Some(port)),
            Some(_) => return false,
            None => (value, None),
        }
    };
    url::Host::parse(host).is_ok() && port.is_none_or(valid_port)
}

fn valid_port(port: &str) -> bool {
    !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok()
}

fn comma_tokens(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim)
}

fn valid_comma_tokens(value: &str) -> bool {
    comma_tokens(value).all(|token| is_token(token.as_bytes()))
}

fn should_drop_upstream_header(headers: &reqwest::header::HeaderMap, name: &str) -> bool {
    HOP_BY_HOP.iter().any(|hop| hop.eq_ignore_ascii_case(name))
        // Providers share the relay origin with the operator dashboard. They
        // must not be able to mint/clear relay cookies or opt fabric responses
        // into browser CORS access.
        || matches_ignore_ascii_case(
            name.as_bytes(),
            &["set-cookie", "set-cookie2", "clear-site-data"],
        )
        || name
            .get(.."access-control-".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("access-control-"))
        || headers
            .get_all(reqwest::header::CONNECTION)
            .iter()
            .any(|value| {
                value
                    .as_bytes()
                    .split(|b| *b == b',')
                    .map(trim_ows)
                    .any(|token| token.eq_ignore_ascii_case(name.as_bytes()))
            })
}

fn matches_ignore_ascii_case(value: &[u8], candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| value.eq_ignore_ascii_case(candidate.as_bytes()))
}

fn headers_too_large() -> HttpReadError {
    HttpReadError::new(
        HttpReadErrorKind::HeadersTooLarge,
        "request headers exceed limit",
    )
}

fn payload_too_large() -> HttpReadError {
    HttpReadError::new(
        HttpReadErrorKind::PayloadTooLarge,
        "request body exceeds limit",
    )
}

#[cfg(test)]
mod tests {
    fn process_state() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }

    use super::*;
    use std::io::{BufReader, Cursor, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    type ParsedRequest = (String, String, HashMap<String, String>, Vec<u8>);

    fn parse(raw: impl AsRef<[u8]>) -> Result<ParsedRequest, HttpReadError> {
        let bytes = raw.as_ref();
        let mut reader = BufReader::new(Cursor::new(bytes));
        let head = read_http_request_head_with(&mut reader)?;
        let body = read_http_request_body_with(&mut reader, &head, MAX_BODY_BYTES)?;
        Ok((head.method, head.path, head.headers, body))
    }

    fn assert_parse_status(raw: impl AsRef<[u8]>, status: u16) {
        let error = parse(raw).unwrap_err();
        assert_eq!(error.status_code(), status, "unexpected error: {error}");
    }

    #[test]
    fn media_copy_does_not_retain_body() {
        let payload = vec![7u8; 64 * 1024];
        let mut src = Cursor::new(payload.clone());
        let mut out = Vec::new();
        let captured = copy_chunked(&mut out, &mut src, false).unwrap();
        assert!(captured.is_empty(), "media hop must not retain the body");
        assert!(
            out.len() > payload.len(),
            "client still receives framed bytes"
        );
    }

    #[test]
    fn chunk_copy_propagates_io_failures_without_marking_a_truncated_body_complete() {
        struct BrokenSource(bool);
        impl Read for BrokenSource {
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                if self.0 {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated"));
                }
                self.0 = true;
                output[..3].copy_from_slice(b"abc");
                Ok(3)
            }
        }
        let mut output = Vec::new();
        assert!(copy_chunked(&mut output, &mut BrokenSource(false), true).is_err());
        assert!(!output.ends_with(b"0\r\n\r\n"));

        struct BrokenWriter;
        impl Write for BrokenWriter {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let client_error =
            copy_chunked(&mut BrokenWriter, &mut Cursor::new(b"usage-event"), true).unwrap_err();
        assert_eq!(client_error.captured, b"usage-event");
    }

    #[test]
    fn captured_response_tail_is_bounded() {
        let payload = vec![b'x'; MAX_CAPTURE_BYTES + 32 * 1024];
        let mut output = Vec::new();
        let captured = copy_chunked(&mut output, &mut Cursor::new(payload), true).unwrap();
        assert_eq!(captured.len(), MAX_CAPTURE_BYTES);
    }

    #[test]
    fn reads_chunked_inbound_body() {
        let _process_state = process_state();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let h = thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut r = BufReader::new(s);
            read_chunked_body(&mut r, MAX_BODY_BYTES).unwrap()
        });
        let mut c = TcpStream::connect(addr).unwrap();
        c.write_all(b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n")
            .unwrap();
        drop(c);
        let body = h.join().unwrap();
        assert_eq!(body, b"hello world");
    }

    #[test]
    fn parses_fixed_body_and_canonicalizes_header_names() {
        let (method, path, headers, body) = parse(
            b"POST /v1/responses?q=1 HTTP/1.1\r\nHost: relay.test\r\nX-Trace: one\r\nx-trace: two\r\nContent-Length: 5\r\n\r\nhello",
        )
        .unwrap();
        assert_eq!(method, "POST");
        assert_eq!(path, "/v1/responses?q=1");
        assert_eq!(headers.get("host").map(String::as_str), Some("relay.test"));
        assert_eq!(headers.get("x-trace").map(String::as_str), Some("one, two"));
        assert!(!headers.contains_key("Host"));
        assert_eq!(body, b"hello");
    }

    #[test]
    fn rejects_oversized_or_invalid_content_length_before_reading_body() {
        let boundary = format!(
            "POST / HTTP/1.1\r\nHost: relay.test\r\nContent-Length: {MAX_BODY_BYTES}\r\n\r\n"
        );
        let mut boundary_reader = BufReader::new(Cursor::new(boundary.as_bytes()));
        assert_eq!(
            read_http_request_head_with(&mut boundary_reader)
                .unwrap()
                .framing,
            BodyFraming::ContentLength(MAX_BODY_BYTES)
        );
        assert_eq!(MAX_CHUNK_BYTES, MAX_BODY_BYTES);

        let huge = format!(
            "POST / HTTP/1.1\r\nHost: relay.test\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY_BYTES as u64 + 1
        );
        assert_parse_status(huge, 413);
        assert_parse_status(
            b"POST / HTTP/1.1\r\nHost: relay.test\r\nContent-Length: 184467440737095516160\r\n\r\n",
            413,
        );
        for value in ["+1", "-1", "1, 1", "0x10", ""] {
            let raw =
                format!("POST / HTTP/1.1\r\nHost: relay.test\r\nContent-Length: {value}\r\n\r\n");
            assert_parse_status(raw, 400);
        }
    }

    #[test]
    fn rejects_duplicate_and_conflicting_message_framing() {
        for raw in [
            "POST / HTTP/1.1\r\nHost: relay.test\r\nContent-Length: 0\r\ncontent-length: 0\r\n\r\n",
            "POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: chunked\r\ntRaNsFeR-EnCoDiNg: chunked\r\n\r\n0\r\n\r\n",
            "POST / HTTP/1.1\r\nHost: relay.test\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
            "POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: gzip, chunked\r\n\r\n0\r\n\r\n",
            "POST / HTTP/1.0\r\nHost: relay.test\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
        ] {
            assert_parse_status(raw, 400);
        }
    }

    #[test]
    fn enforces_request_line_and_header_limits() {
        let long_target = format!(
            "GET /{} HTTP/1.1\r\n\r\n",
            "a".repeat(MAX_REQUEST_LINE_BYTES)
        );
        assert_parse_status(long_target, 414);

        let long_header = format!(
            "GET / HTTP/1.1\r\nX-Large: {}\r\n\r\n",
            "a".repeat(MAX_HEADER_LINE_BYTES)
        );
        assert_parse_status(long_header, 431);

        let mut too_many = String::from("GET / HTTP/1.1\r\n");
        for index in 0..=MAX_HEADER_COUNT {
            too_many.push_str(&format!("X-{index}: value\r\n"));
        }
        too_many.push_str("\r\n");
        assert_parse_status(too_many, 431);

        let mut too_many_bytes = String::from("GET / HTTP/1.1\r\n");
        for index in 0..5 {
            too_many_bytes.push_str(&format!("X-{index}: {}\r\n", "a".repeat(14 * 1024)));
        }
        too_many_bytes.push_str("\r\n");
        assert_parse_status(too_many_bytes, 431);
    }

    #[test]
    fn rejects_malformed_crlf_and_header_syntax() {
        for raw in [
            "GET / HTTP/1.1\nHost: relay.test\n\n",
            "GET / HTTP/1.1\r\nBad Header: value\r\n\r\n",
            "GET / HTTP/1.1\r\n Header-Fold: value\r\n\r\n",
            "GET  / HTTP/1.1\r\n\r\n",
            "GET / HTTP/2\r\n\r\n",
        ] {
            assert_parse_status(raw, 400);
        }
    }

    #[test]
    fn requires_unambiguous_http11_host_and_canonical_methods() {
        for raw in [
            "GET / HTTP/1.1\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: one.test\r\nhOsT: two.test\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: user@example.test\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: example.test:99999\r\n\r\n",
            "get / HTTP/1.1\r\nHost: relay.test\r\n\r\n",
            "GET / HTTP/1.0\r\nHost: relay.test\r\n\r\n",
        ] {
            assert_parse_status(raw, 400);
        }

        assert!(parse("GET / HTTP/1.1\r\nHost: [::1]:8317\r\n\r\n").is_ok());
        assert!(parse("M-SEARCH / HTTP/1.1\r\nHost: relay.test\r\n\r\n").is_ok());
    }

    #[test]
    fn strips_connection_nominated_headers() {
        let (_, _, headers, _) = parse(
            "GET / HTTP/1.1\r\nHost: relay.test\r\nConnection: keep-alive, X-Hop\r\nX-Hop: secret\r\nX-End-To-End: keep\r\n\r\n",
        )
        .unwrap();
        assert!(is_hop_by_hop_header("x-hop", &headers));
        assert!(is_hop_by_hop_header("connection", &headers));
        assert!(!is_hop_by_hop_header("x-end-to-end", &headers));
        assert_parse_status(
            "GET / HTTP/1.1\r\nHost: relay.test\r\nConnection: keep-alive,\r\n\r\n",
            400,
        );

        let mut upstream = reqwest::header::HeaderMap::new();
        upstream.insert(reqwest::header::CONNECTION, "x-hop".parse().unwrap());
        upstream.insert("x-hop", "secret".parse().unwrap());
        upstream.insert("set-cookie", "relay_sid=poison".parse().unwrap());
        upstream.insert("clear-site-data", "\"cookies\"".parse().unwrap());
        upstream.insert("access-control-allow-origin", "*".parse().unwrap());
        assert!(should_drop_upstream_header(&upstream, "x-hop"));
        assert!(should_drop_upstream_header(&upstream, "set-cookie"));
        assert!(should_drop_upstream_header(&upstream, "clear-site-data"));
        assert!(should_drop_upstream_header(
            &upstream,
            "access-control-allow-origin"
        ));
    }

    #[test]
    fn route_body_caps_are_checked_before_allocation() {
        let raw = b"POST / HTTP/1.1\r\nHost: relay.test\r\nContent-Length: 5\r\n\r\nhello";
        let mut reader = BufReader::new(Cursor::new(raw));
        let head = read_http_request_head_with(&mut reader).unwrap();
        assert_eq!(
            read_http_request_body_with(&mut reader, &head, 4)
                .unwrap_err()
                .status_code(),
            413
        );

        let raw = b"POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw));
        let head = read_http_request_head_with(&mut reader).unwrap();
        assert_eq!(
            read_http_request_body_with(&mut reader, &head, 4)
                .unwrap_err()
                .status_code(),
            413
        );
    }

    #[test]
    fn validates_chunks_delimiters_sizes_and_trailers() {
        let oversized = format!(
            "POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
            MAX_CHUNK_BYTES as u64 + 1
        );
        assert_parse_status(oversized, 413);

        for raw in [
            "POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhelloXX0\r\n\r\n",
            "POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: chunked\r\n\r\n 5\r\nhello\r\n0\r\n\r\n",
            "POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: chunked\r\n\r\n5\nhello\r\n0\r\n\r\n",
            "POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n",
            "POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nContent-Length: 1\r\n\r\n",
        ] {
            assert_parse_status(raw, 400);
        }

        let (_, _, _, body) = parse(
            b"POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: Chunked\r\n\r\n5;name=value\r\nhello\r\n6\r\n world\r\n0\r\nX-Checksum: yes\r\n\r\n",
        )
        .unwrap();
        assert_eq!(body, b"hello world");
    }

    #[test]
    fn bounds_chunk_count_even_for_small_decoded_bodies() {
        let mut raw = String::from(
            "POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: chunked\r\n\r\n",
        );
        for _ in 0..=MAX_CHUNK_COUNT {
            raw.push_str("1\r\na\r\n");
        }
        raw.push_str("0\r\n\r\n");
        assert_parse_status(raw, 413);

        let extension = "a".repeat(220);
        let chunk = format!("1;{extension}\r\na\r\n");
        let chunks = MAX_CHUNK_FRAMING_BYTES / (extension.len() + 6) + 2;
        let mut raw = String::from(
            "POST / HTTP/1.1\r\nHost: relay.test\r\nTransfer-Encoding: chunked\r\n\r\n",
        );
        for _ in 0..chunks {
            raw.push_str(&chunk);
        }
        raw.push_str("0\r\n\r\n");
        assert_parse_status(raw, 413);
    }

    #[test]
    fn absolute_deadline_stops_a_trickle_client() {
        let _process_state = process_state();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = HttpRequestReader::with_timeout(stream, Duration::from_millis(100));
            ready_tx.send(()).unwrap();
            read_http_request_head(&mut reader)
                .unwrap_err()
                .status_code()
        });

        let mut client = TcpStream::connect(addr).unwrap();
        ready_rx.recv().unwrap();
        for byte in b"GET / HTTP/1.1\r\nHost: relay.test\r\n\r\n" {
            if client.write_all(&[*byte]).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(server.join().unwrap(), 408);
    }

    #[test]
    fn clean_eof_is_not_reported_as_a_malformed_request() {
        let mut reader = BufReader::new(Cursor::new(Vec::<u8>::new()));
        assert!(read_http_request_head_with(&mut reader)
            .unwrap_err()
            .is_connection_closed());
    }

    #[test]
    fn expect_is_rejected_before_reading_a_body() {
        assert_parse_status(
            b"POST / HTTP/1.1\r\nHost: relay.test\r\nContent-Length: 5\r\nExpect: 100-continue\r\n\r\nhello",
            417,
        );
        assert_parse_status(
            b"POST / HTTP/1.1\r\nHost: relay.test\r\nContent-Length: 1\r\nExpect: kittens\r\n\r\nx",
            417,
        );

        let huge = format!(
            "POST / HTTP/1.1\r\nHost: relay.test\r\nContent-Length: {}\r\nExpect: 100-continue\r\n\r\n",
            MAX_BODY_BYTES as u64 + 1
        );
        assert_parse_status(huge, 417);
    }

    #[test]
    fn expect_rejection_does_not_wait_for_a_raw_socket_body() {
        let _process_state = process_state();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = HttpRequestReader::with_timeout(stream, Duration::from_secs(2));
            read_http_request_head(&mut reader)
                .unwrap_err()
                .status_code()
        });

        let mut client = TcpStream::connect(addr).unwrap();
        client
            .write_all(
                b"POST /v1/responses HTTP/1.1\r\nHost: relay.test\r\nContent-Length: 5\r\nExpect: 100-continue\r\n\r\n",
            )
            .unwrap();
        drop(client);
        assert_eq!(server.join().unwrap(), 417);
    }
}
