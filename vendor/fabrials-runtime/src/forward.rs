//! Execute an authorized provider hop. Hosts retain identity and routing policy.
use crate::http::{
    is_hop_by_hop_header, pipe_upstream, write_status, write_upstream_bytes, HttpRequestReader,
};
use crate::provider::{Provider, Upstream};
use fabrials_core::hop::{HopClass, HopKind, Transport};
use fabrials_model::UsageRecord;
use std::collections::HashMap;
use std::io::Read;
use std::net::TcpStream;
use std::time::Duration;

pub trait HopObserver {
    fn record(&self, record: UsageRecord);
    fn log(&self, message: &str);
    fn authorize_response(
        &self,
        _model: &str,
        _provider: &str,
        _alias: Option<&str>,
        _expected: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        Err("WebSocket request authorization unavailable".into())
    }
    fn response_headers(&self, _status: u16, _headers: &reqwest::header::HeaderMap) {}
    fn models_response(&self, body: Vec<u8>) -> Vec<u8> {
        body
    }
    /// Upstream answered with a non-2xx status. `body` is the buffered rejection
    /// body for HTTP hops (bounded, possibly truncated) and empty for tunneled
    /// ones. Hosts classify it here for quota state, logging, and accounting.
    fn upstream_error(&self, _status: u16, _body: &[u8]) {}
    /// Host may return another credential for the same provider after a
    /// rejection, which makes the runtime repeat the exact request once before
    /// anything reaches the client. `None` forwards the rejection unchanged.
    fn retry_credentials(&self, _status: u16, _body: &[u8]) -> Option<RetryCredential> {
        None
    }
}

/// Replacement credential supplied by a host for a rejected hop.
pub struct RetryCredential {
    pub alias: String,
    pub token: String,
    pub secret: Option<serde_json::Value>,
}

/// Rejection bodies are small JSON documents. Anything larger is truncated
/// rather than buffering an arbitrary upstream error stream in memory.
const MAX_REJECTION_BYTES: usize = 64 * 1024;

enum HopAttempt {
    Live(reqwest::blocking::Response),
    Rejected(reqwest::StatusCode, reqwest::header::HeaderMap, Vec<u8>),
}

pub struct WebSocketHop<'a> {
    pub client: &'a mut TcpStream,
    pub headers: &'a HashMap<String, String>,
    pub prefetched: Vec<u8>,
    pub token: Option<&'a str>,
    pub secret: Option<&'a serde_json::Value>,
    pub upstream: &'a Upstream,
    pub observer: &'a dyn HopObserver,
    pub alias: Option<&'a str>,
    pub key_hash: Option<&'a str>,
}

pub struct AuthorizedHop<'a> {
    pub client: TcpStream,
    pub reader: HttpRequestReader,
    pub prov: &'a dyn Provider,
    pub routed: Upstream,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub class: HopClass,
    pub inject: Option<String>,
    pub alias: Option<String>,
    pub request_id: String,
    pub key_hash: Option<String>,
    pub model: Option<String>,
    pub inspect_json_body: bool,
    pub secret: Option<serde_json::Value>,
    pub strict_credentials: bool,
}

