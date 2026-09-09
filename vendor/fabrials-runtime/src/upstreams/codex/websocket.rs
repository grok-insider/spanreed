//! Frame-aware Responses transport. Authorization and accounting run per creation.
use super::CodexAdapter;
use crate::{forward::WebSocketHop, provider::Provider};
use fabrials_model::UsageRecord;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::io::Write;
use tokio_tungstenite::{
    tungstenite::{
        client::IntoClientRequest,
        protocol::{Role, WebSocketConfig},
        Message,
    },
    WebSocketStream,
};

pub fn forward(adapter: &CodexAdapter, mut hop: WebSocketHop<'_>) -> Result<(), String> {
    let key = hop.headers.get("sec-websocket-key").filter(|key| {
        key.len() == 24
            && key.ends_with("==")
            && key[..22]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/".contains(&b))
    });
    if key.is_none() || hop.headers.get("sec-websocket-version").map(String::as_str) != Some("13") {
        return crate::http::write_status(
            hop.client,
            400,
            "{\"error\":\"invalid_websocket_handshake\"}",
        );
    }
    let token = hop.token.ok_or("Codex authorization unavailable")?;
    let mut url = crate::forward::validated_upstream_url(&hop.upstream.base, &hop.upstream.path)
        .map_err(|_| "Invalid Codex WebSocket origin")?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme)
        .map_err(|_| "Invalid WebSocket origin")?;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| "Invalid WebSocket request")?;
    for (name, value) in adapter.credential_headers(token, hop.upstream, hop.secret)? {
        request.headers_mut().insert(
            name.parse::<tokio_tungstenite::tungstenite::http::HeaderName>()
                .map_err(|_| "Invalid provider header")?,
            value.parse().map_err(|_| "Invalid provider header")?,
        );
    }
    if let Some(beta) = hop
        .headers
        .get("openai-beta")
        .filter(|value| value.len() <= 256)
    {
        request.headers_mut().insert(
            "openai-beta",
            beta.parse().map_err(|_| "Invalid beta header")?,
        );
    }
    let accept =
        tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.unwrap().as_bytes());
    let result = match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.block_on(run(adapter, &mut hop, request, &accept)),
        Err(_) => tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|_| "WebSocket runtime unavailable")?
            .block_on(run(adapter, &mut hop, request, &accept)),
    };
    let _ = hop.client.set_nonblocking(false);
    result
}

struct Active<'a> {
    bytes: usize,
    record: Option<UsageRecord>,
    started: std::time::Instant,
    observer: &'a dyn crate::forward::HopObserver,
}
impl Active<'_> {
    fn finish(&mut self, adapter: &CodexAdapter, event: &Value, status: u16) {
        if let Some(mut record) = self.record.take() {
            if let Some(usage) = adapter.parse_usage(event.to_string().as_bytes()) {
                record.input_tokens = usage.input_tokens;
                record.output_tokens = usage.output_tokens;
                record.cached_input_tokens = usage.cached_input_tokens;
                record.total_tokens = usage.total_tokens;
            }
            record.status = Some(status);
            record.duration_ms = Some(self.started.elapsed().as_millis() as u64);
            tokio::task::block_in_place(|| self.observer.record(record));
        }
    }
}
impl Drop for Active<'_> {
    fn drop(&mut self) {
        if let Some(mut record) = self.record.take() {
            record.status = Some(502);
            record.duration_ms = Some(self.started.elapsed().as_millis() as u64);
            tokio::task::block_in_place(|| self.observer.record(record));
        }
    }
}

