use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const MAX_RECEIPTS: usize = 512;
const MAX_RETIRED: usize = 65_536;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub idempotency_key: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing, default)]
    pub fingerprint: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredReceipt {
    receipt: Receipt,
    fingerprint: String,
    #[serde(default)]
    sequence: u64,
}

#[derive(Deserialize)]
struct DiskLedger {
    version: u32,
    entries: BTreeMap<String, StoredReceipt>,
    retired: BTreeMap<String, String>,
    next_sequence: u64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Disk {
    Current(DiskLedger),
    Legacy(BTreeMap<String, StoredReceipt>),
}

#[derive(Debug, Default)]
pub struct Receipts {
    entries: BTreeMap<String, StoredReceipt>,
    retired: BTreeMap<String, String>,
    next_sequence: u64,
    path: Option<PathBuf>,
}

pub fn scoped_key(principal: &str, key: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(principal.as_bytes());
    hash.update([0]);
    hash.update(key.as_bytes());
    format!("remote-{:x}", hash.finalize())
}

impl Receipts {
    pub fn open(directory: &Path) -> std::io::Result<Self> {
        let path = directory.join("command-receipts.json");
        let mut ledger = Self {
            path: Some(path.clone()),
            ..Self::default()
        };
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.len() > MAX_FILE_BYTES => {
                return Err(std::io::ErrorKind::InvalidData.into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(ledger),
            Err(error) => return Err(error),
        }
        match std::fs::read(&path) {
            Ok(bytes) => {
                if bytes.len() as u64 > MAX_FILE_BYTES {
                    return Err(std::io::ErrorKind::InvalidData.into());
                }
                match serde_json::from_slice(&bytes)? {
                    Disk::Current(disk) if disk.version == 2 => {
                        ledger.entries = disk.entries;
                        ledger.retired = disk.retired;
                        ledger.next_sequence = disk.next_sequence;
                    }
                    Disk::Current(_) => return Err(std::io::ErrorKind::InvalidData.into()),
                    Disk::Legacy(entries) => {
                        ledger.entries = entries;
                        for stored in ledger.entries.values_mut() {
                            stored.sequence = ledger.next_sequence;
                            ledger.next_sequence += 1;
                        }
                    }
                }
                if ledger.entries.len() > MAX_RECEIPTS
                    || ledger.retired.len() > MAX_RETIRED
                    || ledger.entries.iter().any(|(key, stored)| {
                        ledger.retired.contains_key(key) || stored.sequence >= ledger.next_sequence
                    })
                {
                    return Err(std::io::ErrorKind::InvalidData.into());
                }
                for stored in ledger.entries.values_mut() {
                    stored.receipt.fingerprint = stored.fingerprint.clone();
                    if stored.receipt.state == "running" {
                        stored.receipt.state = "needsReview".into();
                        stored.receipt.error = Some("host_restarted".into());
                    }
                }
                ledger.persist()?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Ok(ledger)
    }

    pub fn get(&self, key: &str) -> Option<Receipt> {
        self.entries
            .get(key)
            .map(|stored| stored.receipt.clone())
            .or_else(|| {
                self.retired.get(key).map(|fingerprint| Receipt {
                    idempotency_key: String::new(),
                    state: "needsReview".into(),
                    result: None,
                    error: Some("receipt_expired".into()),
                    fingerprint: fingerprint.clone(),
                })
            })
    }

    pub fn begin(
        &mut self,
        key: String,
        idempotency_key: String,
        operation: &Value,
    ) -> Result<Receipt, &'static str> {
        let encoded = serde_json::to_vec(operation).map_err(|_| "invalid_command")?;
        let fingerprint = format!("{:x}", Sha256::digest(encoded));
        if let Some(mut previous) = self.get(&key) {
            return if previous.fingerprint == fingerprint {
                previous.idempotency_key = idempotency_key;
                Ok(previous)
            } else {
                Err("idempotency_conflict")
            };
        }
        let next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or("command_history_full")?;
        let retired_key = if self.entries.len() >= MAX_RECEIPTS {
            if self.retired.len() >= MAX_RETIRED {
                return Err("command_history_full");
            }
            Some(
                self.entries
                    .iter()
                    .filter(|(_, stored)| {
                        matches!(stored.receipt.state.as_str(), "completed" | "failed")
                    })
                    .min_by_key(|(_, stored)| stored.sequence)
                    .map(|(key, _)| key.clone())
                    .ok_or("command_history_full")?,
            )
        } else {
            None
        };
        let retired = retired_key.map(|key| {
            let stored = self.entries.remove(&key).expect("selected receipt exists");
            self.retired.insert(key.clone(), stored.fingerprint.clone());
            (key, stored)
        });
        let receipt = Receipt {
            idempotency_key,
            state: "running".into(),
            result: None,
            error: None,
            fingerprint: fingerprint.clone(),
        };
        self.entries.insert(
            key.clone(),
            StoredReceipt {
                receipt: receipt.clone(),
                fingerprint,
                sequence: self.next_sequence,
            },
        );
        let old_sequence = self.next_sequence;
        self.next_sequence = next_sequence;
        if self.persist().is_err() {
            self.entries.remove(&key);
            self.next_sequence = old_sequence;
            if let Some((key, stored)) = retired {
                self.retired.remove(&key);
                self.entries.insert(key, stored);
            }
            return Err("intent_not_durable");
        }
        Ok(receipt)
    }

    pub fn finish(
        &mut self,
        key: &str,
        result: Option<Value>,
        error: Option<String>,
        uncertain: bool,
    ) -> Result<Receipt, &'static str> {
        let stored = self.entries.get_mut(key).ok_or("unknown_command")?;
        stored.receipt.state = if uncertain {
            "needsReview"
        } else if error.is_some() {
            "failed"
        } else {
            "completed"
        }
        .into();
        stored.receipt.result = result;
        stored.receipt.error = error;
        let receipt = stored.receipt.clone();
        if self.persist().is_err() {
            let stored = self.entries.get_mut(key).ok_or("unknown_command")?;
            stored.receipt.state = "needsReview".into();
            stored.receipt.result = None;
            stored.receipt.error = Some("result_not_durable".into());
            return Err("result_not_durable");
        }
        Ok(receipt)
    }

    fn persist(&self) -> std::io::Result<()> {
        #[derive(Serialize)]
        struct Persisted<'a> {
            version: u32,
            entries: &'a BTreeMap<String, StoredReceipt>,
            retired: &'a BTreeMap<String, String>,
            next_sequence: u64,
        }
        let bytes = serde_json::to_vec(&Persisted {
            version: 2,
            entries: &self.entries,
            retired: &self.retired,
            next_sequence: self.next_sequence,
        })?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        let temporary = path.with_extension("tmp");
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.flush()?;
        file.sync_all()?;
        #[cfg(windows)]
        crate::win_acl::set_owner_only(&temporary)?;
        std::fs::rename(&temporary, path)?;
        #[cfg(unix)]
        std::fs::File::open(path.parent().ok_or(std::io::ErrorKind::InvalidInput)?)?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_preserves_results_and_never_replays_running_commands() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger = Receipts::open(directory.path()).unwrap();
        let op = serde_json::json!({"kind": "prompt", "text": "hello"});
        ledger.begin("a".into(), "a".into(), &op).unwrap();
        ledger.begin("b".into(), "b".into(), &op).unwrap();
        ledger
            .finish(
                "a",
                Some(serde_json::json!({"outcome": "promptAccepted"})),
                None,
                false,
            )
            .unwrap();
        let mut loaded = Receipts::open(directory.path()).unwrap();
        assert_eq!(loaded.get("a").unwrap().state, "completed");
        assert_eq!(
            loaded.begin("b".into(), "b".into(), &op).unwrap().state,
            "needsReview"
        );
        assert_eq!(
            loaded
                .begin("a".into(), "a".into(), &serde_json::json!({}))
                .unwrap_err(),
            "idempotency_conflict"
        );
        assert!(
            !std::fs::read_to_string(directory.path().join("command-receipts.json"))
                .unwrap()
                .contains("hello")
        );
    }