pub fn forward(hop: AuthorizedHop<'_>, observer: &dyn HopObserver) -> Result<(), String> {
    let AuthorizedHop {
        mut client,
        mut reader,
        prov,
        mut routed,
        method,
        headers,
        body,
        class,
        inject,
        alias,
        request_id,
        key_hash,
        model,
        inspect_json_body,
        secret,
        strict_credentials,
    } = hop;
    if let Some(token) = inject.as_deref() {
        prov.bind_credential(&mut routed, token);
    }
    // Revalidate at the credential-injection boundary even when the host already checked the route.
    let upstream_url = validated_upstream_url(&routed.base, &routed.path)
        .map_err(|_| "invalid upstream origin".to_string())?;
    let strip_http_client_credentials = strict_credentials || inject.is_some();
    if class.transport == Transport::WebSocket {
        let extra = match inject.as_deref() {
            Some(token) => prov.credential_headers(token, &routed, secret.as_ref())?,
            None => Vec::new(),
        };
        let prefetched_client_bytes = match take_prefetched_client_bytes(&client, &mut reader) {
            Ok(bytes) => bytes,
            Err(error) => {
                observer.log(&format!("websocket handoff failed: {error}"));
                return write_status(&mut client, 500, "{\"error\":\"websocket_handoff_failed\"}");
            }
        };
        if let Some(result) = prov.websocket_hop(WebSocketHop {
            client: &mut client,
            headers: &headers,
            prefetched: prefetched_client_bytes.clone(),
            token: inject.as_deref(),
            secret: secret.as_ref(),
            upstream: &routed,
            observer,
            alias: alias.as_deref(),
            key_hash: key_hash.as_deref(),
        }) {
            return result;
        }
        let _ = client.set_read_timeout(None);
        let _ = client.set_write_timeout(None);
        drop(reader);
        let hop_started = std::time::Instant::now();
        let tunneled = crate::ws_tunnel::tunnel(
            &client,
            &method,
            &upstream_url,
            &headers,
            &extra,
            strict_credentials,
            prefetched_client_bytes,
        );
        let wall = hop_started.elapsed().as_millis() as u64;
        let (status, duration_ms) = match &tunneled {
            Ok(result) => (Some(result.status), Some(result.duration_ms)),
            Err(error) => (Some(error.status.unwrap_or(502)), Some(wall)),
        };
        let mut rec = fabrials_model::UsageRecord {
            ts_ms: now_ms(),
            request_id: Some(request_id.clone()),
            account_id: alias.as_ref().map(|a| format!("{}/{a}", prov.id())),
            route: Some(routed.route.to_string()),
            provider: Some(prov.id().into()),
            key_hash: key_hash.clone(),
            kind: Some(class.kind.as_str().into()),
            duration_ms,
            status,
            model: crate::accounting::model_from_query(&routed.path)
                .or_else(|| Some(class.kind.as_str().into())),
            ..Default::default()
        };
        crate::accounting::apply_media_units(&mut rec, class.kind, class.transport, &[], &[]);
        observer.record(rec);
        // A tunneled handshake carries no body; hosts classify the status alone.
        if let Err(error) = &tunneled {
            if let Some(status) = error.status.filter(|status| *status >= 400) {
                observer.upstream_error(status, &[]);
            }
        }
        return match tunneled {
            Ok(_) => Ok(()),
            Err(error) if !error.response_started => {
                observer.log(&format!("websocket upstream failed: {error}"));
                write_status(
                    &mut client,
                    502,
                    "{\"error\":\"websocket_upstream_unavailable\"}",
                )
            }
            Err(error) => Err(format!("websocket tunnel failed: {error}")),
        };
    }
    if let Some(hop) = prov.translated_hop(
        &method,
        &routed.path,
        &body,
        inject.as_deref().unwrap_or(""),
        secret.as_ref(),
        &mut client,
    ) {
        match hop {
            Ok(usage) => {
                if let Some(mut rec) = usage {
                    rec.request_id = Some(request_id.clone());
                    rec.account_id = alias.as_ref().map(|a| format!("{}/{a}", prov.id()));
                    rec.route = Some(routed.route.to_string());
                    rec.provider = Some(prov.id().into());
                    rec.key_hash = key_hash.clone();
                    observer.record(rec);
                }
                return Ok(());
            }
            Err(e) => {
                let safe = if e.to_ascii_lowercase().contains("bearer")
                    || e.to_ascii_lowercase().contains("sbi_")
                {
                    "upstream error".into()
                } else {
                    e
                };
                return write_status(
                    &mut client,
                    502,
                    &serde_json::json!({"error": "upstream", "detail": safe}).to_string(),
                );
            }
        }
    }
    let hop_started = std::time::Instant::now();
    let client_http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("fabrials-runtime/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let media_request = if inspect_json_body {
        crate::accounting::summarize_media_request(class.kind, &body)
    } else {
        crate::accounting::MediaRequestSummary::default()
    };

    // Chat-only upstreams receive a Chat Completions body. A structured-output
    // retry may replace it; a credential retry repeats the body that is current.
    let mut outbound = crate::wire_compat::adapt_upstream_body(&routed.path, &body);
    let mut format_retried = false;
    let mut inject = inject;
    let mut secret = secret;
    let mut alias = alias;
    let mut attempt = 0u8;
    let attempt_result = loop {
        let upstream_url = validated_upstream_url(&routed.base, &routed.path)
            .map_err(|_| "invalid upstream origin".to_string())?;
        let req = build_upstream_request(
            &client_http,
            &method,
            upstream_url,
            &headers,
            &outbound,
            prov,
            &routed,
            inject.as_deref(),
            secret.as_ref(),
            strip_http_client_credentials,
        )?;
        let mut upstream = match req.send() {
            Ok(r) => r,
            Err(e) => {
                let msg = e.to_string();
                let safe = if msg.to_ascii_lowercase().contains("bearer") {
                    "upstream error".into()
                } else {
                    msg
                };
                let mut rec = fabrials_model::UsageRecord {
                    ts_ms: now_ms(),
                    request_id: Some(request_id.clone()),
                    account_id: alias.as_ref().map(|a| format!("{}/{a}", prov.id())),
                    route: Some(routed.route.to_string()),
                    provider: Some(prov.id().into()),
                    key_hash: key_hash.clone(),
                    kind: Some(class.kind.as_str().into()),
                    duration_ms: Some(hop_started.elapsed().as_millis() as u64),
                    status: Some(502),
                    model: model.clone().or_else(|| Some(class.kind.as_str().into())),
                    ..Default::default()
                };
                crate::accounting::apply_media_units_from_summary(
                    &mut rec,
                    class.kind,
                    class.transport,
                    &media_request,
                    &[],
                );
                observer.record(rec);
                return write_status(
                    &mut client,
                    502,
                    &serde_json::json!({"error": "upstream", "detail": safe}).to_string(),
                );
            }
        };
        if is_grok_models_list(&method, routed.route, &routed.path) {
            return forward_models_list(&mut client, upstream, observer);
        }
        let status = upstream.status();
        if status.is_success() {
            break HopAttempt::Live(upstream);
        }
        let rejected = read_rejection_body(&mut upstream);
        observer.upstream_error(status.as_u16(), &rejected);
        if !format_retried {
            if let Some(next) =
                crate::wire_compat::downgrade_after_rejection(status.as_u16(), &rejected, &outbound)
            {
                format_retried = true;
                outbound = next;
                continue;
            }
        }
        if attempt == 0 {
            if let Some(retry) = observer.retry_credentials(status.as_u16(), &rejected) {
                attempt += 1;
                alias = Some(retry.alias);
                inject = Some(retry.token);
                secret = retry.secret;
                if let Some(token) = inject.as_deref() {
                    prov.bind_credential(&mut routed, token);
                }
                continue;
            }
        }
        let normalized = crate::wire_compat::normalize_rejection(&rejected);
        let headers = crate::wire_compat::json_error_headers(
            upstream.headers().clone(),
            &rejected,
            &normalized,
        );
        break HopAttempt::Rejected(status, headers, normalized);
    };
    let mut upstream = match attempt_result {
        HopAttempt::Live(upstream) => upstream,
        HopAttempt::Rejected(status, headers, rejected) => {
            observer.response_headers(status.as_u16(), &headers);
            return write_upstream_bytes(&mut client, status, &headers, &rejected);
        }
    };
    observer.response_headers(upstream.status().as_u16(), upstream.headers());
    let status = upstream.status();
    let capture = !matches!(class.kind, HopKind::Image | HopKind::Video | HopKind::Tts);
    let (captured, pipe_error) = match pipe_upstream(&mut client, &mut upstream, capture) {
        Ok(captured) => (captured, None),
        Err(error) => {
            let message = error.to_string();
            (error.captured, Some(message))
        }
    };

    let parsed = prov.parse_usage(&captured);
    if class.kind.always_record() || parsed.is_some() || pipe_error.is_some() {
        let mut rec = parsed.unwrap_or_default();
        rec.ts_ms = now_ms();
        if rec.request_id.as_deref().unwrap_or("").is_empty() {
            rec.request_id = Some(request_id.clone());
        }
        rec.account_id = alias.as_ref().map(|a| format!("{}/{a}", prov.id()));
        rec.route = Some(routed.route.to_string());
        rec.provider = Some(prov.id().into());
        rec.key_hash = key_hash.clone();
        rec.kind = Some(class.kind.as_str().into());
        rec.duration_ms = Some(hop_started.elapsed().as_millis() as u64);
        rec.status = Some(status.as_u16());
        if rec.model.is_none() && class.kind.always_record() {
            rec.model = model.clone().or_else(|| Some(class.kind.as_str().into()));
        }
        crate::accounting::apply_media_units_from_summary(
            &mut rec,
            class.kind,
            class.transport,
            &media_request,
            &captured,
        );
        observer.record(rec);
    }
    match pipe_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Build one attempt of the upstream request. Credential headers come from the
/// executing hop, so a reactive retry reuses the client headers and body
/// verbatim while swapping only the injected credential.
#[allow(clippy::too_many_arguments)]
fn build_upstream_request(
    client: &reqwest::blocking::Client,
    method: &str,
    upstream_url: reqwest::Url,
    headers: &HashMap<String, String>,
    body: &[u8],
    prov: &dyn Provider,
    routed: &Upstream,
    token: Option<&str>,
    secret: Option<&serde_json::Value>,
    strip_client_credentials: bool,
) -> Result<reqwest::blocking::RequestBuilder, String> {
    let mut req = client.request(
        method.parse().map_err(|_| format!("bad method {method}"))?,
        upstream_url,
    );
    let provider_headers = token
        .map(|token| prov.credential_headers(token, routed, secret))
        .transpose()?
        .unwrap_or_default();
    for (k, v) in headers {
        if is_hop_by_hop_header(k, headers) {
            continue;
        }
        if is_relay_only_header(k) {
            continue;
        }
        if k.eq_ignore_ascii_case("accept-encoding") {
            continue;
        }
        if strip_client_credentials && is_client_credential_header(k) {
            continue;
        }
        // The models body is rewritten, so a client's cached validator must
        // not short-circuit upstream into 304.
        if is_grok_models_list(method, routed.route, &routed.path)
            && (k.eq_ignore_ascii_case("if-none-match")
                || k.eq_ignore_ascii_case("if-modified-since"))
        {
            continue;
        }
        if provider_headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(k))
        {
            continue;
        }
        req = req.header(k.as_str(), v.as_str());
    }
    for (k, v) in provider_headers {
        if k.eq_ignore_ascii_case("accept-encoding") {
            continue;
        }
        req = req.header(k, v);
    }
    // Captured chat/SSE bodies must stay parseable for accounting and model
    // response rewriting, independent of the caller's compression support.
    req = req.header(reqwest::header::ACCEPT_ENCODING, "identity");
    let base = routed.base.trim_end_matches('/');
    if let Some(host) = base
        .strip_prefix("https://")
        .or_else(|| base.strip_prefix("http://"))
    {
        req = req.header("Host", host.split('/').next().unwrap_or(host));
    }
    if !body.is_empty() {
        req = req.body(body.to_vec());
    }
    Ok(req)
}

