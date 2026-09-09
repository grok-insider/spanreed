use super::discovery;
#[cfg(test)]
use fabrials_core::usage::UsageFilter;
use fabrials_core::usage::{ImportCheckpoint, ImportState, SourceStatus, UsageParser};
use fabrials_providers::usage::{catalog, files};
use fabrials_runtime::local_usage::{ImportBatch, UsageStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

static IMPORT: Mutex<()> = Mutex::new(());
const VERSION: u32 = 1;
const MAX_INPUT: u64 = 1024 * 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
struct Fingerprint {
    #[serde(default)]
    rejected: u64,
    size: u64,
    modified: u128,
    hash: String,
    wal: Option<(u64, u128)>,
}

fn metadata(path: &Path) -> Result<(u64, u128), String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("Not a regular usage file".into());
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Ok((metadata.len(), modified))
}

fn digest(file: &mut std::fs::File, limit: u64) -> Result<String, String> {
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut reader = file.take(limit);
    std::io::copy(&mut reader, &mut hash).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", hash.finalize()))
}

fn import_file(
    store: &mut UsageStore,
    client: &str,
    path: &Path,
    force: bool,
) -> Result<(), String> {
    let source = format!("{client}:{}", path.display());
    let previous = store.source(&source)?;
    let before = metadata(path)?;
    if before.0 > MAX_INPUT {
        return Err("Usage file exceeds the 1 GiB per-file bound".into());
    }
    let old: Fingerprint = previous
        .as_ref()
        .and_then(|s| serde_json::from_str(&s.fingerprint).ok())
        .unwrap_or_default();
    let wal = metadata(&std::path::PathBuf::from(format!("{}-wal", path.display()))).ok();
    let related = matches!(
        client,
        "claude" | "droid" | "crush" | "devin-desktop" | "codebuff" | "freebuff" | "amp"
    );
    if !force
        && !related
        && previous
            .as_ref()
            .is_some_and(|s| s.parser_version == VERSION)
        && before == (old.size, old.modified)
        && wal == old.wal
    {
        return if old.rejected > 0 {
            Err(format!(
                "{} malformed usage records; valid records retained",
                old.rejected
            ))
        } else {
            Ok(())
        };
    }
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let parser: Option<Box<dyn UsageParser>> = match client {
        "codex" => Some(Box::new(fabrials_providers::usage::codex::Codex)),
        // Other formats can revise earlier messages or consult sibling files;
        // replacement imports retain their native update semantics.
        _ => None,
    };
    let incremental = old.rejected == 0
        && parser.is_some()
        && previous
            .as_ref()
            .is_some_and(|s| s.parser_version == VERSION)
        && before.0 >= old.size
        && old.size > 0
        && digest(&mut file, old.size)? == old.hash;
    let (records, checkpoint, rejected) = if let Some(parser) = parser {
        let mut checkpoint = if incremental {
            previous.as_ref().unwrap().checkpoint.clone()
        } else {
            ImportCheckpoint::default()
        };
        if checkpoint.parser_state.is_null() {
            checkpoint.parser_state = serde_json::json!({});
        }
        checkpoint.parser_state["fallback_at_ms"] =
            serde_json::json!((before.1 / 1_000_000).min(i64::MAX as u128) as i64);
        file.seek(SeekFrom::Start(checkpoint.offset))
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        let parsed = parser.parse(&bytes, &source, &checkpoint)?;
        (parsed.records, parsed.checkpoint, parsed.rejected_records)
    } else {
        let records = files::read(client, path)?;
        (
            records,
            ImportCheckpoint {
                offset: before.0,
                parser_state: serde_json::Value::Null,
            },
            0,
        )
    };
    let fingerprint = Fingerprint {
        rejected,
        size: before.0,
        modified: before.1,
        hash: digest(&mut file, before.0)?,
        wal,
    };
    if before != metadata(path)?
        || fingerprint.wal
            != metadata(&std::path::PathBuf::from(format!("{}-wal", path.display()))).ok()
    {
        return Err("Usage input changed while being read; retry on the next refresh".into());
    }
    store.import(ImportBatch {
        source: &source,
        client,
        parser_version: VERSION,
        fingerprint: &serde_json::to_string(&fingerprint).map_err(|e| e.to_string())?,
        expected_generation: previous.map(|s| s.generation),
        replace: !incremental && rejected == 0,
        checkpoint: &checkpoint,
        records: &records,
        at_ms: crate::util::now_ms(),
    })?;
    if rejected > 0 {
        Err(format!(
            "{rejected} malformed usage records; valid records retained"
        ))
    } else {
        Ok(())
    }
}

