//! Local composition root for the public Fabrials proxy runtime.
use fabrials_core::hop::{HopKind, Transport};
use fabrials_runtime::{
    accounting, forward, http, listener, provider::Provider, routes, upstreams,
};
use std::net::{SocketAddr, TcpStream};
use std::sync::{atomic::AtomicBool, Arc, Mutex};

pub struct LocalRelay {
    bind: String,
    xai_compat: bool,
    ready: Option<Arc<dyn Fn(SocketAddr) + Send + Sync>>,
    providers: Vec<Arc<dyn Provider>>,
    credentials: Arc<CredentialResolver>,
    usage: Arc<dyn forward::HopObserver + Send + Sync>,
}
type CredentialResolver = dyn Fn(&str, Option<&str>) -> Result<Credential, String> + Send + Sync;

impl LocalRelay {
    pub fn new(bind: &str) -> Result<Self, String> {
        let address: SocketAddr = bind.parse().map_err(|_| "Use a numeric loopback address")?;
        if !address.ip().is_loopback() {
            return Err("Local relay must bind to loopback".into());
        }
        Ok(Self {
            bind: address.to_string(),
            xai_compat: false,
            ready: None,
            providers: vec![
                Arc::new(upstreams::grok::GrokAdapter::default()),
                Arc::new(upstreams::codex::CodexAdapter::default()),
                Arc::new(upstreams::nous::NousAdapter::default()),
                Arc::new(upstreams::openai::OpenAiAdapter::default()),
            ],
            credentials: Arc::new(credential),
            usage: Arc::new(LocalUsage),
        })
    }

    pub(crate) fn with_ready_handler(
        mut self,
        handler: impl Fn(SocketAddr) + Send + Sync + 'static,
    ) -> Self {
        self.ready = Some(Arc::new(handler));
        self
    }

    pub fn serve(self, stop: Arc<AtomicBool>) -> Result<(), String> {
        listener::serve(Arc::new(self), stop, 64, 128 * 1024 * 1024)
    }

    pub fn with_xai_compat(mut self) -> Self {
        self.xai_compat = true;
        self
    }
}