/// Buffer a rejected response body for host classification.
fn read_rejection_body(upstream: &mut reqwest::blocking::Response) -> Vec<u8> {
    let mut body = Vec::new();
    let mut limited = std::io::Read::take(&mut *upstream, MAX_REJECTION_BYTES as u64);
    let _ = std::io::Read::read_to_end(&mut limited, &mut body);
    body
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
#[allow(clippy::result_unit_err)]
pub fn validated_upstream_url(base: &str, path: &str) -> Result<reqwest::Url, ()> {
    if !crate::routes::is_safe_request_target(path) {
        return Err(());
    }

    let base = base.trim().trim_end_matches('/');
    let base_url = reqwest::Url::parse(base).map_err(|_| ())?;
    if !matches!(base_url.scheme(), "http" | "https")
        || base_url.host_str().is_none()
        || !base_url.username().is_empty()
        || base_url.password().is_some()
        || base_url.query().is_some()
        || base_url.fragment().is_some()
    {
        return Err(());
    }

    // Parse the exact URL reqwest will receive, then verify the request path
    // did not alter its origin. This is intentionally independent of the
    // route parser so future adapters cannot reintroduce authority confusion.
    let url = reqwest::Url::parse(&format!("{base}{path}")).map_err(|_| ())?;
    let same_origin = url.scheme() == base_url.scheme()
        && url.host_str() == base_url.host_str()
        && url.port_or_known_default() == base_url.port_or_known_default()
        && url.username() == base_url.username()
        && url.password() == base_url.password();
    let request_path = path.split_once('?').map(|(path, _)| path).unwrap_or(path);
    let expected_path = format!("{}{}", base_url.path().trim_end_matches('/'), request_path);
    if !same_origin || url.fragment().is_some() || url.path() != expected_path {
        return Err(());
    }
    Ok(url)
}

pub fn is_client_credential_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("x-xai-token-auth")
}

