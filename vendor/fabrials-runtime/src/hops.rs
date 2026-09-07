//! Durable, idempotent completed-hop ledger. Contains metadata and usage, never bodies.
use fabrials_model::UsageRecord;
use rusqlite::{params, OptionalExtension};
use std::path::Path;

pub struct HopStore {
    connection: rusqlite::Connection,
}
impl HopStore {
    pub fn recent(&self, namespace: &str, limit: usize) -> Result<Vec<UsageRecord>, String> {
        validate_namespace(namespace)?;
        let mut query = self.connection.prepare("SELECT document FROM usage_hops WHERE namespace=?1 ORDER BY ts_ms DESC,identity DESC LIMIT ?2").map_err(|e| e.to_string())?;
        let rows = query
            .query_map(params![namespace, limit.min(1000) as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(|e| e.to_string())?)
                .map_err(|_| "Invalid stored hop".into())
        })
        .collect()
    }
    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = crate::database::open(path)?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS usage_hops (
            namespace TEXT NOT NULL, identity TEXT NOT NULL, ts_ms INTEGER NOT NULL,
            provider TEXT NOT NULL, document TEXT NOT NULL, PRIMARY KEY(namespace,identity)
        );
        CREATE INDEX IF NOT EXISTS usage_hops_window ON usage_hops(namespace,provider,ts_ms);
        CREATE INDEX IF NOT EXISTS usage_hops_recent ON usage_hops(namespace,ts_ms DESC);
        CREATE TABLE IF NOT EXISTS hop_imports (namespace TEXT NOT NULL, source TEXT NOT NULL, PRIMARY KEY(namespace,source));").map_err(|e| e.to_string())?;
        Ok(Self { connection })
    }
    pub fn append(&mut self, namespace: &str, record: &UsageRecord) -> Result<bool, String> {
        validate_namespace(namespace)?;
        let identity = record_identity(record)
            .unwrap_or_else(|| format!("generated:{}", crate::accounting::new_request_id()));
        insert(&self.connection, namespace, &identity, record)
    }
    /// Read all matching records, bounded in time rather than silently truncating totals.
    pub fn read_window(
        &self,
        namespace: &str,
        provider: Option<&str>,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<UsageRecord>, String> {
        validate_namespace(namespace)?;
        let mut query = self.connection.prepare("SELECT document FROM usage_hops WHERE namespace=?1 AND (?2 IS NULL OR provider=?2) AND ts_ms>=?3 AND ts_ms<=?4 ORDER BY ts_ms,identity").map_err(|e| e.to_string())?;
        let rows = query
            .query_map(params![namespace, provider, from_ms, to_ms], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(|e| e.to_string())?)
                .map_err(|_| "Invalid stored hop".into())
        })
        .collect()
    }
    /// Commit imported records and their marker atomically; leave the source untouched.
    pub fn import_jsonl_once(
        &mut self,
        namespace: &str,
        source: &str,
        path: &Path,
    ) -> Result<usize, String> {
        use std::io::{BufRead, BufReader, Read};
        validate_namespace(namespace)?;
        if source.is_empty() || source.len() > 256 {
            return Err("Invalid hop import source".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM hop_imports WHERE namespace=?1 AND source=?2",
                params![namespace, source],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if exists.is_some() {
            return Ok(0);
        }
        let mut count = 0;
        match std::fs::File::open(path) {
            Ok(file) => {
                if file.metadata().map_err(|e| e.to_string())?.len() > 256 * 1024 * 1024 {
                    return Err("Legacy hop ledger exceeds import limit".into());
                }
                let mut reader = BufReader::new(file);
                let mut line = Vec::new();
                let mut number = 0;
                loop {
                    line.clear();
                    let read = (&mut reader)
                        .take(16_385)
                        .read_until(b'\n', &mut line)
                        .map_err(|e| e.to_string())?;
                    if read == 0 {
                        break;
                    }
                    number += 1;
                    if number > 1_000_000 || read > 16_384 {
                        return Err("Legacy hop ledger exceeds import limit".into());
                    }
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    let record: UsageRecord = serde_json::from_slice(&line)
                        .map_err(|_| format!("Invalid legacy hop at line {number}"))?;
                    let identity = record_identity(&record)
                        .unwrap_or_else(|| format!("import:{source}:{number}"));
                    count += usize::from(insert(&tx, namespace, &identity, &record)?);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        tx.execute(
            "INSERT INTO hop_imports(namespace,source) VALUES(?1,?2)",
            params![namespace, source],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(count)
    }
}
fn validate_namespace(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err("Invalid hop namespace".into());
    }
    Ok(())
}
fn record_identity(record: &UsageRecord) -> Option<String> {
    record
        .request_id
        .as_ref()
        .filter(|id| !id.is_empty())
        .map(|id| format!("request:{id}"))
}
fn insert(
    connection: &rusqlite::Connection,
    namespace: &str,
    identity: &str,
    record: &UsageRecord,
) -> Result<bool, String> {
    let document = serde_json::to_string(record).map_err(|e| e.to_string())?;
    if document.len() > 16_384 || identity.len() > 1024 {
        return Err("Hop metadata exceeds storage limit".into());
    }
    connection.execute("INSERT INTO usage_hops(namespace,identity,ts_ms,provider,document) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(namespace,identity) DO NOTHING", params![namespace,identity,record.ts_ms,record.provider.as_deref().unwrap_or("grok"),document]).map(|count| count>0).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "fabrials-hops-{}",
                crate::accounting::new_request_id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn store(&self) -> HopStore {
            HopStore::open(&self.0.join("runtime.sqlite3")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn record(provider: &str) -> UsageRecord {
        UsageRecord {
            ts_ms: 100,
            provider: Some(provider.into()),
            request_id: Some("fixture-request".into()),
            input_tokens: 20,
            output_tokens: 10,
            ..Default::default()
        }
    }
    #[test]
    fn concurrent_writes_are_idempotent_and_scoped() {
        let fixture = Fixture::new();
        drop(fixture.store());
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let fixture = &fixture;
                scope.spawn(move || {
                    fixture.store().append("local", &record("grok")).unwrap();
                });
            }
        });
        let mut store = fixture.store();
        assert_eq!(store.read_window("local", None, 0, 200).unwrap().len(), 1);
        assert!(store.append("hosted-owner", &record("nous")).unwrap());
        assert!(store
            .read_window("local", Some("nous"), 0, 200)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .read_window("hosted-owner", Some("nous"), 0, 200)
                .unwrap()
                .len(),
            1
        );
        assert!(store
            .read_window("local", None, 101, 200)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn failed_import_rolls_back_and_source_is_preserved() {
        let fixture = Fixture::new();
        let source = fixture.0.join("old.jsonl");
        let line = serde_json::to_string(&record("grok")).unwrap();
        std::fs::write(&source, format!("{line}\ninvalid\n")).unwrap();
        let mut store = fixture.store();
        assert!(store.import_jsonl_once("local", "legacy", &source).is_err());
        assert!(store.read_window("local", None, 0, 200).unwrap().is_empty());
        let content = format!("{line}\n{line}\n");
        std::fs::write(&source, &content).unwrap();
        assert_eq!(
            store.import_jsonl_once("local", "legacy", &source).unwrap(),
            1
        );
        assert_eq!(
            store.import_jsonl_once("local", "legacy", &source).unwrap(),
            0
        );
        assert_eq!(std::fs::read_to_string(&source).unwrap(), content);
        assert!(!store.append("local", &record("grok")).unwrap());
    }
    #[test]
    fn records_without_request_ids_are_not_collapsed() {
        let fixture = Fixture::new();
        let mut record = record("grok");
        record.request_id = None;
        let mut store = fixture.store();
        assert!(store.append("local", &record).unwrap());
        assert!(store.append("local", &record).unwrap());
        assert_eq!(store.read_window("local", None, 0, 200).unwrap().len(), 2);
    }
}