async fn run(
    adapter: &CodexAdapter,
    hop: &mut WebSocketHop<'_>,
    request: tokio_tungstenite::tungstenite::http::Request<()>,
    accept: &str,
) -> Result<(), String> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(8 * 1024 * 1024))
        .max_frame_size(Some(8 * 1024 * 1024));
    let connected = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio_tungstenite::connect_async_with_config(request, Some(config), true),
    )
    .await;
    let (mut upstream, _) = match connected {
        Ok(Ok(value)) => value,
        _ => {
            return crate::http::write_status(
                hop.client,
                502,
                "{\"error\":\"codex_websocket_upstream_unavailable\"}",
            )
        }
    };
    write!(hop.client,"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").map_err(|_|"Client disconnected")?;
    hop.client
        .set_nonblocking(true)
        .map_err(|_| "WebSocket socket unavailable")?;
    let socket = tokio::net::TcpStream::from_std(
        hop.client
            .try_clone()
            .map_err(|_| "WebSocket socket unavailable")?,
    )
    .map_err(|_| "WebSocket socket unavailable")?;
    let mut client = WebSocketStream::from_partially_read(
        socket,
        std::mem::take(&mut hop.prefetched),
        Role::Server,
        Some(config),
    )
    .await;
    let mut active = Active {
        bytes: 0,
        record: None,
        started: std::time::Instant::now(),
        observer: hop.observer,
    };
    loop {
        let timeout = if active.record.is_some() {
            std::time::Duration::from_secs(600).saturating_sub(active.started.elapsed())
        } else {
            std::time::Duration::from_secs(600)
        };
        tokio::select! {
            _=tokio::time::sleep(timeout)=>return Err("Codex WebSocket idle timeout".into()),
            message=client.next()=>{
                let Some(Ok(message))=message else {return Ok(());};
                match message {
                    Message::Text(text)=>{
                        let mut event:Value=match serde_json::from_str(&text){Ok(value)=>value,Err(_)=>{client.send(error("invalid_json")).await.map_err(|_|"Client disconnected")?;continue;}};
                        if event["type"]!="response.create" {client.send(error("unsupported_event_type")).await.map_err(|_|"Client disconnected")?;continue;}
                        if active.record.is_some() {client.send(error("response_in_progress")).await.map_err(|_|"Client disconnected")?;continue;}
                        let Some(model)=event["model"].as_str().filter(|model|!model.is_empty() && model.len()<=256 && model.bytes().all(|b|b.is_ascii_graphic())) else {client.send(error("invalid_model")).await.map_err(|_|"Client disconnected")?;continue;};
                        if tokio::task::block_in_place(|| hop.observer.authorize_response(model,"codex",hop.alias,hop.secret)).is_err(){client.send(error("request_not_authorized")).await.map_err(|_|"Client disconnected")?;continue;}
                        let model=model.to_owned();
                        event["store"]=json!(false);
                        active.bytes=0;active.started=std::time::Instant::now();
                        active.record=Some(UsageRecord {ts_ms:super::translation::now_ms(),request_id:Some(crate::accounting::new_request_id()),provider:Some("codex".into()),route:Some("codex".into()),kind:Some("chat".into()),model:Some(model),account_id:hop.alias.map(|alias|format!("codex/{alias}")),key_hash:hop.key_hash.map(str::to_owned),..Default::default()});
                        upstream.send(Message::Text(event.to_string().into())).await.map_err(|_|"Codex WebSocket disconnected")?;
                    },
                    Message::Ping(_)|Message::Pong(_)=>{client.flush().await.map_err(|_|"Client disconnected")?;},
                    Message::Close(_)=>{let _=upstream.close(None).await;return Ok(());},
                    _=>{client.send(error("text_messages_required")).await.map_err(|_|"Client disconnected")?;},
                }
            },
            message=upstream.next()=>{
                let Some(Ok(message))=message else {return Err("Codex WebSocket disconnected".into());};
                if let Message::Text(text)=&message {
                    active.bytes=active.bytes.saturating_add(text.len());if active.bytes>64*1024*1024{return Err("Codex response exceeds 64 MiB".into());}
                    let event:Value=serde_json::from_str(text).map_err(|_|"Invalid Codex WebSocket event")?;
                    match event["type"].as_str() {
                        Some("response.completed"|"response.incomplete")=>{
                            let response=&event["response"];
                            if !response["output"].is_array() || response["id"].as_str().is_none_or(str::is_empty) {return Err("Invalid Codex completion".into());}
                            active.finish(adapter,response,200);
                        },
                        Some("error"|"response.failed")=>active.finish(adapter,&event["response"],502),
                        _=>{},
                    }
                    client.send(message).await.map_err(|_|"Client disconnected")?;
                } else if message.is_close() {let _=client.close(None).await;return Ok(());}
                else {upstream.flush().await.map_err(|_|"Codex WebSocket disconnected")?;}
            }
        }
    }
}
fn error(code: &str) -> Message {
    Message::Text(
        json!({"type":"error","error":{"type":"invalid_request_error","code":code,"message":code}})
            .to_string()
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forward::HopObserver;
    use std::{
        collections::HashMap,
        io::{BufRead, BufReader},
        net::TcpListener,
        sync::Mutex,
    };
    struct Observer(Mutex<Vec<UsageRecord>>);
    impl HopObserver for Observer {
        fn record(&self, record: UsageRecord) {
            // The hosted observer uses the synchronous Postgres client, which runs
            // its own runtime; this must be called outside the async task context.
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {});
            self.0.lock().unwrap().push(record);
        }
        fn log(&self, _: &str) {}
        fn authorize_response(
            &self,
            model: &str,
            _: &str,
            _: Option<&str>,
            _: Option<&Value>,
        ) -> Result<(), String> {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {});
            if model == "allowed" && self.0.lock().unwrap().len() < 2 {
                Ok(())
            } else {
                Err("Denied model".into())
            }
        }
    }
    #[test]
    fn each_websocket_creation_is_authorized_and_recorded_once() {
        use tokio_tungstenite::tungstenite;
        let origin = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", origin.local_addr().unwrap());
        let upstream = std::thread::spawn(move || {
            let (socket, _) = origin.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut ws = tungstenite::accept_hdr(
                socket,
                |request: &tungstenite::handshake::server::Request,
                 response: tungstenite::handshake::server::Response| {
                    assert_eq!(request.headers()["chatgpt-account-id"], "fixture-account");
                    assert_eq!(request.headers()["authorization"], "Bearer fixture-token");
                    Ok(response)
                },
            )
            .unwrap();
            for index in 0..2 {
                let request: Value =
                    serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(request["model"], "allowed");
                assert_eq!(request["store"], false);
                ws.send(Message::Text(json!({"type":"response.completed","response":{"id":format!("response-{index}"),"status":"completed","model":"allowed","output":[],"usage":{"input_tokens":8,"output_tokens":2,"total_tokens":10}}}).to_string().into())).unwrap();
            }
            let _ = ws.read();
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let caller = std::thread::spawn(move || {
            let socket = std::net::TcpStream::connect(address).unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let (mut client, _) =
                tungstenite::client(format!("ws://{address}/codex/v1/responses"), socket).unwrap();
            client
                .send(Message::Text(
                    json!({"type":"response.create","model":"denied","input":[]})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            let denied: Value =
                serde_json::from_str(client.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(denied["error"]["code"], "request_not_authorized");
            for _ in 0..2 {
                client
                    .send(Message::Text(
                        json!({"type":"response.create","model":"allowed","input":[]})
                            .to_string()
                            .into(),
                    ))
                    .unwrap();
                let event: Value =
                    serde_json::from_str(client.read().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(event["type"], "response.completed");
            }
            client
                .send(Message::Text(
                    json!({"type":"response.create","model":"allowed","input":[]})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            let revoked: Value =
                serde_json::from_str(client.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(revoked["error"]["code"], "request_not_authorized");
            let _ = client.close(None);
        });
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(socket.try_clone().unwrap());
        let mut headers = HashMap::new();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            let (name, value) = line.split_once(':').unwrap();
            headers.insert(name.to_lowercase(), value.trim().to_string());
        }
        let observer = Observer(Mutex::new(Vec::new()));
        let adapter = CodexAdapter { base: Some(base) };
        let routed = adapter.resolve("/codex/v1/responses").unwrap();
        forward(
            &adapter,
            WebSocketHop {
                client: &mut socket,
                headers: &headers,
                prefetched: Vec::new(),
                token: Some("fixture-token"),
                secret: Some(&json!({"account_id":"fixture-account"})),
                upstream: &routed,
                observer: &observer,
                alias: Some("work"),
                key_hash: Some("fixture-hash"),
            },
        )
        .unwrap();
        caller.join().unwrap();
        upstream.join().unwrap();
        let records = observer.0.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert_ne!(records[0].request_id, records[1].request_id);
        for record in records.iter() {
            assert_eq!(record.account_id.as_deref(), Some("codex/work"));
            assert_eq!(record.status, Some(200));
            assert_eq!(record.total_tokens, 10);
        }
    }
}
