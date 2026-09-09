//! Host-owned credentials and bounded, read-only remote consumption collection.
use fabrials_core::usage::ImportCheckpoint;
use fabrials_runtime::local_usage::{ImportBatch, UsageStore};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub client: String,
    pub account: String,
    pub credential: String,
}
fn path() -> std::path::PathBuf {
    crate::app::config_dir().join("usage-connections.json")
}
fn load() -> Result<BTreeMap<String, Connection>, String> {
    match std::fs::read(path()) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "Invalid usage connections".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(_) => Err("Cannot read usage connections".into()),
    }
}
fn connection_lock() -> Result<fabrials_runtime::credential_journal::Rotation, String> {
    let environment = crate::app::config_dir().to_string_lossy().into_owned();
    fabrials_runtime::credential_journal::Rotation::acquire(
        &crate::app::data_dir().join("credential-recovery"),
        fabrials_runtime::credential_journal::Scope {
            environment: &environment,
            owner: "local",
            provider: "usage-connections",
            alias: "configuration",
        },
    )
}
pub fn status() -> Result<Vec<String>, String> {
    let mut clients: Vec<_> = load()?.into_keys().collect();
    if crate::providers::antigravity::discover().is_some() {
        clients.push("antigravity".into());
    }
    Ok(clients)
}
pub fn save(connection: Connection) -> Result<(), String> {
    if !matches!(connection.client.as_str(), "cursor" | "trae" | "warp")
        || connection.account.is_empty()
        || connection.account.len() > 128
        || !connection
            .account
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        || connection.credential.is_empty()
        || connection.credential.len() > 16384
        || !connection.credential.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err("Choose a supported client, an account label using letters/numbers, and a valid credential".into());
    }
    let _lock = connection_lock()?;
    let mut connections = load()?;
    connections.insert(connection.client.clone(), connection);
    fabrials_runtime::files::atomic_write_private(
        &path(),
        &serde_json::to_vec(&connections).map_err(|_| "Invalid connection")?,
    )
    .map_err(|_| "Cannot save usage connection".into())
}
pub fn collect(client: &str, store: &mut UsageStore, force: bool) -> Result<bool, String> {
    if client == "antigravity" {
        return collect_antigravity(store, force);
    }
    let connections = load()?;
    let Some(connection) = connections.get(client) else {
        return Ok(false);
    };
    let source = format!("remote:{client}:{}", connection.account);
    let previous = store.source(&source)?;
    let now = crate::util::now_ms();
    if !force
        && previous
            .as_ref()
            .is_some_and(|s| now - s.last_success_ms < 300_000)
    {
        return Ok(true);
    }
    let started = std::time::Instant::now();
    let mut remaining = 32 * 1024 * 1024;
    let records =
        fabrials_providers::usage::remote::collect(client, &connection.account, now, |request| {
            if started.elapsed() > std::time::Duration::from_secs(45) {
                return Err("Usage collection exceeded its time budget".into());
            }
            let mut http = crate::http::Request::post(request.endpoint)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .header(
                    request.auth_header,
                    format!("{}{}", request.auth_prefix, connection.credential),
                )
                .body(request.body.to_string());
            if let Some(origin) = request.origin {
                http = http
                    .header("Origin", origin)
                    .header("Referer", format!("{origin}/settings"));
            }
            let response = http
                .send_limited(remaining)
                .map_err(|_| "Usage request failed; previous snapshot retained")?;
            remaining = remaining.saturating_sub(response.body.len());
            if response.is_auth_error() {
                return Err("Usage connection expired; replace its credential".into());
            }
            if !(200..300).contains(&response.status) {
                return Err(format!("Usage provider returned HTTP {}", response.status));
            }
            response
                .json()
                .ok_or_else(|| "Invalid usage response".into())
        })?;
    let _lock = connection_lock()?;
    if load()?.get(client) != Some(connection) {
        return Err("Usage connection changed during collection; result discarded".into());
    }
    store.import(ImportBatch {
        source: &source,
        client,
        parser_version: 1,
        fingerprint: "remote-v1",
        expected_generation: previous.map(|s| s.generation),
        replace: true,
        checkpoint: &ImportCheckpoint::default(),
        records: &records,
        at_ms: now,
    })?;
    // A complete fresh report supersedes cached copies and the previous account.
    for old in store
        .source_ids(client)?
        .into_iter()
        .filter(|old| old != &source)
    {
        if let Some(previous) = store.source(&old)? {
            store.import(ImportBatch {
                source: &old,
                client,
                parser_version: 1,
                fingerprint: "superseded",
                expected_generation: Some(previous.generation),
                replace: true,
                checkpoint: &ImportCheckpoint::default(),
                records: &[],
                at_ms: now,
            })?;
        }
    }
    Ok(true)
}

fn collect_antigravity(store: &mut UsageStore, force: bool) -> Result<bool, String> {
    let Some(connection) = crate::providers::antigravity::discover() else {
        return Ok(false);
    };
    let source = "remote:antigravity:local";
    let previous = store.source(source)?;
    let now = crate::util::now_ms();
    if !force
        && previous
            .as_ref()
            .is_some_and(|s| now - s.last_success_ms < 300_000)
    {
        return Ok(true);
    }
    let started = std::time::Instant::now();
    let mut remaining = 32 * 1024 * 1024;
    let records = fabrials_providers::usage::remote::antigravity(|method, body| {
        if started.elapsed() > std::time::Duration::from_secs(45) {
            return Err("Antigravity collection exceeded its time budget".into());
        }
        for port in &connection.ports {
            let result = crate::http::Request::post(format!(
                "https://127.0.0.1:{port}/exa.language_server_pb.LanguageServerService/{method}"
            ))
            .insecure()
            .header("Content-Type", "application/json")
            .header("Connect-Protocol-Version", "1")
            .header("x-codeium-csrf-token", &connection.csrf)
            .body(body.to_string())
            .send_limited(remaining);
            if let Ok(response) = result {
                remaining = remaining.saturating_sub(response.body.len());
                if (200..300).contains(&response.status) {
                    return response
                        .json()
                        .ok_or_else(|| "Invalid Antigravity response".into());
                }
            }
        }
        Err("Antigravity language server did not answer the usage request".into())
    })?;
    store.import(ImportBatch {
        source,
        client: "antigravity",
        parser_version: 1,
        fingerprint: "remote-v1",
        expected_generation: previous.map(|s| s.generation),
        replace: true,
        checkpoint: &ImportCheckpoint::default(),
        records: &records,
        at_ms: now,
    })?;
    Ok(true)
}

pub fn disconnect(client: &str) -> Result<(), String> {
    if !matches!(client, "cursor" | "trae" | "warp") {
        return Err("This client has no stored usage credential".into());
    }
    let _lock = connection_lock()?;
    let mut connections = load()?;
    connections.remove(client);
    fabrials_runtime::files::atomic_write_private(
        &path(),
        &serde_json::to_vec(&connections).map_err(|_| "Invalid connections")?,
    )
    .map_err(|_| "Cannot remove usage connection")?;
    let mut store = UsageStore::open(&crate::history::history_path())?;
    for source in store
        .source_ids(client)?
        .into_iter()
        .filter(|source| source.starts_with("remote:"))
    {
        if let Some(previous) = store.source(&source)? {
            store.import(ImportBatch {
                source: &source,
                client,
                parser_version: 1,
                fingerprint: "disconnected",
                expected_generation: Some(previous.generation),
                replace: true,
                checkpoint: &ImportCheckpoint::default(),
                records: &[],
                at_ms: crate::util::now_ms(),
            })?;
        }
    }
    Ok(())
}
