//! Durable rotation journal for cooperating local hosts. Separate from usage databases.
//! An interrupted redemption is never retried with the same credential generation.
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct Scope<'a> {
    pub environment: &'a str,
    pub owner: &'a str,
    pub provider: &'a str,
    pub alias: &'a str,
}

pub enum Recovery {
    Clean,
    Interrupted,
    Replacement(Value),
}

/// A host-owned source and durable journal held under one cooperating lease.
/// Implementations guard the account generation for the lifetime of this value.
pub trait RotationSource {
    fn load(&self) -> Result<Value, String>;
    fn recover(&self, current: &Value) -> Result<Recovery, String>;
    fn begin(&self, expected: &Value) -> Result<(), String>;
    fn rotated(&self, replacement: &Value) -> Result<(), String>;
    fn commit(&self, expected: &Value, replacement: &Value) -> Result<bool, String>;
    fn complete(&self) -> Result<(), String>;
}

pub struct Rotation {
    connection: rusqlite::Connection,
    key: String,
    _lock: std::fs::File,
}
impl Rotation {
    pub fn acquire(root: &Path, scope: Scope<'_>) -> Result<Self, String> {
        let fields = [scope.environment, scope.owner, scope.provider, scope.alias];
        if fields.iter().any(|field| {
            field.is_empty() || field.len() > 4096 || field.chars().any(char::is_control)
        }) {
            return Err("Invalid credential scope".into());
        }
        let key = hex::encode(Sha256::digest(
            serde_json::to_vec(&fields).map_err(|_| "Invalid credential scope")?,
        ));
        std::fs::create_dir_all(root).map_err(|_| "Credential journal unavailable")?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        // Lock files are stable: unlinking one could let a second process lock a
        // different inode for the same account while this process still holds it.
        let lock = options
            .open(root.join(format!("{key}.lock")))
            .map_err(|_| "Credential lock unavailable")?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .map_err(|_| "Account refresh already running; retry shortly")?;
        let connection = crate::database::open(&root.join("recovery.sqlite3"))
            .map_err(|_| "Credential journal unavailable")?;
        connection.execute_batch("PRAGMA secure_delete=ON; CREATE TABLE IF NOT EXISTS credential_rotations (scope TEXT PRIMARY KEY, expected TEXT NOT NULL, replacement TEXT);").map_err(|_| "Credential journal unavailable")?;
        Ok(Self {
            connection,
            key,
            _lock: lock,
        })
    }

