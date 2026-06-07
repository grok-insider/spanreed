//! Local HTTP API on `127.0.0.1:6736`.
//!
//! Serves the latest probe results as JSON so other apps (status bars, scripts)
//! can read usage without re-probing. Minimal std-only HTTP, no framework.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::model::ProviderOutput;
use crate::probe;

const BIND_ADDR: &str = "127.0.0.1:6736";
const CONN_TIMEOUT: Duration = Duration::from_secs(5);

/// Shared cache of the most recent outputs.
type Cache = Arc<Mutex<Vec<ProviderOutput>>>;

pub fn serve(refresh_secs: u64) -> std::io::Result<()> {
    let listener = TcpListener::bind(BIND_ADDR)?;
    log::info!("local API listening on http://{BIND_ADDR}");

    let cache: Cache = Arc::new(Mutex::new(probe::probe_detected()));

    // Background refresher.
    {
        let cache = Arc::clone(&cache);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(refresh_secs.max(30)));
            let fresh = probe::probe_detected();
            if let Ok(mut c) = cache.lock() {
                *c = fresh;
            }
        });
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let cache = Arc::clone(&cache);
                std::thread::spawn(move || {
                    if let Err(e) = handle(stream, cache) {
                        log::debug!("connection error: {e}");
                    }
                });
            }
            Err(e) => log::warn!("accept failed: {e}"),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, cache: Cache) -> std::io::Result<()> {
    stream.set_read_timeout(Some(CONN_TIMEOUT))?;
    stream.set_write_timeout(Some(CONN_TIMEOUT))?;

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    let (status, body) = match path {
        "/" | "/usage" => {
            let outputs = cache.lock().map(|c| c.clone()).unwrap_or_default();
            (
                "200 OK",
                serde_json::to_string(&outputs).unwrap_or_else(|_| "[]".into()),
            )
        }
        "/health" => ("200 OK", "{\"status\":\"ok\"}".to_string()),
        _ => ("404 Not Found", "{\"error\":\"not found\"}".to_string()),
    };

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}
