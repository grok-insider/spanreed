pub use fabrials_model::HistorySample;

/// Whether `curr` should be treated as a new rate-limit epoch vs `prev`.
pub fn is_reset_event(prev: &HistorySample, curr: &HistorySample) -> bool {
    matches!(
        (&prev.resets_at, &curr.resets_at),
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() && a != b
    )
}

/// Whether we should skip writing `curr` because nothing material changed.
pub fn is_duplicate(prev: &HistorySample, curr: &HistorySample) -> bool {
    if is_reset_event(prev, curr) {
        return false;
    }
    same_used(prev.used, curr.used) && prev.resets_at == curr.resets_at
}

fn same_used(a: f64, b: f64) -> bool {
    (a * 10.0).round() == (b * 10.0).round()
}

/// Apply reset event flag and decide write eligibility against last known sample.
pub fn prepare_sample(
    prev: Option<&HistorySample>,
    mut curr: HistorySample,
) -> Option<HistorySample> {
    if let Some(p) = prev {
        if is_duplicate(p, &curr) {
            return None;
        }
        if is_reset_event(p, &curr) {
            curr.event = Some("reset".into());
        }
    }
    Some(curr)
}

/// Transactional local history, scoped by host environment/owner namespace.
pub struct HistoryStore {
    connection: rusqlite::Connection,
}
impl HistoryStore {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let connection = crate::database::open(path)?;
        connection.execute_batch("            CREATE TABLE IF NOT EXISTS usage_history (
                seq INTEGER PRIMARY KEY,
                namespace TEXT NOT NULL,
                provider TEXT NOT NULL,
                label TEXT NOT NULL,
                ts_ms INTEGER NOT NULL,
                document TEXT NOT NULL,
                UNIQUE(namespace, provider, label, ts_ms)
            );
            CREATE INDEX IF NOT EXISTS usage_history_latest ON usage_history(namespace, provider, label, ts_ms DESC);
            CREATE INDEX IF NOT EXISTS usage_history_time ON usage_history(namespace, ts_ms DESC);
            CREATE TABLE IF NOT EXISTS history_imports (namespace TEXT NOT NULL, source TEXT NOT NULL, PRIMARY KEY(namespace, source));"
        ).map_err(|e| e.to_string())?;
        Ok(Self { connection })
    }

    pub fn append(&mut self, namespace: &str, samples: &[HistorySample]) -> Result<usize, String> {
        validate_namespace(namespace)?;
        if samples.len() > 4096 {
            return Err("History batch too large".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let count = append_transaction(&tx, namespace, samples)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(count)
    }

    pub fn imported(&self, namespace: &str, source: &str) -> Result<bool, String> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM history_imports WHERE namespace=?1 AND source=?2)",
                rusqlite::params![namespace, source],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())
    }

    /// The marker and imported samples commit together. The source is never modified.
    pub fn import_once(
        &mut self,
        namespace: &str,
        source: &str,
        samples: &[HistorySample],
    ) -> Result<usize, String> {
        validate_namespace(namespace)?;
        if samples.len() > 100_000 || source.is_empty() || source.len() > 256 {
            return Err("History import too large".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM history_imports WHERE namespace=?1 AND source=?2)",
                rusqlite::params![namespace, source],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists {
            return Ok(0);
        }
        let mut samples = samples.to_vec();
        samples.sort_by_key(|sample| sample.ts_ms);
        let count = append_transaction(&tx, namespace, &samples)?;
        tx.execute(
            "INSERT INTO history_imports(namespace,source) VALUES(?1,?2)",
            rusqlite::params![namespace, source],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(count)
    }

    pub fn read(
        &self,
        namespace: &str,
        provider: Option<&str>,
        limit: usize,
    ) -> Result<Vec<HistorySample>, String> {
        validate_namespace(namespace)?;
        let mut query = self.connection.prepare("SELECT document FROM usage_history WHERE namespace=?1 AND (?2 IS NULL OR provider=?2) ORDER BY ts_ms DESC, seq DESC LIMIT ?3").map_err(|e| e.to_string())?;
        let rows = query
            .query_map(
                rusqlite::params![namespace, provider, limit.min(100_000) as i64],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())?;
        let mut samples = rows
            .map(|row| {
                serde_json::from_str(&row.map_err(|e| e.to_string())?)
                    .map_err(|_| "Invalid stored history sample".into())
            })
            .collect::<Result<Vec<_>, String>>()?;
        samples.reverse();
        Ok(samples)
    }

    /// Retain the latest observation per metric even if its provider is inactive.
    pub fn prune(&mut self, namespace: &str, cutoff_ms: i64) -> Result<usize, String> {
        validate_namespace(namespace)?;
        self.connection.execute("DELETE FROM usage_history WHERE namespace=?1 AND ts_ms<?2 AND seq NOT IN (SELECT MAX(seq) FROM usage_history WHERE namespace=?1 GROUP BY provider,label)", rusqlite::params![namespace,cutoff_ms]).map_err(|e| e.to_string())
    }
}

fn validate_namespace(namespace: &str) -> Result<(), String> {
    if namespace.is_empty() || namespace.len() > 256 || namespace.chars().any(char::is_control) {
        return Err("Invalid history namespace".into());
    }
    Ok(())
}

fn append_transaction(
    tx: &rusqlite::Transaction<'_>,
    namespace: &str,
    samples: &[HistorySample],
) -> Result<usize, String> {
    use rusqlite::OptionalExtension;
    let mut count = 0;
    for sample in samples {
        if !sample.used.is_finite()
            || !sample.limit.is_finite()
            || sample.limit <= 0.0
            || sample.provider.is_empty()
            || sample.label.is_empty()
            || sample.provider.len() > 128
            || sample.label.len() > 128
        {
            return Err("Invalid history sample".into());
        }
        let previous: Option<String> = tx.query_row("SELECT document FROM usage_history WHERE namespace=?1 AND provider=?2 AND label=?3 ORDER BY ts_ms DESC,seq DESC LIMIT 1", rusqlite::params![namespace,sample.provider,sample.label], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
        let previous: Option<HistorySample> = previous
            .map(|raw| {
                serde_json::from_str(&raw).map_err(|_| "Invalid stored history sample".to_string())
            })
            .transpose()?;
        if previous
            .as_ref()
            .is_some_and(|prev| prev.ts_ms > sample.ts_ms)
        {
            continue;
        }
        let Some(sample) = prepare_sample(previous.as_ref(), sample.clone()) else {
            continue;
        };
        let document = serde_json::to_string(&sample).map_err(|e| e.to_string())?;
        if document.len() > 16_384 {
            return Err("History sample too large".into());
        }
        count += tx.execute("INSERT INTO usage_history(namespace,provider,label,ts_ms,document) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(namespace,provider,label,ts_ms) DO NOTHING", rusqlite::params![namespace,sample.provider,sample.label,sample.ts_ms,document]).map_err(|e| e.to_string())?;
    }
    Ok(count)
}

#[cfg(test)]
mod store_tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "fabrials-history-{}",
                crate::accounting::new_request_id()
            )))
        }
        fn path(&self) -> std::path::PathBuf {
            self.0.join("runtime.sqlite3")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn sample(time: i64, used: f64) -> HistorySample {
        HistorySample {
            ts_ms: time,
            provider: "grok".into(),
            plan: Some("SuperGrok".into()),
            label: "Weekly".into(),
            used,
            limit: 100.0,
            resets_at: Some("2030-01-01".into()),
            window_start_ms: None,
            limit_window_secs: None,
            kind: "percent".into(),
            event: None,
        }
    }
    #[test]
    fn independent_connections_deduplicate_and_isolate_namespaces() {
        let fixture = Fixture::new();
        drop(HistoryStore::open(&fixture.path()).unwrap());
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let path = fixture.path();
                scope.spawn(move || {
                    HistoryStore::open(&path)
                        .unwrap()
                        .append("local", &[sample(1, 20.0)])
                        .unwrap();
                });
            }
        });
        let mut store = HistoryStore::open(&fixture.path()).unwrap();
        assert_eq!(store.read("local", None, 100).unwrap().len(), 1);
        store.append("other-owner", &[sample(1, 80.0)]).unwrap();
        assert_eq!(store.read("other-owner", None, 100).unwrap()[0].used, 80.0);
        assert_eq!(store.read("local", None, 100).unwrap()[0].used, 20.0);
        assert!(store.read("local", Some("nous"), 100).unwrap().is_empty());
    }
    #[test]
    fn failed_import_rolls_back_samples_and_marker_then_retries_once() {
        let fixture = Fixture::new();
        let mut store = HistoryStore::open(&fixture.path()).unwrap();
        let invalid = sample(2, f64::NAN);
        assert!(store
            .import_once("local", "legacy", &[sample(1, 10.0), invalid])
            .is_err());
        assert!(store.read("local", None, 100).unwrap().is_empty());
        assert!(!store.imported("local", "legacy").unwrap());
        assert_eq!(
            store
                .import_once("local", "legacy", &[sample(1, 10.0), sample(2, 15.0)])
                .unwrap(),
            2
        );
        assert_eq!(
            store
                .import_once("local", "legacy", &[sample(3, 20.0)])
                .unwrap(),
            0
        );
        assert_eq!(store.read("local", None, 100).unwrap().len(), 2);
    }
    #[test]
    fn reset_events_survive_reopen_and_retention_keeps_last_observation() {
        let fixture = Fixture::new();
        let mut store = HistoryStore::open(&fixture.path()).unwrap();
        store.append("local", &[sample(1, 90.0)]).unwrap();
        let mut reset = sample(2, 1.0);
        reset.resets_at = Some("2030-01-08".into());
        store.append("local", &[reset]).unwrap();
        assert_eq!(
            store.read("local", None, 100).unwrap()[1].event.as_deref(),
            Some("reset")
        );
        assert_eq!(store.prune("local", 100).unwrap(), 1);
        drop(store);
        let store = HistoryStore::open(&fixture.path()).unwrap();
        let rows = store.read("local", None, 100).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ts_ms, 2);
    }
}
