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
        let path = creds::config_home().join("spanreed").join("config.json");
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
        .user_agent("spanreed/0.1 (+linux)");
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

    pub fn send(self) -> Result<Response, String> {
        let client = client_with(self.insecure).map_err(|e| e.to_string())?;
        let mut req = client.request(self.method, &self.url);
        for (k, v) in self.headers {
            req = req.header(k, v);
        }
        if let Some(body) = self.body {
            req = req.body(body);
        }
        let resp = req.send().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let body = resp.text().map_err(|e| e.to_string())?;
        Ok(Response { status, body })
    }
}