impl listener::ConnectionHost for LocalRelay {
    fn ready(&self, address: SocketAddr) {
        if let Some(handler) = &self.ready {
            handler(address);
        }
    }
    fn bind(&self) -> &str {
        &self.bind
    }
    fn log(&self, message: &str) {
        crate::capture_log::append(message);
    }
    fn classify(&self, path: &str) -> listener::AdmissionClass {
        if routes::is_health_path(path) {
            listener::AdmissionClass::Health
        } else if path.starts_with("/__spanreed/") {
            listener::AdmissionClass::Control
        } else {
            listener::AdmissionClass::Provider
        }
    }
    fn body_reservation(
        &self,
        head: &listener::PreflightHead,
        class: listener::AdmissionClass,
    ) -> usize {
        let Some(parsed) = &head.parsed else {
            return 0;
        };
        if class == listener::AdmissionClass::Health {
            return 0;
        }
        let max = self
            .providers
            .iter()
            .find_map(|p| p.resolve(&parsed.path))
            .map(|r| body_limit(&r.path))
            .unwrap_or(1024 * 1024);
        parsed
            .buffered_body_bytes_hint()
            .unwrap_or(max)
            .min(max)
            .saturating_mul(3)
            .saturating_add(http::MAX_CAPTURE_BYTES * 2)
    }
    fn handle(&self, mut client: TcpStream, _: &Mutex<usize>) -> Result<(), String> {
        client
            .set_write_timeout(Some(std::time::Duration::from_secs(120)))
            .map_err(|e| e.to_string())?;
        let mut reader =
            http::HttpRequestReader::new(client.try_clone().map_err(|e| e.to_string())?);
        let mut head = match http::read_http_request_head(&mut reader) {
            Ok(head) => head,
            Err(error) => {
                return http::write_status(
                    &mut client,
                    error.status_code(),
                    "{\"error\":\"invalid_request\"}",
                )
            }
        };
        if !routes::is_safe_request_target(&head.path) {
            return http::write_status(&mut client, 400, "{\"error\":\"invalid_request_target\"}");
        }
        if head.headers.get("host") != Some(&self.bind) {
            return http::write_status(&mut client, 400, "{\"error\":\"invalid_local_host\"}");
        }
        if routes::is_health_path(&head.path) {
            return if head.method == "GET" {
                http::write_status(&mut client, 200, "{\"ok\":true,\"service\":\"spanreed\"}")
            } else {
                http::write_status(&mut client, 405, "{\"error\":\"method_not_allowed\"}")
            };
        }
        if !origin_allowed(&head.headers, &self.bind) {
            return http::write_status(&mut client, 403, "{\"error\":\"cross_origin_forbidden\"}");
        }
        let control_path = head.path.split('?').next().unwrap_or(&head.path);
        if control_path.starts_with("/__spanreed/") {
            let provider = head
                .path
                .split_once('?')
                .and_then(|(_, query)| {
                    query
                        .split('&')
                        .find_map(|pair| pair.strip_prefix("provider="))
                })
                .unwrap_or("grok");
            let result = match (head.method.as_str(), control_path) {
                ("GET", "/__spanreed/environment") => crate::local_control::environment(&self.bind),
                ("GET", "/__spanreed/limits") => crate::local_control::limits(),
                ("GET", "/__spanreed/autosteer") => crate::local_control::status(provider),
                ("POST", "/__spanreed/autosteer") => {
                    let body = match http::read_http_request_body(&mut reader, &head, 64 * 1024) {
                        Ok(body) => body,
                        Err(error) => {
                            return http::write_status(
                                &mut client,
                                error.status_code(),
                                "{\"error\":\"invalid_body\"}",
                            )
                        }
                    };
                    let body: serde_json::Value = match serde_json::from_slice(&body) {
                        Ok(value) => value,
                        Err(_) => {
                            return http::write_status(&mut client, 400, "{\"error\":\"bad_json\"}")
                        }
                    };
                    let provider = body
                        .get("provider")
                        .and_then(|v| v.as_str())
                        .unwrap_or(provider);
                    let Some(on) = body.get("on").and_then(|v| v.as_bool()) else {
                        return http::write_status(&mut client, 400, "{\"error\":\"invalid_on\"}");
                    };
                    if !crate::local_control::PROVIDERS.contains(&provider) {
                        return http::write_status(
                            &mut client,
                            400,
                            "{\"error\":\"unsupported_provider\"}",
                        );
                    }
                    let threshold = match body.get("exhausted_pct") {
                        None => None,
                        Some(value) => match value
                            .as_f64()
                            .filter(|value| value.is_finite() && *value > 0.0 && *value <= 100.0)
                        {
                            Some(value) => Some(value),
                            None => {
                                return http::write_status(
                                    &mut client,
                                    400,
                                    "{\"error\":\"invalid_exhaustion_threshold\"}",
                                )
                            }
                        },
                    };
                    crate::local_control::set_policy(provider, on, threshold)
                        .and_then(|()| crate::local_control::status(provider))
                }
                (_, "/__spanreed/environment" | "/__spanreed/limits" | "/__spanreed/autosteer") => {
                    return http::write_status(
                        &mut client,
                        405,
                        "{\"error\":\"method_not_allowed\"}",
                    )
                }
                _ => {
                    return http::write_status(
                        &mut client,
                        404,
                        "{\"error\":\"unknown_local_endpoint\"}",
                    )
                }
            };
            return match result {
                Ok(value) => http::write_status(&mut client, 200, &value.to_string()),
                Err(_) => {
                    http::write_status(&mut client, 503, "{\"error\":\"local_state_unavailable\"}")
                }
            };
        }
        if self.xai_compat && head.path.starts_with("/v1/") {
            head.path = format!("/xai{}", head.path);
        }
        let Some((provider, routed)) = self
            .providers
            .iter()
            .find_map(|p| p.resolve(&head.path).map(|route| (p, route)))
        else {
            return http::write_status(&mut client, 404, "{\"error\":\"unknown_route\"}");
        };
        let upgrade =
            fabrials_runtime::ws_tunnel::is_websocket_upgrade(&head.method, &head.headers);
        if !provider.allows_request(&head.method, &routed, upgrade) {
            return http::write_status(
                &mut client,
                404,
                "{\"error\":\"unsupported_provider_route\"}",
            );
        }
        if head
            .headers
            .get("content-encoding")
            .is_some_and(|v| !v.eq_ignore_ascii_case("identity"))
        {
            return http::write_status(
                &mut client,
                415,
                "{\"error\":\"unsupported_content_encoding\"}",
            );
        }
        let class = provider.classify(&routed, upgrade);
        let mut body = match http::read_http_request_body(
            &mut reader,
            &head,
            if class.transport == Transport::WebSocket {
                0
            } else {
                body_limit(&routed.path)
            },
        ) {
            Ok(body) => body,
            Err(error) => {
                return http::write_status(
                    &mut client,
                    error.status_code(),
                    "{\"error\":\"invalid_body\"}",
                )
            }
        };
        if provider.id() == "grok" {
            if let Some(rewritten) = fabrials_runtime::models::rewrite_grok_request_model(&body) {
                body = rewritten;
            }
        }
        let model = match accounting::request_model(&body) {
            Ok(model) => model,
            Err(()) => {
                return http::write_status(&mut client, 400, "{\"error\":\"invalid_model\"}")
            }
        };
        let credential = match (self.credentials)(provider.id(), routed.account_alias.as_deref()) {
            Ok(credential) => credential,
            Err(_) => {
                return http::write_status(
                    &mut client,
                    503,
                    "{\"error\":\"provider_credentials_unavailable\"}",
                )
            }
        };
        let request_id = accounting::new_request_id();
        http::set_request_id(request_id.clone());
        forward::forward(
            forward::AuthorizedHop {
                client,
                reader,
                prov: provider.as_ref(),
                routed,
                method: head.method,
                headers: head.headers,
                body,
                class,
                inject: credential.token,
                alias: credential.alias,
                request_id,
                key_hash: None,
                model,
                inspect_json_body: matches!(
                    class.kind,
                    HopKind::Chat | HopKind::Image | HopKind::Video | HopKind::Tts
                ),
                secret: credential.document,
                strict_credentials: false,
            },
            self.usage.as_ref(),
        )
    }
}