    #[test]
    fn expired_capacity_does_not_drop_deduplication() {
        let mut ledger = Receipts::default();
        for i in 0..512 {
            ledger
                .begin(i.to_string(), i.to_string(), &Value::Null)
                .unwrap();
        }
        assert_eq!(
            ledger
                .begin("new".into(), "new".into(), &Value::Null)
                .unwrap_err(),
            "command_history_full"
        );
        assert_eq!(
            ledger
                .begin("0".into(), "0".into(), &Value::Null)
                .unwrap()
                .state,
            "running"
        );
    }

    #[test]
    fn principal_scopes_idempotency() {
        assert_ne!(scoped_key("a", "same"), scoped_key("b", "same"));
    }

    #[test]
    fn completed_history_rotates_chronologically_without_reexecution_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger = Receipts::default();
        for i in 0..600 {
            let key = (1000 - i).to_string();
            ledger
                .begin(key.clone(), key.clone(), &Value::Null)
                .unwrap();
            ledger
                .finish(&key, Some(Value::Bool(true)), None, false)
                .unwrap();
        }
        assert_eq!(ledger.entries.len(), MAX_RECEIPTS);
        assert_eq!(ledger.retired.len(), 88);
        assert_eq!(
            ledger.get("1000").unwrap().error.as_deref(),
            Some("receipt_expired")
        );
        assert_eq!(ledger.get("401").unwrap().state, "completed");
        ledger.path = Some(directory.path().join("command-receipts.json"));
        ledger.persist().unwrap();
        let mut loaded = Receipts::open(directory.path()).unwrap();
        let old = loaded
            .begin("1000".into(), "original-id".into(), &Value::Null)
            .unwrap();
        assert_eq!(old.idempotency_key, "original-id");
        assert_eq!(old.state, "needsReview");
        assert_eq!(old.error.as_deref(), Some("receipt_expired"));
        assert_eq!(
            loaded
                .begin("1000".into(), "original-id".into(), &Value::Bool(false))
                .unwrap_err(),
            "idempotency_conflict"
        );
    }

