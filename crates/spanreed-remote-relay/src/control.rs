use crate::routing::Identity;
use reqwest::{Client, Url};
use serde::{Deserialize, de::DeserializeOwned};
use std::time::Duration;

#[derive(Clone)]
pub struct ControlPlane {
    client: Client,
    base: Url,
    secret: String,
}

impl ControlPlane {
    #[cfg(test)]
    pub(crate) fn loopback_fixture(base: &str, secret: String) -> Result<Self, &'static str> {
        let base = Url::parse(base).map_err(|_| "invalid fixture URL")?;
        if base.scheme() != "http"
            || base.host_str() != Some("127.0.0.1")
            || base.port().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
            || secret.len() < 32
        {
            return Err("fixture control plane must be explicit loopback with a service secret");
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| "could not create fixture client")?;
        Ok(Self {
            client,
            base,
            secret,
        })
    }

    pub fn new(base: &str, secret: String) -> Result<Self, &'static str> {
        let mut base = Url::parse(base).map_err(|_| "invalid control-plane URL")?;
        if base.scheme() != "https"
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err("control-plane URL must use HTTPS without credentials or query");
        }
        if secret.len() < 32 {
            return Err("relay service secret must have at least 32 bytes");
        }
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let client = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| "could not create control-plane client")?;
        Ok(Self {
            client,
            base,
            secret,
        })
    }
    async fn post<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        body: serde_json::Value,
        timeout: Duration,
    ) -> Result<T, ()> {
        let url = self.base.join(endpoint).map_err(|_| ())?;
        let mut response = self
            .client
            .post(url)
            .timeout(timeout)
            .bearer_auth(&self.secret)
            .json(&body)
            .send()
            .await
            .map_err(|_| ())?;
        if !response.status().is_success()
            || response.content_length().is_some_and(|size| size > 16_384)
        {
            return Err(());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
            if bytes.len() + chunk.len() > 16_384 {
                return Err(());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| ())
    }
    pub async fn consume(&self, ticket: &str) -> Result<Identity, ()> {
        if ticket.is_empty() || ticket.len() > 128 {
            return Err(());
        }
        let identity: Identity = self
            .post(
                "api/spanreed/remote/tickets/consume",
                serde_json::json!({"ticket":ticket}),
                Duration::from_secs(5),
            )
            .await?;
        if identity.account_id.is_empty()
            || identity.account_id.len() > 128
            || identity.host_id.is_empty()
            || identity.host_id.len() > 128
            || identity.generation == 0
        {
            return Err(());
        }
        Ok(identity)
    }
    pub async fn active(&self, identity: &Identity) -> bool {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Active {
            active: bool,
        }
        self.post::<Active>("api/spanreed/remote/tickets/introspect", serde_json::json!({
            "accountId":identity.account_id,"hostId":identity.host_id,"generation":identity.generation,
        }), Duration::from_secs(5)).await.is_ok_and(|value| value.active)
    }

    pub async fn ready(&self) -> bool {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Ready {
            ready: bool,
        }
        self.post::<Ready>(
            "api/spanreed/remote/ready",
            serde_json::json!({}),
            Duration::from_secs(3),
        )
        .await
        .is_ok_and(|value| value.ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn readiness_requires_authenticated_strict_success() {
        for (status, body, expected) in [
            (200, "{\"ready\":true}", true),
            (200, "{\"ready\":false}", false),
            (200, "{\"ready\":true,\"unknown\":1}", false),
            (200, "{}", false),
            (503, "{\"ready\":true}", false),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut buffer = [0; 1024];
                    let size = socket.read(&mut buffer).await.unwrap();
                    assert!(size > 0 && request.len() + size <= 4096);
                    request.extend_from_slice(&buffer[..size]);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.starts_with("POST /api/spanreed/remote/ready HTTP/1.1"));
                assert!(
                    request
                        .to_ascii_lowercase()
                        .contains(&format!("authorization: bearer {}", "x".repeat(32)))
                );
                let response = format!(
                    "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            });
            let client =
                ControlPlane::loopback_fixture(&format!("http://{address}"), "x".repeat(32))
                    .unwrap();
            assert_eq!(client.ready().await, expected);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn readiness_timeout_is_shorter_than_poll_interval() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
            drop(socket);
        });
        let client =
            ControlPlane::loopback_fixture(&format!("http://{address}"), "x".repeat(32)).unwrap();
        let start = std::time::Instant::now();
        assert!(!client.ready().await);
        assert!(start.elapsed() >= Duration::from_secs(2));
        assert!(start.elapsed() < Duration::from_secs(5));
        server.abort();
        let _ = server.await;
    }
    #[test]
    fn fixture_is_restricted_to_an_explicit_loopback_listener() {
        for base in [
            "http://127.0.0.1",
            "http://localhost:3000",
            "http://example.com:3000",
            "http://127.0.0.1:3000/path",
            "http://user:secret@127.0.0.1:3000",
            "http://127.0.0.1:3000?secret=x",
            "http://127.0.0.1:3000#secret",
            "https://127.0.0.1:3000",
        ] {
            assert!(ControlPlane::loopback_fixture(base, "x".repeat(32)).is_err());
        }
        assert!(ControlPlane::loopback_fixture("http://127.0.0.1:3000", "short".into()).is_err());
        assert!(ControlPlane::loopback_fixture("http://127.0.0.1:3000", "x".repeat(32)).is_ok());
    }

    #[test]
    fn rejects_http_credentials_and_weak_secret() {
        for base in [
            "http://fabrials.com",
            "https://user:secret@fabrials.com",
            "https://fabrials.com?token=secret",
            "https://fabrials.com#secret",
        ] {
            assert!(ControlPlane::new(base, "x".repeat(32)).is_err());
        }
        assert!(ControlPlane::new("https://fabrials.com", "short".into()).is_err());
        assert!(ControlPlane::new("https://fabrials.com", "x".repeat(32)).is_ok());
    }
}
