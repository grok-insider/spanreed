//! Thin blocking HTTP client wrapper shared by providers.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use crate::creds;

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

/// Binary HTTP response (e.g. release assets).
#[derive(Debug)]
pub struct BytesResponse {
    pub status: u16,
    pub headers: reqwest::header::HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_str(self.body.trim()).ok()
    }

    pub fn is_auth_error(&self) -> bool {
        self.status == 401 || self.status == 403
    }
}

/// Optional proxy resolved once from `~/.config/spanreed/config.json`:
/// `{ "proxy": { "enabled": true, "url": "socks5://127.0.0.1:9050" } }`
fn resolved_proxy() -> &'static Option<reqwest::Proxy> {
    static PROXY: OnceLock<Option<reqwest::Proxy>> = OnceLock::new();
    PROXY.get_or_init(|| {
        let path = crate::app::config_dir().join("config.json");
        let cfg = creds::read_json(&path)?;
        let proxy = cfg.get("proxy")?;
        if !proxy
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return None;
        }
        let url = proxy.get("url").and_then(|v| v.as_str())?;
        match reqwest::Proxy::all(url) {
            Ok(p) => {
                let no_proxy = reqwest::NoProxy::from_string("localhost,127.0.0.1,::1");
                Some(p.no_proxy(no_proxy))
            }
            Err(e) => {
                log::warn!("invalid proxy url, ignoring: {e}");
                None
            }
        }
    })
}

fn client_with(insecure: bool) -> reqwest::Result<reqwest::blocking::Client> {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        // OS tag follows the build target (linux/macos/windows/...).
        .user_agent(crate::app::user_agent());
    if let Some(proxy) = resolved_proxy() {
        builder = builder.proxy(proxy.clone());
    }
    if insecure {
        builder = builder.danger_accept_invalid_certs(true);
    }
    builder.build()
}

pub struct Request {
    method: reqwest::Method,
    url: String,
    headers: HashMap<String, String>,
    body: Option<String>,
    insecure: bool,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Self::new(reqwest::Method::GET, url)
    }
    pub fn post(url: impl Into<String>) -> Self {
        Self::new(reqwest::Method::POST, url)
    }
    pub fn put(url: impl Into<String>) -> Self {
        Self::new(reqwest::Method::PUT, url)
    }
    fn new(method: reqwest::Method, url: impl Into<String>) -> Self {
        Request {
            method,
            url: url.into(),
            headers: HashMap::new(),
            body: None,
            insecure: false,
        }
    }

    /// Accept invalid/self-signed TLS certs (for local language-server probes).
    pub fn insecure(mut self) -> Self {
        self.insecure = true;
        self
    }
    pub fn header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.headers.insert(k.into(), v.into());
        self
    }
    pub fn bearer(self, token: &str) -> Self {
        self.header("Authorization", format!("Bearer {token}"))
    }
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    fn execute(self) -> Result<reqwest::blocking::Response, String> {
        let client = client_with(self.insecure).map_err(|e| e.to_string())?;
        let mut req = client.request(self.method, &self.url);
        for (k, v) in self.headers {
            req = req.header(k, v);
        }
        if let Some(body) = self.body {
            req = req.body(body);
        }
        req.send().map_err(|e| e.to_string())
    }

    pub fn send(self) -> Result<Response, String> {
        let resp = self.execute()?;
        let status = resp.status().as_u16();
        let body = resp.text().map_err(|e| e.to_string())?;
        Ok(Response { status, body })
    }

    pub fn send_limited(self, limit: usize) -> Result<Response, String> {
        use std::io::Read;
        let resp = self.execute()?;
        let status = resp.status().as_u16();
        let mut bytes = Vec::new();
        resp.take(limit.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "Response read failed")?;
        if bytes.len() > limit {
            return Err("Response exceeds size limit".into());
        }
        let body = String::from_utf8(bytes).map_err(|_| "Invalid response text")?;
        Ok(Response { status, body })
    }

    /// Bounded provider response using the normal timeout and redirect policy.
    pub fn send_bytes_limited(self, limit: usize) -> Result<BytesResponse, String> {
        use std::io::Read;
        let resp = self.execute()?;
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        let mut body = Vec::new();
        resp.take(limit.saturating_add(1) as u64)
            .read_to_end(&mut body)
            .map_err(|_| "Response read failed")?;
        if body.len() > limit {
            return Err("Response exceeds size limit".into());
        }
        Ok(BytesResponse {
            status,
            headers,
            body,
        })
    }

    /// Download response body as raw bytes (release archives, etc.).
    pub fn send_bytes(self) -> Result<BytesResponse, String> {
        // Longer timeout for multi-MB release assets.
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent(crate::app::user_agent());
        if let Some(proxy) = resolved_proxy() {
            builder = builder.proxy(proxy.clone());
        }
        if self.insecure {
            builder = builder.danger_accept_invalid_certs(true);
        }
        let client = builder.build().map_err(|e| e.to_string())?;
        let mut req = client.request(self.method, &self.url);
        for (k, v) in self.headers {
            req = req.header(k, v);
        }
        if let Some(body) = self.body {
            req = req.body(body);
        }
        let resp = req.send().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        let body = resp.bytes().map_err(|e| e.to_string())?.to_vec();
        Ok(BytesResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_binary_response_preserves_status_without_following_redirects() {
        use std::io::{Read, Write};
        for (status, headers, body, limit, expected) in [
            ("200 OK", "grpc-status: 16\r\n", "abc", 3, Ok(200)),
            ("200 OK", "", "abc", 2, Err("Response exceeds size limit")),
            (
                "302 Found",
                "Location: http://127.0.0.1:1/\r\n",
                "",
                3,
                Ok(302),
            ),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    assert!(request.len() < 4096);
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                write!(stream, "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            });
            let result = Request::get(format!("http://{address}/resets")).send_bytes_limited(limit);
            match expected {
                Ok(status) => {
                    let response = result.unwrap();
                    assert_eq!(response.status, status);
                    assert_eq!(response.body, body.as_bytes());
                    if headers.starts_with("grpc-status") {
                        assert_eq!(response.headers["grpc-status"], "16");
                    }
                }
                Err(error) => assert_eq!(result.unwrap_err(), error),
            }
            server.join().unwrap();
        }
    }

    #[test]
    fn bounded_response_rejects_chunked_body_over_limit() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len() < 4096);
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n0\r\n\r\n").unwrap();
        });
        let error = Request::get(format!("http://{address}/models"))
            .send_limited(2)
            .unwrap_err();
        assert_eq!(error, "Response exceeds size limit");
        server.join().unwrap();
    }
}
