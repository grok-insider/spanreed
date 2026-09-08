//! Durable outbox and separate downloaded history. Imported records are never re-uploaded.
use fabrials_model::private_sync::{PrivateObservation, PrivatePage};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

pub struct Store {
    connection: Connection,
}
impl Store {
    pub fn open() -> Result<Self, String> {
        crate::grok_ledger::ensure_store()?;
        crate::history::local_samples(None, 1)?;
        let connection = Connection::open(crate::app::data_dir().join("runtime.sqlite3"))
            .map_err(|e| e.to_string())?;
        let mut store = Self::from_connection(connection)?;
        store.initialize_scan()?;
        Ok(store)
    }
    fn from_connection(connection: Connection) -> Result<Self, String> {
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS private_sync_outbox(owner TEXT NOT NULL,id TEXT NOT NULL,source TEXT NOT NULL,document TEXT NOT NULL,acked INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(owner,id));
        CREATE TABLE IF NOT EXISTS private_sync_inbox(owner TEXT NOT NULL,cursor INTEGER NOT NULL,device TEXT NOT NULL,document TEXT NOT NULL,PRIMARY KEY(owner,cursor));
        CREATE TABLE IF NOT EXISTS private_sync_progress(owner TEXT NOT NULL,stream TEXT NOT NULL,cursor INTEGER NOT NULL,PRIMARY KEY(owner,stream));").map_err(|e|e.to_string())?;
        Ok(Self { connection })
    }
    pub fn enqueue(&self, owner: &str, observation: &PrivateObservation) -> Result<(), String> {
        Self::enqueue_on(&self.connection, owner, observation)
    }
    fn enqueue_on(
        connection: &Connection,
        owner: &str,
        observation: &PrivateObservation,
    ) -> Result<(), String> {
        let document =
            serde_json::to_string(observation).map_err(|_| "Invalid sync observation")?;
        if document.len() > 60_000 {
            return Err("A sync observation exceeds the upload limit".into());
        }
        let id = Sha256::digest(document.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        connection.execute("INSERT OR IGNORE INTO private_sync_outbox(owner,id,source,document) VALUES(?1,?2,?3,?4)",params![owner,id,observation.source,document]).map_err(|e|e.to_string())?;
        Ok(())
    }
    fn initialize_scan(&mut self) -> Result<(), String> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS private_sync_changes(seq INTEGER PRIMARY KEY AUTOINCREMENT,stream TEXT NOT NULL,document TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS private_sync_meta(key TEXT PRIMARY KEY);
        CREATE TRIGGER IF NOT EXISTS private_sync_hop_insert AFTER INSERT ON usage_hops WHEN NEW.namespace='local' BEGIN INSERT INTO private_sync_changes(stream,document) VALUES('hop',NEW.document); END;
        CREATE TRIGGER IF NOT EXISTS private_sync_hop_update AFTER UPDATE OF document ON usage_hops WHEN NEW.namespace='local' AND OLD.document<>NEW.document BEGIN INSERT INTO private_sync_changes(stream,document) VALUES('hop',NEW.document); END;
        CREATE TRIGGER IF NOT EXISTS private_sync_history_insert AFTER INSERT ON usage_history WHEN NEW.namespace='local' BEGIN INSERT INTO private_sync_changes(stream,document) VALUES('history',NEW.document); END;
        CREATE TRIGGER IF NOT EXISTS private_sync_history_update AFTER UPDATE OF document ON usage_history WHEN NEW.namespace='local' AND OLD.document<>NEW.document BEGIN INSERT INTO private_sync_changes(stream,document) VALUES('history',NEW.document); END;").map_err(|e|e.to_string())?;
        let seeded: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM private_sync_meta WHERE key='seeded')",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !seeded {
            tx.execute_batch("INSERT INTO private_sync_changes(stream,document) SELECT 'hop',document FROM usage_hops WHERE namespace='local' ORDER BY ts_ms;
            INSERT INTO private_sync_changes(stream,document) SELECT 'history',document FROM usage_history WHERE namespace='local' ORDER BY seq;
            INSERT INTO private_sync_meta(key) VALUES('seeded');").map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn scan_page(&mut self, owner: &str, sources: &[String]) -> Result<bool, String> {
        use fabrials_model::private_sync::PrivateEvent;
        let mut sources = sources.to_vec();
        sources.sort();
        sources.dedup();
        let selection = serde_json::to_vec(&sources).map_err(|_| "Invalid sync sources")?;
        let stream = format!("scan:{:x}", Sha256::digest(selection));
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let cursor: i64 = tx
            .query_row(
                "SELECT cursor FROM private_sync_progress WHERE owner=?1 AND stream=?2",
                params![owner, stream],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or(0);
        let entries = {
            let mut query=tx.prepare("SELECT seq,stream,document FROM private_sync_changes WHERE seq>?1 ORDER BY seq LIMIT 500").map_err(|e|e.to_string())?;
            let rows = query
                .query_map(params![cursor], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        for (_, kind, document) in &entries {
            let observation = if kind == "hop" {
                let mut record: fabrials_model::UsageRecord = serde_json::from_str(document)
                    .map_err(|_| "Invalid captured request in sync scan")?;
                let Some(source) = record
                    .account_id
                    .clone()
                    .or(record.provider.clone())
                    .or(record.route.clone())
                else {
                    continue;
                };
                record.key_hash = None;
                PrivateObservation {
                    source,
                    event: PrivateEvent::Request { record },
                }
            } else {
                let sample: fabrials_model::HistorySample = serde_json::from_str(document)
                    .map_err(|_| "Invalid quota history in sync scan")?;
                PrivateObservation {
                    source: sample.provider.clone(),
                    event: PrivateEvent::History { sample },
                }
            };
            if sources.contains(&observation.source) {
                Self::enqueue_on(&tx, owner, &observation)?;
            }
        }
        if let Some((cursor, _, _)) = entries.last() {
            tx.execute("INSERT INTO private_sync_progress(owner,stream,cursor) VALUES(?1,?2,?3) ON CONFLICT(owner,stream) DO UPDATE SET cursor=excluded.cursor",params![owner,stream,cursor]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(entries.len() == 500)
    }
    pub fn pending(
        &self,
        owner: &str,
        sources: &[String],
    ) -> Result<Vec<(String, PrivateObservation)>, String> {
        let mut query=self.connection.prepare("SELECT id,source,document FROM private_sync_outbox WHERE owner=?1 AND acked=0 ORDER BY rowid").map_err(|e|e.to_string())?;
        let mut rows = query.query(params![owner]).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        let mut size = 0;
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let source: String = row.get(1).map_err(|e| e.to_string())?;
            if !sources.contains(&source) {
                continue;
            }
            let document: String = row.get(2).map_err(|e| e.to_string())?;
            if size + document.len() > 60_000 || result.len() == 200 {
                break;
            }
            size += document.len();
            result.push((
                row.get(0).map_err(|e| e.to_string())?,
                serde_json::from_str(&document).map_err(|_| "Invalid queued sync observation")?,
            ));
        }
        Ok(result)
    }
    pub fn acknowledge(&mut self, owner: &str, ids: &[String]) -> Result<(), String> {
        let tx = self.connection.transaction().map_err(|e| e.to_string())?;
        for id in ids {
            tx.execute(
                "UPDATE private_sync_outbox SET acked=1 WHERE owner=?1 AND id=?2",
                params![owner, id],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn cursor(&self, owner: &str) -> Result<i64, String> {
        self.connection
            .query_row(
                "SELECT cursor FROM private_sync_progress WHERE owner=?1 AND stream='pull'",
                params![owner],
                |row| row.get(0),
            )
            .optional()
            .map(|value| value.unwrap_or(0))
            .map_err(|e| e.to_string())
    }
    pub fn recent(
        &self,
        owner: &str,
        before: Option<i64>,
    ) -> Result<fabrials_model::private_sync::PrivateRecentPage, String> {
        let mut query = self.connection.prepare("SELECT cursor,device,document FROM private_sync_inbox WHERE owner=?1 AND cursor<?2 ORDER BY cursor DESC LIMIT 51").map_err(|e| e.to_string())?;
        let rows = query
            .query_map(params![owner, before.unwrap_or(i64::MAX)], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let has_more = rows.len() > 50;
        let observations = rows
            .into_iter()
            .take(50)
            .map(|(cursor, device, document)| {
                Ok(fabrials_model::private_sync::PrivateStoredObservation {
                    cursor,
                    device,
                    observation: serde_json::from_str(&document)
                        .map_err(|_| "Invalid downloaded observation")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(fabrials_model::private_sync::PrivateRecentPage {
            before: observations.last().map(|item| item.cursor),
            observations,
            has_more,
        })
    }
    pub fn import(&mut self, owner: &str, page: &PrivatePage) -> Result<(), String> {
        let old = self.cursor(owner)?;
        if page.next_cursor < old
            || page.observations.len() > 200
            || (page.has_more && page.next_cursor == old)
        {
            return Err("Invalid private history cursor".into());
        }
        let mut previous = old;
        for item in &page.observations {
            if item.cursor <= previous || item.cursor > page.next_cursor {
                return Err("Private history is not ordered".into());
            }
            previous = item.cursor;
        }
        if page.next_cursor != previous {
            return Err("Private history cursor skipped observations".into());
        }
        let tx = self.connection.transaction().map_err(|e| e.to_string())?;
        for item in &page.observations {
            let document =
                serde_json::to_string(&item.observation).map_err(|_| "Invalid remote history")?;
            tx.execute("INSERT OR IGNORE INTO private_sync_inbox(owner,cursor,device,document) VALUES(?1,?2,?3,?4)",params![owner,item.cursor,item.device,document]).map_err(|e|e.to_string())?;
        }
        tx.execute("INSERT INTO private_sync_progress(owner,stream,cursor) VALUES(?1,'pull',?2) ON CONFLICT(owner,stream) DO UPDATE SET cursor=excluded.cursor",params![owner,page.next_cursor]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabrials_model::private_sync::{PrivateEvent, PrivateStoredObservation};
    fn observation(source: &str) -> PrivateObservation {
        PrivateObservation {
            source: source.into(),
            event: PrivateEvent::Quota {
                at_ms: 1000,
                output: fabrials_model::ProviderOutput::error(source, "Fixture", "No quota"),
            },
        }
    }
    #[test]
    fn restart_preserves_unacknowledged_work_and_paginated_private_history() {
        let path = std::env::temp_dir().join(format!(
            "spanreed-sync-restart-{}-{}.sqlite3",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = Store::from_connection(Connection::open(&path).unwrap()).unwrap();
        store.enqueue("alice", &observation("codex")).unwrap();
        let original = store.pending("alice", &["codex".into()]).unwrap()[0]
            .0
            .clone();
        let observations = (1..=111)
            .map(|cursor| PrivateStoredObservation {
                cursor,
                device: "fixture".into(),
                observation: observation("codex"),
            })
            .collect();
        store
            .import(
                "alice",
                &PrivatePage {
                    observations,
                    next_cursor: 111,
                    has_more: false,
                },
            )
            .unwrap();
        drop(store);
        let mut store = Store::from_connection(Connection::open(&path).unwrap()).unwrap();
        assert_eq!(store.cursor("alice").unwrap(), 111);
        assert_eq!(
            store.pending("alice", &["codex".into()]).unwrap()[0].0,
            original
        );
        let first = store.recent("alice", None).unwrap();
        assert_eq!(first.observations.len(), 50);
        assert_eq!(first.observations[0].cursor, 111);
        let second = store.recent("alice", first.before).unwrap();
        assert_eq!(second.observations[0].cursor, 61);
        let third = store.recent("alice", second.before).unwrap();
        assert_eq!(third.observations.len(), 11);
        assert!(!third.has_more);
        assert!(store.recent("bob", None).unwrap().observations.is_empty());
        store.acknowledge("alice", &[original]).unwrap();
        drop(store);
        let store = Store::from_connection(Connection::open(&path).unwrap()).unwrap();
        assert!(store
            .pending("alice", &["codex".into()])
            .unwrap()
            .is_empty());
        drop(store);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn incremental_scan_includes_large_history_late_records_and_new_selections() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE usage_hops(namespace TEXT,identity TEXT,ts_ms INTEGER,document TEXT); CREATE TABLE usage_history(seq INTEGER PRIMARY KEY,namespace TEXT,document TEXT);").unwrap();
        for index in 0..1251 {
            let record = fabrials_model::UsageRecord {
                ts_ms: index + 1,
                provider: Some("codex".into()),
                request_id: Some(format!("request-{index}")),
                ..Default::default()
            };
            connection
                .execute(
                    "INSERT INTO usage_hops VALUES('local',?1,?2,?3)",
                    params![
                        format!("request-{index}"),
                        index + 1,
                        serde_json::to_string(&record).unwrap()
                    ],
                )
                .unwrap();
        }
        let mut store = Store::from_connection(connection).unwrap();
        store.initialize_scan().unwrap();
        let selected = vec!["codex".into()];
        let mut pages = 1;
        while store.scan_page("alice", &selected).unwrap() {
            pages += 1;
        }
        assert_eq!(pages, 3);
        let count: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM private_sync_outbox WHERE owner='alice'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1251);
        let late = fabrials_model::UsageRecord {
            ts_ms: 1,
            provider: Some("nous".into()),
            request_id: Some("late".into()),
            ..Default::default()
        };
        store
            .connection
            .execute(
                "INSERT INTO usage_hops VALUES('local','late',1,?1)",
                params![serde_json::to_string(&late).unwrap()],
            )
            .unwrap();
        assert!(!store.scan_page("alice", &selected).unwrap());
        while store.scan_page("alice", &["nous".into()]).unwrap() {}
        assert_eq!(store.pending("alice", &["nous".into()]).unwrap().len(), 1);
        store.initialize_scan().unwrap();
        let changes: i64 = store
            .connection
            .query_row("SELECT count(*) FROM private_sync_changes", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(changes, 1252);
    }
    #[test]
    fn unacknowledged_batches_retry_and_deselected_sources_remain_private() {
        let mut store = Store::from_connection(Connection::open_in_memory().unwrap()).unwrap();
        store.enqueue("alice", &observation("codex")).unwrap();
        store
            .enqueue("alice", &observation("grok/private"))
            .unwrap();
        store.enqueue("alice", &observation("codex")).unwrap();
        let selected = vec!["codex".into()];
        let first = store.pending("alice", &selected).unwrap();
        assert_eq!(first.len(), 1);
        assert!(store.pending("mallory", &selected).unwrap().is_empty());
        assert_eq!(store.pending("alice", &selected).unwrap()[0].0, first[0].0);
        store.acknowledge("mallory", &[first[0].0.clone()]).unwrap();
        assert_eq!(store.pending("alice", &selected).unwrap().len(), 1);
        store.acknowledge("alice", &[first[0].0.clone()]).unwrap();
        assert!(store.pending("alice", &selected).unwrap().is_empty());
        assert_eq!(
            store
                .pending("alice", &["grok/private".into()])
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn downloaded_pages_and_cursors_commit_together_without_reuploading() {
        let mut store = Store::from_connection(Connection::open_in_memory().unwrap()).unwrap();
        let mut page = PrivatePage {
            observations: vec![PrivateStoredObservation {
                cursor: 10,
                device: "other-device".into(),
                observation: observation("codex"),
            }],
            next_cursor: 11,
            has_more: false,
        };
        assert!(store.import("alice", &page).is_err());
        assert_eq!(store.cursor("alice").unwrap(), 0);
        page.next_cursor = 10;
        store.import("alice", &page).unwrap();
        assert_eq!(store.cursor("alice").unwrap(), 10);
        assert_eq!(store.cursor("mallory").unwrap(), 0);
        assert!(store
            .pending("alice", &["codex".into()])
            .unwrap()
            .is_empty());
        assert!(store.import("alice", &page).is_err());
        assert_eq!(store.cursor("alice").unwrap(), 10);
    }
}