fn body_limit(path: &str) -> usize {
    if path.starts_with("/v1/audio/") || path.starts_with("/v1/stt") {
        http::MAX_BODY_BYTES
    } else {
        8 * 1024 * 1024
    }
}

fn origin_allowed(headers: &std::collections::HashMap<String, String>, bind: &str) -> bool {
    headers
        .get("origin")
        .is_none_or(|origin| origin == &format!("http://{bind}"))
}

pub(crate) fn models_for_account(id: &str) -> Result<Vec<String>, String> {
    let account = crate::accounts::routing_registry()?
        .accounts
        .into_iter()
        .find(|account| account.id == id)
        .ok_or("Account no longer exists")?;
    if account.provider == "codex" {
        crate::drivers::oauth::token("codex", &account.alias)?;
        let doc = crate::accounts::read_secret_document("codex", &account.alias)?
            .ok_or("Codex authorization missing")?;
        let catalog = fabrials_providers::codex::auth::Client::new()?
            .get("/backend-api/codex/models?client_version=0.153.4", &doc)?;
        return fabrials_providers::codex::model_ids(&catalog);
    }
    let (provider, path): (Box<dyn Provider>, &str) = match account.provider.as_str() {
        "grok" => (
            Box::new(upstreams::grok::GrokAdapter::default()),
            "/v1/models",
        ),
        "codex" => (
            Box::new(upstreams::codex::CodexAdapter::default()),
            "/codex/v1/models",
        ),
        "nous" => (
            Box::new(upstreams::nous::NousAdapter::default()),
            "/nous/v1/models",
        ),
        "openai" => (
            Box::new(upstreams::openai::OpenAiAdapter::default()),
            "/openai/v1/models",
        ),
        _ => return Err("Model discovery is unavailable for this provider".into()),
    };
    let upstream = provider.resolve(path).ok_or("Model endpoint unavailable")?;
    let authorization = credential(&account.provider, Some(&account.alias))?;
    let token = authorization
        .token
        .ok_or("Account authorization unavailable")?;
    let mut request = crate::http::Request::get(format!("{}{}", upstream.base, upstream.path))
        .header("Accept", "application/json");
    for (name, value) in
        provider.credential_headers(&token, &upstream, authorization.document.as_ref())?
    {
        request = request.header(&name, &value);
    }
    let response = request
        .send_limited(2 * 1024 * 1024)
        .map_err(|_| "Model catalog request failed")?;
    if !(200..300).contains(&response.status) {
        return Err(format!("Model catalog HTTP {}", response.status));
    }
    fabrials_providers::catalog::model_ids(&response.json().ok_or("Invalid model catalog")?)
}