    #[test]
    fn legacy_bare_map_is_migrated_without_losing_deduplication() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger = Receipts::default();
        ledger.begin("a".into(), "a".into(), &Value::Null).unwrap();
        let mut legacy = serde_json::to_value(&ledger.entries).unwrap();
        legacy["a"].as_object_mut().unwrap().remove("sequence");
        std::fs::write(
            directory.path().join("command-receipts.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        let loaded = Receipts::open(directory.path()).unwrap();
        assert_eq!(
            loaded.get("a").unwrap().error.as_deref(),
            Some("host_restarted")
        );
        assert_eq!(loaded.next_sequence, 1);
    }

    #[test]
    fn uncertain_commands_are_never_retired_and_tombstone_capacity_fails_closed() {
        let mut ledger = Receipts::default();
        for i in 0..MAX_RECEIPTS {
            let key = i.to_string();
            ledger
                .begin(key.clone(), key.clone(), &Value::Null)
                .unwrap();
            ledger.finish(&key, None, None, true).unwrap();
        }
        assert_eq!(
            ledger
                .begin("new".into(), "new".into(), &Value::Null)
                .unwrap_err(),
            "command_history_full"
        );
        ledger.finish("0", None, None, false).unwrap();
        ledger.retired = (0..MAX_RETIRED)
            .map(|i| (format!("retired-{i}"), "digest".into()))
            .collect();
        assert_eq!(
            ledger
                .begin("new".into(), "new".into(), &Value::Null)
                .unwrap_err(),
            "command_history_full"
        );
        assert_eq!(ledger.get("0").unwrap().state, "completed");
    }

    #[test]
    fn failed_rotation_write_restores_the_full_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let mut ledger = Receipts::default();
        for i in 0..MAX_RECEIPTS {
            let key = i.to_string();
            ledger
                .begin(key.clone(), key.clone(), &Value::Null)
                .unwrap();
            ledger.finish(&key, None, None, false).unwrap();
        }
        ledger.path = Some(directory.path().join("command-receipts.json"));
        std::fs::create_dir(directory.path().join("command-receipts.tmp")).unwrap();
        assert_eq!(
            ledger
                .begin("new".into(), "new".into(), &Value::Null)
                .unwrap_err(),
            "intent_not_durable"
        );
        assert_eq!(ledger.get("0").unwrap().state, "completed");
        assert!(ledger.retired.is_empty());
        assert!(ledger.get("new").is_none());
        assert_eq!(ledger.next_sequence, MAX_RECEIPTS as u64);
    }
}
