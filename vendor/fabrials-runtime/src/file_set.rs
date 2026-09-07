//! Recoverable changes to a small set of private text files under one advisory lock.
//! Every cooperating reader acquires the lock and replays a committed journal first.
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    pub contents: Option<String>,
}

pub struct FileSet {
    root: PathBuf,
    _lock: std::fs::File,
}
impl FileSet {
    pub fn acquire(root: &Path) -> Result<Self, String> {
        Self::open(root, false)
    }
    /// Use from blocking host workers when concurrent account operations should queue.
    pub fn acquire_wait(root: &Path) -> Result<Self, String> {
        Self::open(root, true)
    }
    fn open(root: &Path, wait: bool) -> Result<Self, String> {
        std::fs::create_dir_all(root).map_err(|_| "Account storage unavailable")?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options
            .open(root.join(".write.lock"))
            .map_err(|_| "Account lock unavailable")?;
        if wait {
            fs2::FileExt::lock_exclusive(&lock)
        } else {
            fs2::FileExt::try_lock_exclusive(&lock)
        }
        .map_err(|_| "Account storage is busy; retry shortly")?;
        let set = Self {
            root: root.into(),
            _lock: lock,
        };
        set.recover()?;
        Ok(set)
    }
    pub fn commit(&self, changes: Vec<Change>) -> Result<(), String> {
        validate(&changes)?;
        self.recover()?;
        let bytes = serde_json::to_vec(&changes).map_err(|_| "Invalid account transaction")?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("Account transaction too large".into());
        }
        crate::files::atomic_write_private(&self.root.join(".transaction.json"), &bytes)
            .map_err(|_| "Could not journal account transaction")?;
        self.apply(changes)
    }
    fn recover(&self) -> Result<(), String> {
        use std::io::Read;
        let file = match std::fs::File::open(self.root.join(".transaction.json")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("Account recovery journal unavailable".into()),
        };
        let mut bytes = Vec::new();
        file.take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Account recovery journal unavailable")?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("Account recovery journal too large".into());
        }
        let changes: Vec<Change> =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid account recovery journal")?;
        validate(&changes)?;
        self.apply(changes)
    }
    fn apply(&self, changes: Vec<Change>) -> Result<(), String> {
        for change in changes {
            let path = self.root.join(change.path);
            match change.contents {
                Some(text) => crate::files::atomic_write_private(&path, text.as_bytes())
                    .map_err(|_| "Account transaction pending; storage write failed")?,
                None => match std::fs::remove_file(&path) {
                    Ok(()) =>
                    {
                        #[cfg(unix)]
                        if let Some(parent) = path.parent() {
                            std::fs::File::open(parent)
                                .and_then(|file| file.sync_all())
                                .map_err(|_| "Account deletion durability failed")?;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err("Account transaction pending; deletion failed".into()),
                },
            }
        }
        std::fs::remove_file(self.root.join(".transaction.json"))
            .map_err(|_| "Account recovery cleanup failed")?;
        #[cfg(unix)]
        std::fs::File::open(&self.root)
            .and_then(|file| file.sync_all())
            .map_err(|_| "Account recovery cleanup durability failed")?;
        Ok(())
    }
}
fn validate(changes: &[Change]) -> Result<(), String> {
    if changes.is_empty() || changes.len() > 10_000 {
        return Err("Invalid account transaction size".into());
    }
    let mut paths = std::collections::HashSet::new();
    for change in changes {
        let path = Path::new(&change.path);
        if path.as_os_str().is_empty() || change.path.len()>512 || !paths.insert(&change.path)
            || path.components().any(|component| !matches!(component, Component::Normal(name) if !name.to_string_lossy().starts_with('.')))
            || change.contents.as_ref().is_some_and(|contents| contents.len()>8*1024*1024) {
            return Err("Invalid account transaction path or content".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_transaction_replays_before_next_reader() {
        let root = std::env::temp_dir().join(format!(
            "fabrials-files-{}",
            crate::accounting::new_request_id()
        ));
        let set = FileSet::acquire(&root).unwrap();
        assert!(FileSet::acquire(&root).is_err());
        std::fs::create_dir(root.join("index.json")).unwrap();
        assert!(set
            .commit(vec![
                Change {
                    path: "grok/work.json".into(),
                    contents: Some("fixture-grant".into())
                },
                Change {
                    path: "index.json".into(),
                    contents: Some("fixture-index".into())
                }
            ])
            .is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("grok/work.json")).unwrap(),
            "fixture-grant"
        );
        drop(set);
        std::fs::remove_dir(root.join("index.json")).unwrap();
        let set = FileSet::acquire(&root).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("index.json")).unwrap(),
            "fixture-index"
        );
        assert!(!root.join(".transaction.json").exists());
        assert!(set
            .commit(vec![Change {
                path: "../outside".into(),
                contents: None
            }])
            .is_err());
        assert!(set
            .commit(vec![Change {
                path: ".write.lock".into(),
                contents: None
            }])
            .is_err());
        drop(set);
        std::fs::remove_dir_all(root).unwrap();
    }
}