pub fn is_relay_only_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("cookie")
        || name.eq_ignore_ascii_case("forwarded")
        || name.eq_ignore_ascii_case("x-real-ip")
        || name.eq_ignore_ascii_case("cf-connecting-ip")
        || name.eq_ignore_ascii_case("true-client-ip")
        || name.eq_ignore_ascii_case("referer")
        || name.to_ascii_lowercase().starts_with("x-forwarded-")
}

pub fn take_prefetched_client_bytes(
    client: &TcpStream,
    reader: &mut HttpRequestReader,
) -> Result<Vec<u8>, String> {
    const MAX_PREFETCH: usize = 64 * 1024;
    client
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let mut bytes = vec![0u8; MAX_PREFETCH];
    let read = loop {
        match reader.read(&mut bytes) {
            Ok(read) => break Ok(read),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break Ok(0),
            Err(error) => break Err(error.to_string()),
        }
    };
    let restored = client
        .set_nonblocking(false)
        .map_err(|error| error.to_string());
    let read = read?;
    restored?;
    bytes.truncate(read);
    Ok(bytes)
}

fn is_grok_models_list(method: &str, route: &str, path: &str) -> bool {
    if !method.eq_ignore_ascii_case("GET") {
        return false;
    }
    if route != "xai" && route != "grok" {
        return false;
    }
    let p = path.split('?').next().unwrap_or(path).trim_end_matches('/');
    p == "/v1/models"
}