pub fn refresh(selected: Option<&str>, force: bool) -> Result<Vec<SourceStatus>, String> {
    let _guard = IMPORT.lock().unwrap_or_else(|e| e.into_inner());
    let settings = discovery::settings()?;
    if selected.is_some_and(|id| !catalog::clients().iter().any(|c| c.id == id)) {
        return Err("Unknown usage client".into());
    }
    let mut store = UsageStore::open(&crate::history::history_path())?;
    let mut statuses = Vec::new();
    let connected_clients = super::connections::status()?;
    for client in catalog::clients()
        .iter()
        .filter(|c| selected.is_none_or(|id| c.id == id))
    {
        if settings.disabled_clients.contains(&client.id) {
            continue;
        }
        let connected = connected_clients.contains(&client.id);
        let mut remote_error = None;
        if connected {
            if let Err(error) = super::connections::collect(&client.id, &mut store, force) {
                remote_error = Some(error);
            }
        }
        let discovered = if connected {
            discovery::Discovery {
                files: Vec::new(),
                errors: remote_error.into_iter().collect(),
            }
        } else {
            discovery::discover(client, &settings)
        };
        let mut errors = discovered.errors;
        for path in &discovered.files {
            if let Err(error) = import_file(&mut store, &client.id, path, force) {
                errors.push(error);
            }
        }
        let mut last_success = None;
        for id in store.source_ids(&client.id)? {
            if let Some(source) = store.source(&id)? {
                last_success = Some(last_success.map_or(source.last_success_ms, |t: i64| {
                    t.max(source.last_success_ms)
                }));
                if errors.is_empty() {
                    if let Some(path) = id.strip_prefix(&format!("{}:", client.id)) {
                        if std::fs::metadata(path)
                            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                        {
                            store.import(ImportBatch {
                                source: &id,
                                client: &client.id,
                                parser_version: VERSION,
                                fingerprint: "removed",
                                expected_generation: Some(source.generation),
                                replace: true,
                                checkpoint: &ImportCheckpoint::default(),
                                records: &[],
                                at_ms: crate::util::now_ms(),
                            })?;
                        }
                    }
                }
            }
        }
        let count = store.record_count(&client.id)?;
        let state = if !errors.is_empty() {
            if count > 0 {
                ImportState::Partial
            } else if errors
                .iter()
                .any(|error| error.starts_with("Unsupported usage format:"))
            {
                ImportState::UnsupportedFormat
            } else {
                ImportState::Error
            }
        } else if count > 0 {
            ImportState::Ready
        } else if connected {
            ImportState::NoActivity
        } else if discovered.files.is_empty() {
            if client.remote_collection() {
                ImportState::NeedsConnection
            } else {
                ImportState::NoData
            }
        } else {
            ImportState::NoActivity
        };
        statuses.push(SourceStatus {
            source: client.id.clone(),
            client: client.id.clone(),
            state,
            last_success_ms: last_success,
            revision: store.revision()?,
            records: count,
            detail: (!errors.is_empty()).then(|| errors.join("; ")),
        });
    }
    Ok(statuses)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "spanreed-import-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn store(&self) -> UsageStore {
            UsageStore::open(&self.0.join("usage.sqlite3")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn rollout(total: u64) -> String {
        format!(
            "{}\n{}\n",
            serde_json::json!({"type":"session_meta","payload":{"id":"session"}}),
            event(total)
        )
    }
    fn event(total: u64) -> String {
        serde_json::json!({"type":"event_msg","timestamp":"2026-09-01T00:00:00Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":total}}}}).to_string()
    }
    fn total(store: &UsageStore) -> u64 {
        store
            .records(&UsageFilter::default())
            .unwrap()
            .iter()
            .map(|r| r.tokens.as_ref().unwrap().total())
            .sum()
    }
    #[test]
    fn restart_append_partial_line_and_corrections() {
        use std::io::Write;
        let fixture = Fixture::new();
        let path = fixture.0.join("session.jsonl");
        std::fs::write(&path, rollout(10)).unwrap();
        let mut store = fixture.store();
        import_file(&mut store, "codex", &path, false).unwrap();
        assert_eq!(total(&store), 10);
        let revision = store.revision().unwrap();
        import_file(&mut store, "codex", &path, true).unwrap();
        assert_eq!(store.revision().unwrap(), revision);
        drop(store);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        write!(file, "{}", event(30)).unwrap();
        let mut store = fixture.store();
        import_file(&mut store, "codex", &path, false).unwrap();
        assert_eq!(total(&store), 10);
        writeln!(file).unwrap();
        import_file(&mut store, "codex", &path, false).unwrap();
        assert_eq!(total(&store), 30);
        std::fs::write(&path, rollout(5)).unwrap();
        import_file(&mut store, "codex", &path, true).unwrap();
        assert_eq!(total(&store), 5);
        std::fs::write(&path, "").unwrap();
        import_file(&mut store, "codex", &path, true).unwrap();
        assert_eq!(total(&store), 0);
    }
    #[test]
    fn archive_copy_deduplicates_and_malformed_replacement_retains_data() {
        let fixture = Fixture::new();
        let original = fixture.0.join("original.jsonl");
        let archive = fixture.0.join("archive.jsonl");
        std::fs::write(&original, rollout(10)).unwrap();
        std::fs::copy(&original, &archive).unwrap();
        let mut store = fixture.store();
        import_file(&mut store, "codex", &original, false).unwrap();
        import_file(&mut store, "codex", &archive, false).unwrap();
        assert_eq!(total(&store), 10);
        let revision = store.revision().unwrap();
        std::fs::write(&original, "{\"type\":\"token_count\",broken}\n").unwrap();
        assert!(import_file(&mut store, "codex", &original, true).is_err());
        assert_eq!(total(&store), 10);
        assert_eq!(store.revision().unwrap(), revision);
    }
}