    pub fn recover(&self, current: &Value) -> Result<Recovery, String> {
        let pending: Option<(String, Option<String>)> = self
            .connection
            .query_row(
                "SELECT expected,replacement FROM credential_rotations WHERE scope=?1",
                [&self.key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|_| "Credential journal read failed")?;
        let Some((expected, replacement)) = pending else {
            return Ok(Recovery::Clean);
        };
        let expected: Value =
            serde_json::from_str(&expected).map_err(|_| "Invalid credential journal")?;
        let replacement: Option<Value> = replacement
            .map(|value| serde_json::from_str(&value).map_err(|_| "Invalid credential journal"))
            .transpose()?;
        if replacement.as_ref() == Some(current) || &expected != current {
            self.complete()?;
            return Ok(Recovery::Clean);
        }
        Ok(match replacement {
            Some(value) => Recovery::Replacement(value),
            None => Recovery::Interrupted,
        })
    }

    /// Commit this marker before sending a request that may rotate a grant.
    pub fn begin(&self, expected: &Value) -> Result<(), String> {
        let expected = document(expected)?;
        let count = self.connection.execute("INSERT INTO credential_rotations(scope,expected) SELECT ?1,?2 WHERE (SELECT COUNT(*) FROM credential_rotations)<64 ON CONFLICT(scope) DO NOTHING", params![self.key,expected]).map_err(|_| "Could not reserve durable credential recovery")?;
        if count != 1 {
            return Err("Credential recovery is pending or full".into());
        }
        Ok(())
    }

    /// Persist the complete replacement before attempting to update its source.
    pub fn rotated(&self, replacement: &Value) -> Result<(), String> {
        let count = self
            .connection
            .execute(
                "UPDATE credential_rotations SET replacement=?2 WHERE scope=?1",
                params![self.key, document(replacement)?],
            )
            .map_err(|_| "Could not persist rotated credentials")?;
        if count != 1 {
            return Err("Credential rotation was not reserved".into());
        }
        Ok(())
    }

    /// Call only after the source is updated or its generation has changed.
    pub fn complete(&self) -> Result<(), String> {
        self.connection
            .execute(
                "DELETE FROM credential_rotations WHERE scope=?1",
                [&self.key],
            )
            .map_err(|_| "Could not finish credential recovery")?;
        Ok(())
    }
}
fn document(value: &Value) -> Result<String, String> {
    let text = serde_json::to_string(value).map_err(|_| "Invalid credential document")?;
    if text.len() > 1024 * 1024 {
        return Err("Credential document too large".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope(owner: &str) -> Scope<'_> {
        Scope {
            environment: "fixture-env",
            owner,
            provider: "grok",
            alias: "work",
        }
    }
    fn directory() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "fabrials-rotation-{}",
            crate::accounting::new_request_id()
        ))
    }
    #[test]
    fn scoped_locks_and_generations_prevent_duplicate_redemption() {
        let root = directory();
        let first = Rotation::acquire(&root, scope("owner-a")).unwrap();
        assert!(Rotation::acquire(&root, scope("owner-a")).is_err());
        let second = Rotation::acquire(&root, scope("owner-b")).unwrap();
        let old = serde_json::json!({"refresh":"fixture-old"});
        first.begin(&old).unwrap();
        assert!(first.begin(&old).is_err());
        assert!(matches!(second.recover(&old).unwrap(), Recovery::Clean));
        assert!(matches!(
            first.recover(&old).unwrap(),
            Recovery::Interrupted
        ));
        let new = serde_json::json!({"refresh":"fixture-new", "key":"fixture-access"});
        first.rotated(&new).unwrap();
        assert!(
            matches!(first.recover(&old).unwrap(), Recovery::Replacement(value) if value == new)
        );
        assert!(matches!(first.recover(&new).unwrap(), Recovery::Clean));
        first.begin(&new).unwrap();
        assert!(matches!(
            first
                .recover(&serde_json::json!({"refresh":"another-login"}))
                .unwrap(),
            Recovery::Clean
        ));
        drop(first);
        drop(second);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn recovery_survives_process_exit_without_running_destructors() {
        for rotated in [false, true] {
            let root = directory();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "credential_journal::tests::crash_fixture",
                ])
                .env("FABRIALS_TEST_JOURNAL", &root)
                .env("FABRIALS_TEST_ROTATED", if rotated { "1" } else { "0" })
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success());
            let recovery = Rotation::acquire(&root, scope("local")).unwrap();
            let state = recovery
                .recover(&serde_json::json!({"refresh":"old"}))
                .unwrap();
            if rotated {
                assert!(matches!(state, Recovery::Replacement(value) if value["refresh"] == "new"));
            } else {
                assert!(matches!(state, Recovery::Interrupted));
            }
            drop(recovery);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    #[ignore = "subprocess fixture for recovery_survives_process_exit_without_running_destructors"]
    fn crash_fixture() {
        let Some(root) = std::env::var_os("FABRIALS_TEST_JOURNAL") else {
            return;
        };
        let journal = Rotation::acquire(Path::new(&root), scope("local")).unwrap();
        journal
            .begin(&serde_json::json!({"refresh":"old"}))
            .unwrap();
        if std::env::var("FABRIALS_TEST_ROTATED").unwrap() == "1" {
            journal
                .rotated(&serde_json::json!({"refresh":"new"}))
                .unwrap();
        }
        std::process::exit(0);
    }
}
