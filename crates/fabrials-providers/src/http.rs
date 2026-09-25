//! Injected HTTP for the protocol clients. Hosts supply an [`HttpPort`]; the
//! default-on `reqwest` feature provides [`ReqwestHttp`]. Clients never
//! follow redirects and never read an unbounded body.

use serde_json::Value;

/// Largest body an [`HttpPort`] implementation returns.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json(&self) -> Result<Value, String> {
        serde_json::from_slice(&self.body).map_err(|_| "Response was not JSON".to_string())
    }

    /// The body as JSON, `Err(too_large)` above `limit` bytes.
    pub fn json_within(&self, limit: usize, too_large: &str) -> Result<Value, String> {
        if self.body.len() > limit {
            return Err(too_large.to_string());
        }
        self.json()
    }
}

/// Blocking HTTP with header pairs, returning status and body. Transport
/// failures are `Err`; any HTTP status is `Ok`.
pub trait HttpPort: Send + Sync {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, String>;
    /// `body` is already `application/x-www-form-urlencoded` (see [`form_body`]).
    fn post_form(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<HttpResponse, String>;
    fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &Value,
    ) -> Result<HttpResponse, String>;
}

/// Encode form pairs as `application/x-www-form-urlencoded`.
pub fn form_body(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

/// Every [`HttpPort`] can refresh Grok CLI tokens.
impl<T: HttpPort + ?Sized> crate::grok_cli::TokenHttp for T {
    fn post_form(&self, url: &str, body: &str) -> Result<(u16, String), String> {
        let response = HttpPort::post_form(
            self,
            url,
            &[("Content-Type", "application/x-www-form-urlencoded")],
            body,
        )?;
        Ok((
            response.status,
            String::from_utf8_lossy(&response.body).into_owned(),
        ))
    }
}

/// The [`HttpPort`] hosts get by default: rustls, no redirects, one timeout.
#[cfg(feature = "reqwest")]
pub struct ReqwestHttp {
    client: reqwest::blocking::Client,
}

#[cfg(feature = "reqwest")]
impl ReqwestHttp {
    pub fn new(timeout: std::time::Duration) -> Result<Self, String> {
        reqwest::blocking::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map(|client| Self { client })
            .map_err(|_| "HTTP client unavailable".to_string())
    }

    fn send(
        &self,
        mut request: reqwest::blocking::RequestBuilder,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, String> {
        use std::io::Read;
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request
            .send()
            .map_err(|_| "HTTP request failed".to_string())?;
        let status = response.status().as_u16();
        let mut body = Vec::new();
        response
            .take(MAX_RESPONSE_BYTES as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|_| "HTTP response unreadable".to_string())?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err("HTTP response too large".into());
        }
        Ok(HttpResponse { status, body })
    }
}

#[cfg(feature = "reqwest")]
impl HttpPort for ReqwestHttp {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, String> {
        self.send(self.client.get(url), headers)
    }

    fn post_form(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<HttpResponse, String> {
        let request = self
            .client
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.to_string());
        self.send(request, headers)
    }

    fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &Value,
    ) -> Result<HttpResponse, String> {
        self.send(self.client.post(url).json(body), headers)
    }
}

#[cfg(feature = "reqwest")]
pub(crate) fn default_port(seconds: u64) -> Result<std::sync::Arc<dyn HttpPort>, String> {
    Ok(std::sync::Arc::new(ReqwestHttp::new(
        std::time::Duration::from_secs(seconds),
    )?))
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::sync::Mutex;

    /// `(method, url, headers, body)` of one scripted call.
    pub type Call = (String, String, Vec<(String, String)>, String);

    /// Scripted port: answers in order and records every call.
    #[derive(Default)]
    pub struct ScriptedHttp {
        pub replies: Mutex<Vec<HttpResponse>>,
        pub calls: Mutex<Vec<Call>>,
    }

    impl ScriptedHttp {
        pub fn new(replies: Vec<(u16, Value)>) -> Self {
            Self {
                replies: Mutex::new(
                    replies
                        .into_iter()
                        .rev()
                        .map(|(status, body)| HttpResponse {
                            status,
                            body: body.to_string().into_bytes(),
                        })
                        .collect(),
                ),
                calls: Mutex::default(),
            }
        }

        fn answer(
            &self,
            method: &str,
            url: &str,
            headers: &[(&str, &str)],
            body: String,
        ) -> Result<HttpResponse, String> {
            self.calls.lock().unwrap().push((
                method.into(),
                url.into(),
                headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                body,
            ));
            self.replies
                .lock()
                .unwrap()
                .pop()
                .ok_or_else(|| "no scripted reply".to_string())
        }
    }

    impl HttpPort for ScriptedHttp {
        fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, String> {
            self.answer("GET", url, headers, String::new())
        }
        fn post_form(
            &self,
            url: &str,
            headers: &[(&str, &str)],
            body: &str,
        ) -> Result<HttpResponse, String> {
            self.answer("POST form", url, headers, body.into())
        }
        fn post_json(
            &self,
            url: &str,
            headers: &[(&str, &str)],
            body: &Value,
        ) -> Result<HttpResponse, String> {
            self.answer("POST json", url, headers, body.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_bodies_are_percent_encoded() {
        assert_eq!(
            form_body(&[("a", "b c"), ("scope", "x:y&z")]),
            "a=b+c&scope=x%3Ay%26z"
        );
    }

    #[test]
    fn a_port_refreshes_grok_tokens_through_token_http() {
        use crate::grok_cli::TokenHttp;
        let port = testing::ScriptedHttp::new(vec![(200, serde_json::json!({"ok": true}))]);
        let (status, body) = TokenHttp::post_form(&port, "https://example.invalid", "a=b").unwrap();
        assert_eq!(status, 200);
        assert!(body.contains("ok"));
        let calls = port.calls.lock().unwrap();
        assert_eq!(calls[0].3, "a=b");
    }

    #[test]
    fn oversized_json_is_refused() {
        let response = HttpResponse {
            status: 200,
            body: b"{\"a\":1}".to_vec(),
        };
        assert!(response.json_within(3, "too large").is_err());
        assert_eq!(response.json_within(64, "too large").unwrap()["a"], 1);
    }

    #[test]
    fn protocol_clients_speak_through_the_injected_port() {
        use serde_json::json;
        use std::sync::Arc;

        let port = Arc::new(testing::ScriptedHttp::new(vec![
            (200, json!({"five_hour": {"utilization": 10}})),
            (401, json!({})),
        ]));
        let claude = crate::claude::Client::with_http(port.clone(), Default::default());
        assert!(claude.usage("sk-ant-oat01-token").is_ok());
        assert_eq!(
            claude.profile("sk-ant-oat01-token").unwrap_err(),
            "Claude rejected the credential"
        );
        {
            let calls = port.calls.lock().unwrap();
            assert_eq!(calls[0].0, "GET");
            assert!(calls[0].1.ends_with(crate::claude::USAGE_PATH));
            assert!(calls[0]
                .2
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer sk-ant-oat01-token"));
        }

        let port = Arc::new(testing::ScriptedHttp::new(vec![(
            400,
            json!({"error": "authorization_pending"}),
        )]));
        let nous = crate::nous::Client::with_http(port.clone(), None);
        assert!(matches!(
            nous.poll("device-code", 0).unwrap(),
            crate::device_flow::PollResult::Pending
        ));
        let calls = port.calls.lock().unwrap();
        assert_eq!(calls[0].0, "POST form");
        assert!(calls[0].3.contains("device_code=device-code"));
    }
}
