//! Atomic source checkpoints and corrected consumption projections.
use fabrials_core::usage::{ImportCheckpoint, UsageFilter, UsageRecord};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct UsageStore(Connection);

#[derive(Debug, Clone)]
pub struct StoredSource {
    pub parser_version: u32,
    pub fingerprint: String,
    pub generation: u64,
    pub checkpoint: ImportCheckpoint,
    pub last_success_ms: i64,
}

pub struct ImportBatch<'a> {
    pub source: &'a str,
    pub client: &'a str,
    pub parser_version: u32,
    pub fingerprint: &'a str,
    pub expected_generation: Option<u64>,
    pub replace: bool,
    pub checkpoint: &'a ImportCheckpoint,
    pub records: &'a [UsageRecord],
    pub at_ms: i64,
}

impl UsageStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::initialize(crate::database::open(path)?)
    }

    fn initialize(connection: Connection) -> Result<Self, String> {
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS consumption_sources (
                source TEXT PRIMARY KEY, client TEXT NOT NULL, parser_version INTEGER NOT NULL,
                fingerprint TEXT NOT NULL, generation INTEGER NOT NULL, checkpoint TEXT NOT NULL,
                observed_at_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS consumption_records (
                source TEXT NOT NULL, identity TEXT NOT NULL, client TEXT NOT NULL,
                origin TEXT NOT NULL, at_ms INTEGER NOT NULL, record TEXT NOT NULL,
                PRIMARY KEY(source, identity));
             CREATE INDEX IF NOT EXISTS consumption_time ON consumption_records(client, at_ms);
             CREATE TABLE IF NOT EXISTS consumption_exports (
                scope TEXT PRIMARY KEY, digest TEXT NOT NULL, revision INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS consumption_revision (singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL);
             INSERT OR IGNORE INTO consumption_revision VALUES(1, 0);"
        ).map_err(|e| e.to_string())?;
        Ok(Self(connection))
    }

    pub fn source(&self, source: &str) -> Result<Option<StoredSource>, String> {
        let row: Option<(u32, String, u64, String, i64)> = self.0.query_row(
            "SELECT parser_version, fingerprint, generation, checkpoint, observed_at_ms FROM consumption_sources WHERE source=?1",
            [source], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).optional().map_err(|e| e.to_string())?;
        row.map(
            |(parser_version, fingerprint, generation, checkpoint, last_success_ms)| {
                Ok(StoredSource {
                    parser_version,
                    fingerprint,
                    generation,
                    last_success_ms,
                    checkpoint: serde_json::from_str(&checkpoint).map_err(|e| e.to_string())?,
                })
            },
        )
        .transpose()
    }

    pub fn revision(&self) -> Result<u64, String> {
        self.0
            .query_row(
                "SELECT revision FROM consumption_revision WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())
    }

    pub fn record_count(&self, client: &str) -> Result<u64, String> {
        self.0.query_row("SELECT COUNT(*) FROM (SELECT origin,identity FROM consumption_records WHERE client=?1 GROUP BY origin,identity)", [client], |row| row.get(0)).map_err(|e|e.to_string())
    }

    pub fn source_ids(&self, client: &str) -> Result<Vec<String>, String> {
        let mut statement = self
            .0
            .prepare("SELECT source FROM consumption_sources WHERE client=?1 ORDER BY source")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([client], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn import(&mut self, batch: ImportBatch<'_>) -> Result<u64, String> {
        if batch.source.is_empty() || batch.client.is_empty() {
            return Err("Missing import source".into());
        }
        let mut identities = std::collections::HashSet::new();
        let encoded: Vec<_> = batch
            .records
            .iter()
            .map(|record| {
                record.validate()?;
                if record.client != batch.client || !identities.insert(&record.id) {
                    return Err("Import contains a foreign client or duplicate identity".into());
                }
                serde_json::to_string(record).map_err(|e| e.to_string())
            })
            .collect::<Result<_, _>>()?;
        let checkpoint = serde_json::to_string(batch.checkpoint).map_err(|e| e.to_string())?;
        let tx = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let current: Option<u64> = tx
            .query_row(
                "SELECT generation FROM consumption_sources WHERE source=?1",
                [batch.source],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if current != batch.expected_generation {
            return Err("Source changed during import; retry with its latest checkpoint".into());
        }
        let mut changed = false;
        if batch.replace {
            let mut statement = tx
                .prepare("SELECT identity, record FROM consumption_records WHERE source=?1")
                .map_err(|e| e.to_string())?;
            let old = statement
                .query_map([batch.source], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<std::collections::BTreeMap<_, _>, _>>()
                .map_err(|e| e.to_string())?;
            let new: std::collections::BTreeMap<_, _> = batch
                .records
                .iter()
                .zip(&encoded)
                .map(|(r, json)| (r.id.clone(), json.clone()))
                .collect();
            changed = old != new;
            drop(statement);
            if changed {
                tx.execute(
                    "DELETE FROM consumption_records WHERE source=?1",
                    [batch.source],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        if !batch.replace || changed {
            for (record, json) in batch.records.iter().zip(&encoded) {
                let origin = serde_json::to_string(&record.origin).map_err(|e| e.to_string())?;
                let count = tx.execute(
                    "INSERT INTO consumption_records(source,identity,client,origin,at_ms,record) VALUES(?1,?2,?3,?4,?5,?6)
                     ON CONFLICT(source,identity) DO UPDATE SET record=excluded.record,at_ms=excluded.at_ms,origin=excluded.origin
                     WHERE consumption_records.record != excluded.record",
                    params![batch.source, record.id, record.client, origin, record.at_ms, json],
                ).map_err(|e| e.to_string())?;
                changed |= count != 0;
            }
        }
        tx.execute(
            "INSERT INTO consumption_sources VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(source) DO UPDATE SET client=excluded.client,parser_version=excluded.parser_version,
             fingerprint=excluded.fingerprint,generation=excluded.generation,checkpoint=excluded.checkpoint,observed_at_ms=excluded.observed_at_ms",
            params![batch.source, batch.client, batch.parser_version, batch.fingerprint, current.unwrap_or(0) + 1, checkpoint, batch.at_ms],
        ).map_err(|e| e.to_string())?;
        if changed {
            tx.execute(
                "UPDATE consumption_revision SET revision=revision+1 WHERE singleton=1",
                [],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        self.revision()
    }

    /// A projection revision changes for corrections, repricing and window expiry.
    pub fn export_revision(&mut self, scope: &str, digest: &str) -> Result<u64, String> {
        self.export_revision_after(scope, digest, 0)
    }

    pub fn export_revision_after(
        &mut self,
        scope: &str,
        digest: &str,
        floor: u64,
    ) -> Result<u64, String> {
        let next = floor
            .checked_add(1)
            .filter(|n| *n <= i64::MAX as u64)
            .ok_or("Usage revision exhausted")?;
        let tx = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO consumption_exports(scope,digest,revision) VALUES(?1,?2,?3)
            ON CONFLICT(scope) DO UPDATE SET digest=excluded.digest,revision=MAX(consumption_exports.revision+1,excluded.revision)
            WHERE consumption_exports.digest!=excluded.digest OR consumption_exports.revision<?4",params![scope,digest,next,floor]).map_err(|e|e.to_string())?;
        let revision = tx
            .query_row(
                "SELECT revision FROM consumption_exports WHERE scope=?1",
                [scope],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(revision)
    }

    pub fn records(&self, filter: &UsageFilter) -> Result<Vec<UsageRecord>, String> {
        // A moved/copied rollout can appear under multiple roots. Deduplicate by
        // parser-provided identity, never by equal timestamps or token counts.
        let mut statement = self.0.prepare(
            "SELECT record FROM (
                SELECT r.record,r.at_ms,r.client,ROW_NUMBER() OVER (
                    PARTITION BY r.client,r.origin,r.identity ORDER BY s.observed_at_ms DESC,r.source
                ) AS rank FROM consumption_records r JOIN consumption_sources s USING(source)
             ) WHERE rank=1 AND (?1 IS NULL OR client=?1) AND (?2 IS NULL OR at_ms>=?2)
               AND (?3 IS NULL OR at_ms<?3) ORDER BY at_ms,record"
        ).map_err(|e| e.to_string())?;
        let rows = statement
            .query_map(
                params![filter.client, filter.since_ms, filter.until_ms],
                |r| r.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for row in rows {
            let record: UsageRecord = serde_json::from_str(&row.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if [
                (&filter.provider, &record.provider),
                (&filter.account, &record.account),
                (&filter.model, &record.model),
                (&filter.session, &record.session),
                (&filter.project, &record.project),
            ]
            .iter()
            .all(|(wanted, value)| wanted.is_none() || wanted == value)
            {
                result.push(record);
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabrials_core::usage::{Granularity, Tokens, UsageOrigin};
    fn record(id: &str, tokens: u64) -> UsageRecord {
        UsageRecord {
            id: id.into(),
            client: "codex".into(),
            provider: Some("openai".into()),
            account: None,
            session: Some("session".into()),
            project: None,
            model: None,
            service_tier: None,
            at_ms: 100,
            timestamp_inferred: false,
            period_end_ms: None,
            origin: UsageOrigin::LocalLog,
            granularity: Granularity::Request,
            tokens: Some(Tokens {
                input: tokens,
                ..Tokens::default()
            }),
            cost: None,
            request_id: None,
        }
    }
    fn apply(store: &mut UsageStore, source: &str, records: &[UsageRecord]) -> u64 {
        let generation = store.source(source).unwrap().map(|s| s.generation);
        store
            .import(ImportBatch {
                source,
                client: "codex",
                parser_version: 1,
                fingerprint: "hash",
                expected_generation: generation,
                replace: true,
                checkpoint: &ImportCheckpoint::default(),
                records,
                at_ms: 1,
            })
            .unwrap()
    }
    #[test]
    fn rebuilds_replace_totals_and_clear_to_zero() {
        let mut store = UsageStore::initialize(Connection::open_in_memory().unwrap()).unwrap();
        assert_eq!(apply(&mut store, "a", &[record("one", 100)]), 1);
        assert_eq!(apply(&mut store, "a", &[record("one", 100)]), 1);
        assert_eq!(apply(&mut store, "a", &[record("one", 70)]), 2);
        assert_eq!(
            store.records(&UsageFilter::default()).unwrap()[0]
                .tokens
                .as_ref()
                .unwrap()
                .input,
            70
        );
        assert_eq!(apply(&mut store, "a", &[]), 3);
        assert!(store.records(&UsageFilter::default()).unwrap().is_empty());
    }
    #[test]
    fn overlapping_roots_dedup_but_distinct_requests_do_not() {
        let mut store = UsageStore::initialize(Connection::open_in_memory().unwrap()).unwrap();
        apply(&mut store, "a", &[record("one", 100)]);
        apply(
            &mut store,
            "archive",
            &[record("one", 100), record("two", 100)],
        );
        assert_eq!(store.records(&UsageFilter::default()).unwrap().len(), 2);
        apply(&mut store, "a", &[]);
        assert_eq!(store.records(&UsageFilter::default()).unwrap().len(), 2);
    }
    #[test]
    fn stale_import_cannot_replace_a_newer_checkpoint() {
        let mut store = UsageStore::initialize(Connection::open_in_memory().unwrap()).unwrap();
        apply(&mut store, "a", &[record("one", 100)]);
        assert!(store
            .import(ImportBatch {
                source: "a",
                client: "codex",
                parser_version: 1,
                fingerprint: "old",
                expected_generation: None,
                replace: true,
                checkpoint: &ImportCheckpoint::default(),
                records: &[],
                at_ms: 0
            })
            .is_err());
        assert_eq!(store.records(&UsageFilter::default()).unwrap().len(), 1);
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;
    #[test]
    fn projection_revisions_are_idempotent_and_owner_scoped() {
        let mut store = UsageStore::initialize(Connection::open_in_memory().unwrap()).unwrap();
        assert_eq!(
            store
                .export_revision("owner/device/codex", "first")
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .export_revision("owner/device/codex", "first")
                .unwrap(),
            1
        );
        assert_eq!(
            store.export_revision("owner/device/codex", "zero").unwrap(),
            2
        );
        assert_eq!(
            store.export_revision("other/device/codex", "zero").unwrap(),
            1
        );
    }
    #[test]
    fn projection_recovers_after_database_rebuild() {
        let mut store = UsageStore::initialize(Connection::open_in_memory().unwrap()).unwrap();
        assert_eq!(
            store
                .export_revision_after("owner/device/codex", "zero", 40)
                .unwrap(),
            41
        );
        assert_eq!(
            store
                .export_revision_after("owner/device/codex", "zero", 41)
                .unwrap(),
            41
        );
        assert_eq!(
            store
                .export_revision_after("owner/device/codex", "corrected", 41)
                .unwrap(),
            42
        );
        assert!(store
            .export_revision_after("owner/device/codex", "bad", u64::MAX)
            .is_err());
    }
}