struct Credential {
    alias: Option<String>,
    token: Option<String>,
    document: Option<serde_json::Value>,
}

fn credential(provider: &str, requested: Option<&str>) -> Result<Credential, String> {
    let registry = crate::accounts::routing_registry()?;
    let account = crate::local_control::select_account(&registry, provider, requested)?;
    if let Some(account) = account {
        let document: serde_json::Value = crate::accounts::get_secret(provider, &account.alias)
            .and_then(|s| serde_json::from_str(&s).ok())
            .ok_or("Account credential unavailable")?;
        let static_key = document
            .get("api_key")
            .or_else(|| document.get("key"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string);
        let token = match static_key {
            Some(key) => key,
            None if provider == "grok" => {
                crate::local_tokens::grok(Some(&account.alias))?.ok_or("Grok login required")?
            }
            None if provider == "codex" => crate::drivers::oauth::token("codex", &account.alias)?,
            None if provider == "nous" => crate::drivers::nous::token(&account.alias)?,
            None => return Err("Provider key missing".into()),
        };
        return Ok(Credential {
            alias: Some(account.alias),
            token: Some(token),
            document: Some(document),
        });
    }
    // Existing clients can continue supplying their own provider credential when
    // no managed account exists. It is only forwarded to the fixed provider origin.
    Ok(Credential {
        alias: None,
        token: if provider == "grok" {
            crate::local_tokens::grok(None)?
        } else {
            None
        },
        document: None,
    })
}

struct LocalUsage;
impl forward::HopObserver for LocalUsage {
    fn authorize_response(
        &self,
        _model: &str,
        provider: &str,
        alias: Option<&str>,
        expected: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let alias = alias.ok_or("Managed account required")?;
        if !crate::accounts::routing_registry()?
            .accounts
            .iter()
            .any(|account| account.provider == provider && account.alias == alias)
            || crate::accounts::read_secret_document(provider, alias)?.as_ref() != expected
        {
            return Err("Account authorization changed; reconnect".into());
        }
        Ok(())
    }

    fn models_response(&self, body: Vec<u8>) -> Vec<u8> {
        fabrials_runtime::models::rewrite_grok_models_body(&body).unwrap_or(body)
    }
    fn log(&self, message: &str) {
        crate::capture_log::append(message);
    }
    fn record(&self, record: fabrials_model::UsageRecord) {
        static LEDGER: Mutex<()> = Mutex::new(());
        let _guard = LEDGER.lock().unwrap_or_else(|e| e.into_inner());
        if crate::grok_ledger::append(&record).is_err() {
            crate::capture_log::append("Local usage persistence failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listener::ConnectionHost;
    use std::io::{Read, Write};

    struct TestUsage(Mutex<Vec<fabrials_model::UsageRecord>>);
    impl forward::HopObserver for TestUsage {
        fn log(&self, _: &str) {}
        fn record(&self, record: fabrials_model::UsageRecord) {
            self.0.lock().unwrap().push(record);
        }
    }

    #[test]
    fn local_host_forwards_and_accounts_without_ai_relay_process() {
        check_forward("nous", false);
    }

    #[test]
    fn compatibility_listener_forwards_bare_v1_to_xai() {
        check_forward("grok", true);
    }

    fn check_forward(provider_id: &'static str, xai_compat: bool) {
        let upstream = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_worker = std::thread::spawn(move || {
            let (mut socket, _) = upstream.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut reader = http::HttpRequestReader::new(socket.try_clone().unwrap());
            let head = http::read_http_request_head(&mut reader).unwrap();
            let body = http::read_http_request_body(&mut reader, &head, 1024).unwrap();
            assert_eq!(head.path, "/v1/responses");
            assert_eq!(
                head.headers.get("authorization").map(String::as_str),
                Some("Bearer fixture-upstream")
            );
            assert!(!head.headers.contains_key("cookie"));
            assert!(!head.headers.contains_key("x-forwarded-for"));
            assert_eq!(body, br#"{"model":"fixture-model"}"#);
            http::write_status(
                &mut socket,
                200,
                r#"{"model":"fixture-model","usage":{"input_tokens":2,"output_tokens":1}}"#,
            )
            .unwrap();
        });
        let mut relay = LocalRelay::new("127.0.0.1:0").unwrap();
        relay.xai_compat = xai_compat;
        relay.providers = if xai_compat {
            vec![Arc::new(upstreams::grok::GrokAdapter {
                cli_base: Some("http://127.0.0.1:1".into()),
                xai_base: Some(format!("http://{upstream_address}")),
            })]
        } else {
            vec![Arc::new(upstreams::nous::NousAdapter {
                base: Some(format!("http://{upstream_address}")),
            })]
        };
        relay.credentials = Arc::new(move |provider, alias| {
            assert_eq!(provider, provider_id);
            assert_eq!(alias, if xai_compat { None } else { Some("work") });
            Ok(Credential {
                alias: Some("canonical".into()),
                token: Some("fixture-upstream".into()),
                document: None,
            })
        });
        let usage = Arc::new(TestUsage(Mutex::new(Vec::new())));
        relay.usage = usage.clone();
        let inbound = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = inbound.local_addr().unwrap();
        relay.bind = address.to_string();
        let worker = std::thread::spawn(move || {
            let (socket, _) = inbound.accept().unwrap();
            relay.handle(socket, &Mutex::new(0)).unwrap();
        });
        let mut client = TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let body = r#"{"model":"fixture-model"}"#;
        let path = if xai_compat {
            "/v1/responses"
        } else {
            "/acct/work/nous/v1/responses"
        };
        write!(client, "POST {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer client-fixture\r\nCookie: fixture-private\r\nX-Forwarded-For: 1.2.3.4\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        worker.join().unwrap();
        upstream_worker.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let records = usage.0.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].account_id,
            Some(format!("{provider_id}/canonical"))
        );
        assert_eq!(records[0].input_tokens, 2);
        assert_eq!(records[0].output_tokens, 1);
    }

    #[test]
    fn bind_and_browser_origin_stay_local() {
        assert!(LocalRelay::new("0.0.0.0:18736").is_err());
        assert!(LocalRelay::new("localhost:18736").is_err());
        assert!(LocalRelay::new("127.0.0.1:18736").is_ok());
        let mut headers = std::collections::HashMap::new();
        assert!(origin_allowed(&headers, "127.0.0.1:18736"));
        headers.insert("origin".into(), "https://attacker.example".into());
        assert!(!origin_allowed(&headers, "127.0.0.1:18736"));
    }
}
