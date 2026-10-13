use std::collections::{BTreeSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{journal::JournalError, protocol::Event};

const FILE: &str = "notification-activity.json";
const LIMIT: usize = 256 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    sequence: u64,
    created_at: u64,
    running: BTreeSet<String>,
    permissions: VecDeque<String>,
}

#[derive(Debug, Default)]
pub(crate) struct Activity {
    state: Stored,
    directory: Option<PathBuf>,
    unhealthy: bool,
}

impl Activity {
    pub(crate) fn open(directory: &Path) -> Result<Self, JournalError> {
        let owned = directory.join("notification-activity");
        let directory = owned.as_path();
        crate::instance::ensure_private_directory(directory).map_err(|_| JournalError::Storage)?;
        let path = directory.join(FILE);
        let state = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.len() > LIMIT as u64 {
                    return Err(JournalError::Storage);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o077 != 0 {
                        return Err(JournalError::Storage);
                    }
                }
                #[cfg(windows)]
                if !crate::win_acl::is_owner_only(&path).map_err(|_| JournalError::Storage)? {
                    return Err(JournalError::Storage);
                }
                let mut bytes = Vec::new();
                std::fs::File::open(&path)
                    .and_then(|file| file.take((LIMIT + 1) as u64).read_to_end(&mut bytes))
                    .map_err(|_| JournalError::Storage)?;
                if bytes.len() > LIMIT {
                    return Err(JournalError::Storage);
                }
                serde_json::from_slice(&bytes).map_err(|_| JournalError::Storage)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Stored::default(),
            Err(_) => return Err(JournalError::Storage),
        };
        let state: Stored = state;
        if state.running.len() > 1024
            || state.permissions.len() > 1024
            || (state.sequence == 0) != (state.created_at == 0)
            || state
                .running
                .iter()
                .chain(state.permissions.iter())
                .any(|key| key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(JournalError::Storage);
        }
        Ok(Self {
            state,
            directory: Some(directory.into()),
            unhealthy: false,
        })
    }

    pub(crate) fn observe(&mut self, event: &Event) {
        if matches!(
            event,
            Event::PermissionRequest { .. }
                | Event::InteractionRequest { .. }
                | Event::SessionStatus { .. }
        ) {
            self.observe_at(event, crate::now_ms());
        }
    }

    fn observe_at(&mut self, event: &Event, now: u64) {
        if self.unhealthy {
            return;
        }
        let mut next = self.state.clone();
        let hash = |value: &[u8]| format!("{:x}", Sha256::digest(value));
        let notify = match event {
            Event::PermissionRequest {
                session_id,
                request_id,
                ..
            }
            | Event::InteractionRequest {
                session_id,
                request_id,
                ..
            } => {
                let key = hash(
                    &serde_json::to_vec(&(session_id, request_id)).expect("string pair serializes"),
                );
                if next.permissions.contains(&key) {
                    return;
                }
                if next.permissions.len() == 1024 {
                    next.permissions.pop_front();
                }
                next.permissions.push_back(key);
                true
            }
            Event::SessionStatus { session_id, state } => {
                let key = hash(session_id.as_bytes());
                if state == "running" {
                    if next.running.len() == 1024 || !next.running.insert(key) {
                        return;
                    }
                    false
                } else {
                    let running = next.running.remove(&key);
                    if !running {
                        return;
                    }
                    state == "idle"
                }
            }
            _ => return,
        };
        if notify {
            let Some(sequence) = next.sequence.checked_add(1) else {
                self.unhealthy = true;
                return;
            };
            next.sequence = sequence;
            next.created_at = now;
        }
        if self.flush(&next).is_err() {
            self.unhealthy = true;
        } else {
            self.state = next;
        }
    }

    pub(crate) fn latest(&self) -> Result<Option<(u64, u64)>, JournalError> {
        if self.unhealthy {
            return Err(JournalError::Storage);
        }
        Ok((self.state.sequence > 0).then_some((self.state.sequence, self.state.created_at)))
    }

    fn flush(&self, state: &Stored) -> Result<(), JournalError> {
        let Some(directory) = &self.directory else {
            return Ok(());
        };
        let bytes = serde_json::to_vec(state).map_err(|_| JournalError::Storage)?;
        if bytes.len() > LIMIT {
            return Err(JournalError::Storage);
        }
        let mut entropy = [0u8; 16];
        getrandom::fill(&mut entropy).map_err(|_| JournalError::Storage)?;
        let temporary = directory.join(format!(
            ".notification-activity-{:x}.new",
            u128::from_le_bytes(entropy)
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| JournalError::Storage)?;
        let result = (|| {
            #[cfg(windows)]
            crate::win_acl::set_owner_only(&temporary).map_err(|_| JournalError::Storage)?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| JournalError::Storage)?;
            drop(file);
            std::fs::rename(&temporary, directory.join(FILE)).map_err(|_| JournalError::Storage)?;
            #[cfg(unix)]
            std::fs::File::open(directory)
                .and_then(|file| file.sync_all())
                .map_err(|_| JournalError::Storage)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn status(state: &str) -> Event {
        Event::SessionStatus {
            session_id: "synthetic-session".into(),
            state: state.into(),
        }
    }
    #[test]
    fn persisted_completion_sequence_survives_restart_and_contains_no_content() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("activity");
        let mut activity = Activity::open(&dir).unwrap();
        activity.observe_at(&status("running"), 1000);
        drop(activity);
        let mut activity = Activity::open(&dir).unwrap();
        assert_eq!(activity.latest().unwrap(), None);
        activity.observe_at(&status("idle"), 2000);
        activity.observe_at(&status("idle"), 3000);
        assert_eq!(activity.latest().unwrap(), Some((1, 2000)));
        drop(activity);
        let mut activity = Activity::open(&dir).unwrap();
        activity.observe_at(&status("running"), 4000);
        activity.observe_at(&status("idle"), 5000);
        assert_eq!(activity.latest().unwrap(), Some((2, 5000)));
        assert!(
            !std::fs::read_to_string(dir.join("notification-activity").join(FILE))
                .unwrap()
                .contains("synthetic-session")
        );
    }
    #[test]
    fn persistence_failure_is_visible_and_does_not_invent_delivery() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("activity");
        let mut activity = Activity::open(&dir).unwrap();
        activity.observe_at(&status("running"), 1000);
        std::fs::rename(&dir, temp.path().join("moved")).unwrap();
        activity.observe_at(&status("idle"), 2000);
        assert_eq!(activity.latest(), Err(JournalError::Storage));
        activity.observe_at(&status("idle"), 3000);
        assert_eq!(activity.latest(), Err(JournalError::Storage));
    }
    #[test]
    fn corrupt_or_oversized_state_fails_closed() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("activity");
        Activity::open(&dir).unwrap();
        std::fs::write(dir.join("notification-activity").join(FILE), b"{}").unwrap();
        assert!(Activity::open(&dir).is_err());
        std::fs::write(
            dir.join("notification-activity").join(FILE),
            vec![b'x'; LIMIT + 1],
        )
        .unwrap();
        assert!(Activity::open(&dir).is_err());
    }
}
