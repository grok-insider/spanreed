//! The SQLite stores satisfy the engine's storage ports behind trait objects.

use fabrials_fabric::ports::{HistoryStore, HopStore};
use fabrials_store_sqlite::{SqliteHistoryStore, SqliteHopStore};
use fabrials_types::{HistorySample, HopRecord};

#[test]
fn hosts_can_hold_the_stores_as_ports() {
    let dir = std::env::temp_dir().join(format!(
        "fabrials-store-ports-{}",
        fabrials_fabric::accounting::new_request_id()
    ));
    let path = dir.join("store.sqlite3");
    let mut hops: Box<dyn HopStore> = Box::new(SqliteHopStore::open(&path).unwrap());
    let record = HopRecord {
        ts_ms: 10,
        request_id: Some("req-1".into()),
        provider: Some("grok".into()),
        ..HopRecord::default()
    };
    assert!(hops.append("local", &record).unwrap());
    assert!(!hops.append("local", &record).unwrap());
    assert_eq!(hops.recent("local", 10).unwrap().len(), 1);
    assert_eq!(
        hops.read_window("local", Some("grok"), 0, 20)
            .unwrap()
            .len(),
        1
    );

    let mut history: Box<dyn HistoryStore> = Box::new(SqliteHistoryStore::open(&path).unwrap());
    let sample = HistorySample {
        ts_ms: 10,
        provider: "grok".into(),
        plan: None,
        label: "Weekly".into(),
        used: 5.0,
        limit: 100.0,
        resets_at: None,
        window_start_ms: None,
        limit_window_secs: None,
        kind: "percent".into(),
        event: None,
    };
    assert_eq!(history.append("local", &[sample]).unwrap(), 1);
    assert_eq!(history.read("local", None, 10).unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}
