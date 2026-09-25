//! Append-only JSONL ledger. Callers pass the file path (no XDG).

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use fabrials_model::UsageRecord;

/// Append one usage record. Skips duplicate `request_id` already in the file.
pub fn append(path: &Path, record: &UsageRecord) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir ledger dir: {e}"))?;
    }
    if let Some(rid) = record.request_id.as_deref() {
        if !rid.is_empty() && recent_has_request_id(path, rid) {
            return Ok(());
        }
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open ledger: {e}"))?;
    let line = serde_json::to_string(record).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| format!("write ledger: {e}"))?;
    Ok(())
}

fn recent_has_request_id(path: &Path, rid: &str) -> bool {
    let Ok(f) = std::fs::File::open(path) else {
        return false;
    };
    let needle = format!("\"request_id\":\"{rid}\"");
    BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.contains(&needle))
}

pub fn read_all(path: &Path) -> Vec<UsageRecord> {
    let Ok(f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<UsageRecord>(line) {
            out.push(rec);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabrials_model::UsageRecord;

    #[test]
    fn append_roundtrip_and_dedup_request_id() {
        let dir =
            std::env::temp_dir().join(format!("fabrials-metrics-ledger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grok-usage.jsonl");
        let rec = UsageRecord {
            ts_ms: 42,
            model: Some("grok-4.5".into()),
            input_tokens: 10,
            output_tokens: 2,
            request_id: Some("resp_dup".into()),
            account_id: Some("grok/heavy".into()),
            route: Some("grok".into()),
            ..UsageRecord::default()
        };
        append(&path, &rec).unwrap();
        append(&path, &rec).unwrap();
        let all = read_all(&path);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].account_id.as_deref(), Some("grok/heavy"));
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("access_token"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
