//! Local HTTP API on `127.0.0.1:6736`.
//!
//! Serves the latest probe results as JSON so other apps (status bars, scripts)
//! can read usage without re-probing. Minimal std-only HTTP, no framework.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::model::ProviderOutput;
use crate::probe;

const BIND_ADDR: &str = "127.0.0.1:6736";
const CONN_TIMEOUT: Duration = Duration::from_secs(5);

/// Backoff schedule (seconds) when every detected provider fails to probe —
/// the signature of probing before the network is up (login, suspend/resume).
const RETRY_BACKOFF_SECS: [u64; 3] = [5, 10, 30];

/// How many consecutive refreshes a provider's last-good result is served in
/// place of a fresh error before the error is surfaced.
const MAX_STALE_REFRESHES: u32 = 3;

/// Shared cache of the most recent outputs.
type Cache = Arc<Mutex<Vec<ProviderOutput>>>;

/// Fetch the cached outputs from a running daemon, if one is up.
///
/// Lets `spanreed waybar` reuse the daemon's already-refreshed data instead of
/// re-probing on every status-bar poll. Returns None when the daemon is down or
/// the response is empty/unusable.
pub fn fetch_cached() -> Option<Vec<ProviderOutput>> {
    let resp = crate::http::Request::get(format!("http://{BIND_ADDR}/usage"))
        .send()
        .ok()?;
    if !(200..300).contains(&resp.status) {
        return None;
    }
    let outputs: Vec<ProviderOutput> = serde_json::from_str(resp.body.trim()).ok()?;
    if outputs.is_empty() || outputs.iter().all(ProviderOutput::has_error) {
        None
    } else {
        Some(outputs)
    }
}

/// Probe all detected providers, retrying with a short backoff while every
/// provider errors (network likely not up yet). Gives up after the schedule
/// and returns whatever the last attempt produced.
fn probe_with_retry() -> Vec<ProviderOutput> {
    let mut outputs = probe::probe_detected();
    for backoff in RETRY_BACKOFF_SECS {
        let all_err = !outputs.is_empty() && outputs.iter().all(ProviderOutput::has_error);
        if !all_err {
            break;
        }
        log::info!("all probes failed (network down?), retrying in {backoff}s");
        std::thread::sleep(Duration::from_secs(backoff));
        outputs = probe::probe_detected();
    }
    outputs
}

/// Merge a fresh probe into the previous cache, retaining each provider's
/// last-good result when the fresh one is an error — but only for up to
/// `max_stale` consecutive refreshes, so persistent failures still surface.
fn merge(
    prev: &[ProviderOutput],
    fresh: Vec<ProviderOutput>,
    stale_counts: &mut HashMap<String, u32>,
    max_stale: u32,
) -> Vec<ProviderOutput> {
    fresh
        .into_iter()
        .map(|out| {
            if !out.has_error() {
                stale_counts.remove(&out.provider_id);
                return out;
            }
            let last_good = prev
                .iter()
                .find(|p| p.provider_id == out.provider_id && !p.has_error());
            match last_good {
                Some(good) => {
                    let n = stale_counts.entry(out.provider_id.clone()).or_insert(0);
                    *n += 1;
                    if *n <= max_stale {
                        log::debug!(
                            "{}: probe failed, serving last-good result ({n}/{max_stale})",
                            out.provider_id
                        );
                        good.clone()
                    } else {
                        out
                    }
                }
                None => out,
            }
        })
        .collect()
}

pub fn serve(refresh_secs: u64) -> std::io::Result<()> {
    let listener = TcpListener::bind(BIND_ADDR)?;
    log::info!("local API listening on http://{BIND_ADDR}");

    let initial = probe_with_retry();
    crate::history::record(&initial);
    let cache: Cache = Arc::new(Mutex::new(initial));

    // Background refresher.
    {
        let cache = Arc::clone(&cache);
        std::thread::spawn(move || {
            let mut stale_counts = HashMap::new();
            loop {
                std::thread::sleep(Duration::from_secs(refresh_secs.max(30)));
                let fresh = probe_with_retry();
                crate::history::record(&fresh);
                if let Ok(mut c) = cache.lock() {
                    *c = merge(&c, fresh, &mut stale_counts, MAX_STALE_REFRESHES);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MetricLine;

    fn good(id: &str, value: &str) -> ProviderOutput {
        ProviderOutput::new(id, id, vec![MetricLine::text("Session", value)])
    }

    fn bad(id: &str) -> ProviderOutput {
        ProviderOutput::error(id, id, "error sending request")
    }

    fn value_of(out: &ProviderOutput) -> &str {
        match &out.lines[0] {
            MetricLine::Text { value, .. } => value,
            _ => panic!("expected text line"),
        }
    }

    #[test]
    fn fresh_good_replaces_old() {
        let mut counts = HashMap::new();
        let prev = vec![good("claude", "old")];
        let merged = merge(&prev, vec![good("claude", "new")], &mut counts, 3);
        assert_eq!(value_of(&merged[0]), "new");
        assert!(counts.is_empty());
    }

    #[test]
    fn fresh_error_retains_last_good_until_max_stale() {
        let mut counts = HashMap::new();
        let prev = vec![good("claude", "ok")];
        for _ in 0..3 {
            let merged = merge(&prev, vec![bad("claude")], &mut counts, 3);
            assert!(!merged[0].has_error());
            assert_eq!(value_of(&merged[0]), "ok");
        }
        let merged = merge(&prev, vec![bad("claude")], &mut counts, 3);
        assert!(merged[0].has_error(), "error surfaces after max_stale");
    }

    #[test]
    fn recovery_resets_stale_count() {
        let mut counts = HashMap::new();
        let prev = vec![good("claude", "ok")];
        merge(&prev, vec![bad("claude")], &mut counts, 3);
        merge(&prev, vec![bad("claude")], &mut counts, 3);
        merge(&prev, vec![good("claude", "back")], &mut counts, 3);
        assert!(counts.is_empty());
        for _ in 0..3 {
            let merged = merge(&prev, vec![bad("claude")], &mut counts, 3);
            assert!(!merged[0].has_error());
        }
    }

    #[test]
    fn error_without_last_good_passes_through() {
        let mut counts = HashMap::new();
        let merged = merge(&[], vec![bad("claude")], &mut counts, 3);
        assert!(merged[0].has_error());
        let prev = vec![bad("claude")];
        let merged = merge(&prev, vec![bad("claude")], &mut counts, 3);
        assert!(merged[0].has_error());
    }

    #[test]
    fn merge_is_per_provider() {
        let mut counts = HashMap::new();
        let prev = vec![good("claude", "ok"), good("codex", "ok")];
        let merged = merge(
            &prev,
            vec![bad("claude"), good("codex", "new"), bad("grok")],
            &mut counts,
            3,
        );
        assert_eq!(merged.len(), 3);
        assert!(!merged[0].has_error(), "claude retained");
        assert_eq!(value_of(&merged[1]), "new");
        assert!(merged[2].has_error(), "grok had no last-good");
    }
}