fn forward_models_list(
    client: &mut TcpStream,
    mut upstream: reqwest::blocking::Response,
    observer: &dyn HopObserver,
) -> Result<(), String> {
    let status = upstream.status();
    let headers = upstream.headers().clone();
    let mut captured = Vec::new();
    let mut limited = std::io::Read::take(&mut upstream, crate::http::MAX_CAPTURE_BYTES as u64 + 1);
    if std::io::Read::read_to_end(&mut limited, &mut captured).is_err() {
        return write_status(client, 502, "{\"error\":\"upstream_read_failed\"}");
    }
    if captured.len() > crate::http::MAX_CAPTURE_BYTES {
        return write_status(client, 502, "{\"error\":\"upstream_response_too_large\"}");
    }
    let rewritten = (200..300).contains(&status.as_u16());
    let body = if rewritten {
        observer.models_response(captured)
    } else {
        captured
    };
    let mut headers = headers;
    if rewritten {
        headers.remove(reqwest::header::ETAG);
        if let Ok(value) = reqwest::header::HeaderValue::from_str(&models_body_etag(&body)) {
            headers.insert(reqwest::header::ETAG, value);
        }
    }
    write_upstream_bytes(client, status, &headers, &body)
}

fn models_body_etag(body: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(body);
    format!("\"{}\"", hex::encode(&digest[..8]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use fabrials_core::hop::HopClass;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct FakeProvider;

    impl Provider for FakeProvider {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn resolve(&self, _raw_path: &str) -> Option<Upstream> {
            None
        }
        fn inject(&self, token: &str) -> Vec<(String, String)> {
            vec![("authorization".into(), format!("Bearer {token}"))]
        }
        fn parse_usage(&self, response_body: &[u8]) -> Option<UsageRecord> {
            let value: serde_json::Value = serde_json::from_slice(response_body).ok()?;
            let usage = value.get("usage")?;
            Some(UsageRecord {
                output_tokens: usage.get("output_tokens")?.as_u64()?,
                ..Default::default()
            })
        }
    }

    #[derive(Default)]
    struct TestObserver {
        offer_retry: bool,
        errors: Mutex<Vec<(u16, String)>>,
        records: Mutex<Vec<UsageRecord>>,
    }

    impl HopObserver for TestObserver {
        fn record(&self, record: UsageRecord) {
            self.records.lock().unwrap().push(record);
        }
        fn log(&self, _: &str) {}
        fn upstream_error(&self, status: u16, body: &[u8]) {
            self.errors
                .lock()
                .unwrap()
                .push((status, String::from_utf8_lossy(body).into_owned()));
        }
        fn retry_credentials(&self, status: u16, _body: &[u8]) -> Option<RetryCredential> {
            if self.offer_retry && status == 402 {
                Some(RetryCredential {
                    alias: "second".into(),
                    token: "tok-second".into(),
                    secret: None,
                })
            } else {
                None
            }
        }
    }

    /// Scripted HTTP/1.1 upstream. Each connection answers with the next
    /// response and records the Authorization header it received.
    fn fake_upstream(
        responses: Vec<(u16, &'static str, String)>,
    ) -> (String, Arc<Mutex<Vec<String>>>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&seen);
        let handle = std::thread::spawn(move || {
            for (status, reason, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut length = 0usize;
                let mut authorization = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 {
                        break;
                    }
                    let trimmed = line.trim_end();
                    if trimmed.is_empty() {
                        break;
                    }
                    let (name, value) = trimmed.split_once(':').unwrap_or((trimmed, ""));
                    if name.eq_ignore_ascii_case("authorization") {
                        authorization = value.trim().to_string();
                    }
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                }
                if length > 0 {
                    let mut request_body = vec![0u8; length];
                    reader.read_exact(&mut request_body).unwrap();
                }
                recorded.lock().unwrap().push(authorization);
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (base, seen, handle)
    }

    /// Accept the relay-side socket and read the whole response, like a client.
    fn fake_client() -> (TcpStream, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut response = Vec::new();
            let _ = stream.read_to_end(&mut response);
            String::from_utf8_lossy(&response).into_owned()
        });
        (TcpStream::connect(address).unwrap(), handle)
    }

    fn run_hop(client: TcpStream, base: String, observer: &TestObserver) -> Result<(), String> {
        let reader = crate::http::HttpRequestReader::new(client.try_clone().unwrap());
        let provider = FakeProvider;
        forward(
            AuthorizedHop {
                client,
                reader,
                prov: &provider,
                routed: Upstream {
                    base,
                    path: "/v1/chat/completions".into(),
                    account_alias: None,
                    route: "grok",
                },
                method: "POST".into(),
                headers: HashMap::from([(
                    "content-type".to_string(),
                    "application/json".to_string(),
                )]),
                body: br#"{"model":"fake-1"}"#.to_vec(),
                class: HopClass::default(),
                inject: Some("tok-first".into()),
                alias: Some("first".into()),
                request_id: "req-retry-test".into(),
                key_hash: Some("key-hash".into()),
                model: Some("fake-1".into()),
                inspect_json_body: true,
                secret: None,
                strict_credentials: true,
            },
            observer,
        )
    }

    #[test]
    fn rejection_retries_once_with_the_host_credential() {
        let (base, seen, upstream) = fake_upstream(vec![
            (
                402,
                "Payment Required",
                r#"{"message":"Grok Build usage balance exhausted"}"#.into(),
            ),
            (200, "OK", r#"{"usage":{"output_tokens":12}}"#.into()),
        ]);
        let (client, response) = fake_client();
        let observer = TestObserver {
            offer_retry: true,
            ..Default::default()
        };

        run_hop(client, base, &observer).unwrap();

        let response = response.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                "Bearer tok-first".to_string(),
                "Bearer tok-second".to_string()
            ]
        );
        let errors = observer.errors.lock().unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, 402);
        assert!(errors[0].1.contains("balance exhausted"));
        drop(errors);
        let records = observer.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].account_id.as_deref(), Some("fake/second"));
        assert_eq!(records[0].status, Some(200));
        upstream.join().unwrap();
    }

    #[test]
    fn rejection_without_a_host_credential_reaches_the_client() {
        let (base, seen, upstream) = fake_upstream(vec![(
            402,
            "Payment Required",
            r#"{"message":"Grok Build usage balance exhausted"}"#.into(),
        )]);
        let (client, response) = fake_client();
        let observer = TestObserver::default();

        run_hop(client, base, &observer).unwrap();

        let response = response.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 402"), "{response}");
        assert!(response.contains("balance exhausted"), "{response}");
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(observer.errors.lock().unwrap().len(), 1);
        assert!(observer.records.lock().unwrap().is_empty());
        upstream.join().unwrap();
    }

    #[test]
    fn successful_hops_stream_without_rejection_hooks() {
        let (base, seen, upstream) =
            fake_upstream(vec![(200, "OK", r#"{"usage":{"output_tokens":7}}"#.into())]);
        let (client, response) = fake_client();
        let observer = TestObserver {
            offer_retry: true,
            ..Default::default()
        };

        run_hop(client, base, &observer).unwrap();

        let response = response.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains(r#""output_tokens":7"#), "{response}");
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert!(observer.errors.lock().unwrap().is_empty());
        let records = observer.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].account_id.as_deref(), Some("fake/first"));
        upstream.join().unwrap();
    }

    fn fake_upstream_bodies(
        responses: Vec<(u16, &'static str, String)>,
    ) -> (
        String,
        Arc<Mutex<Vec<Vec<u8>>>>,
        std::thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&seen);
        let handle = std::thread::spawn(move || {
            for (status, reason, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 {
                        break;
                    }
                    let trimmed = line.trim_end();
                    if trimmed.is_empty() {
                        break;
                    }
                    let (name, value) = trimmed.split_once(':').unwrap_or((trimmed, ""));
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                }
                let mut request_body = vec![0u8; length];
                if length > 0 {
                    reader.read_exact(&mut request_body).unwrap();
                }
                recorded.lock().unwrap().push(request_body);
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (base, seen, handle)
    }

    fn run_posted(path: &str, route: &'static str, body: &[u8], base: String) -> String {
        let (client, response) = fake_client();
        let reader = crate::http::HttpRequestReader::new(client.try_clone().unwrap());
        let provider = FakeProvider;
        let observer = TestObserver::default();
        forward(
            AuthorizedHop {
                client,
                reader,
                prov: &provider,
                routed: Upstream {
                    base,
                    path: path.into(),
                    account_alias: None,
                    route,
                },
                method: "POST".into(),
                headers: HashMap::from([(
                    "content-type".to_string(),
                    "application/json".to_string(),
                )]),
                body: body.to_vec(),
                class: HopClass::default(),
                inject: Some("tok-first".into()),
                alias: Some("first".into()),
                request_id: "req-wire".into(),
                key_hash: None,
                model: Some("fake-1".into()),
                inspect_json_body: true,
                secret: None,
                strict_credentials: true,
            },
            &observer,
        )
        .unwrap();
        response.join().unwrap()
    }

    #[test]
    fn responses_body_on_chat_path_is_sent_as_messages() {
        let (base, seen, upstream) =
            fake_upstream_bodies(vec![(200, "OK", r#"{"usage":{"output_tokens":1}}"#.into())]);
        let response = run_posted(
            "/v1/chat/completions",
            "opencode-go",
            br#"{"model":"deepseek-flash","input":[{"type":"message","role":"user","content":"hi"}],"text":{"format":{"type":"json_schema","name":"structured_output","strict":true,"schema":{"type":"object"}}},"stream":true}"#,
            base,
        );
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let bodies = seen.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        let sent: serde_json::Value = serde_json::from_slice(&bodies[0]).unwrap();
        assert!(sent.get("input").is_none(), "{sent}");
        assert_eq!(sent["messages"][0]["content"], "hi");
        assert_eq!(
            sent["response_format"]["json_schema"]["name"],
            "structured_output"
        );
        assert_eq!(sent["stream"], true);
        drop(bodies);
        upstream.join().unwrap();
    }

    #[test]
    fn grok_responses_hop_keeps_input() {
        let (base, seen, upstream) =
            fake_upstream_bodies(vec![(200, "OK", r#"{"usage":{"output_tokens":1}}"#.into())]);
        let raw = br#"{"model":"grok-4.7","input":"hi"}"#;
        let response = run_posted("/v1/responses", "grok", raw, base);
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let bodies = seen.lock().unwrap();
        assert_eq!(bodies[0].as_slice(), raw);
        drop(bodies);
        upstream.join().unwrap();
    }

    #[test]
    fn json_schema_html_400_retries_as_json_object() {
        let (base, seen, upstream) = fake_upstream_bodies(vec![
            (400, "Bad Request", "<html>nope</html>".into()),
            (200, "OK", r#"{"usage":{"output_tokens":4}}"#.into()),
        ]);
        let response = run_posted(
            "/v1/chat/completions",
            "opencode-go",
            br#"{"model":"deepseek-flash","messages":[{"role":"user","content":"goal"}],"response_format":{"type":"json_schema","json_schema":{"name":"structured_output","strict":true,"schema":{"type":"object","required":["decision"]}}},"stream":true}"#,
            base,
        );
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let bodies = seen.lock().unwrap();
        assert_eq!(bodies.len(), 2, "schema rejection must be retried once");
        let second: serde_json::Value = serde_json::from_slice(&bodies[1]).unwrap();
        assert_eq!(second["response_format"]["type"], "json_object");
        let system = second["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("json"), "{system}");
        assert!(system.contains("decision"), "{system}");
        drop(bodies);
        upstream.join().unwrap();
    }

    #[test]
    fn context_length_400_is_not_downgraded() {
        let (base, seen, upstream) = fake_upstream_bodies(vec![(
            400,
            "Bad Request",
            r#"{"error":{"message":"prompt is too long for this model's context window","type":"invalid_request_error"}}"#
                .into(),
        )]);
        let response = run_posted(
            "/v1/chat/completions",
            "opencode-go",
            br#"{"model":"deepseek-flash","messages":[{"role":"user","content":"goal"}],"response_format":{"type":"json_schema","json_schema":{"schema":{"type":"object"}}}}"#,
            base,
        );
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
        assert!(response.contains("context window"), "{response}");
        assert_eq!(seen.lock().unwrap().len(), 1);
        upstream.join().unwrap();
    }

    #[test]
    fn opaque_400_without_schema_is_a_readable_error() {
        let (base, seen, upstream) =
            fake_upstream_bodies(vec![(400, "Bad Request", "<html>blocked</html>".into())]);
        let response = run_posted(
            "/v1/chat/completions",
            "opencode-go",
            br#"{"model":"deepseek-flash","messages":[{"role":"user","content":"hi"}]}"#,
            base,
        );
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
        assert!(
            response.contains("upstream rejected the request"),
            "{response}"
        );
        assert!(!response.contains("<html>"), "{response}");
        assert!(response.contains("application/json"), "{response}");
        assert_eq!(seen.lock().unwrap().len(), 1);
        upstream.join().unwrap();
    }
}
