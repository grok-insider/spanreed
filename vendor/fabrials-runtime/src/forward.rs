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
        routed,
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
    let upstream_base = routed.base.trim_end_matches('/');

    let hop_started = std::time::Instant::now();
    let client_http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("fabrials-runtime/0.1")
        .build()
        .map_err(|e| e.to_string())?;

    let mut req = client_http.request(
        method.parse().map_err(|_| format!("bad method {method}"))?,
        upstream_url,
    );
    let provider_headers = inject
        .as_deref()
        .map(|token| prov.credential_headers(token, &routed, secret.as_ref()))
        .transpose()?
        .unwrap_or_default();
    for (k, v) in &headers {
        if is_hop_by_hop_header(k, &headers) {
            continue;
        }
        if is_relay_only_header(k) {
            continue;
        }
        if k.eq_ignore_ascii_case("accept-encoding") {
            continue;
        }
        if strip_http_client_credentials && is_client_credential_header(k) {
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
    if let Some(host) = upstream_base
        .strip_prefix("https://")
        .or_else(|| upstream_base.strip_prefix("http://"))
    {
        req = req.header("Host", host.split('/').next().unwrap_or(host));
    }
    let media_request = if inspect_json_body {
        crate::accounting::summarize_media_request(class.kind, &body)
    } else {
        crate::accounting::MediaRequestSummary::default()
    };
    if !body.is_empty() {
        req = req.body(body);
    }

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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
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
    let body = if (200..300).contains(&status.as_u16()) {
        observer.models_response(captured)
    } else {
        captured
    };
    write_upstream_bytes(client, status, &headers, &body)
}
