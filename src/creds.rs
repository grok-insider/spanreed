//! Cross-platform credential and local-state discovery:
//! - XDG paths (`~/.config`, `~/.local/share`)
//! - plain credential files (JSON)
//! - app SQLite state DBs (via rusqlite, read-only)
//!
//! The OS keyring fallback lives in [`crate::secret`] (the cross-platform
//! SecretStore seam); process & listening-port discovery lives in
//! [`crate::proc`] (the cross-platform ProcessList seam).

pub mod opencode;

use std::path::{Path, PathBuf};

/// Explicit absolute overrides work on every host, including Windows known folders.
fn configured_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

pub fn home_dir() -> Option<PathBuf> {
    configured_path("HOME")
        .or_else(|| configured_path("USERPROFILE"))
        .or_else(dirs::home_dir)
}

/// Expand a leading `~` to the user's home directory.
pub fn expand(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

/// Absolute `$XDG_CONFIG_HOME`, then the OS-native configuration directory.
pub fn config_home() -> PathBuf {
    configured_path("XDG_CONFIG_HOME")
        .or_else(dirs::config_dir)
        .unwrap_or_else(|| expand("~/.config"))
}

/// Absolute `$XDG_DATA_HOME`, then the OS-native data directory.
pub fn data_home() -> PathBuf {
    configured_path("XDG_DATA_HOME")
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| expand("~/.local/share"))
}

/// Absolute `$XDG_CACHE_HOME`, then the OS-native cache directory.
pub fn cache_home() -> PathBuf {
    configured_path("XDG_CACHE_HOME")
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| expand("~/.cache"))
}

/// Machine-local data on Windows; an explicit XDG data root still takes precedence.
#[cfg(windows)]
pub fn data_local_home() -> PathBuf {
    configured_path("XDG_DATA_HOME")
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| expand("~/AppData/Local"))
}

/// Read a file to a string, returning None on any error.
pub fn read_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Read and parse a JSON credential file.
pub fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = read_file(path)?;
    serde_json::from_str(text.trim()).ok()
}

/// First existing path from a candidate list.
pub fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.exists()).cloned()
}

/// Read a single scalar value from a SQLite DB (read-only).
///
/// `sql` should select exactly one column. Returns the first row's value as a
/// string. Opens read-only with immutable fallback so WAL locks don't block us.
pub fn sqlite_query_one(
    db_path: &Path,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Option<String> {
    use rusqlite::OpenFlags;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    // Try a normal read-only open first (sees WAL), then immutable as a fallback.
    let conn = rusqlite::Connection::open_with_flags(db_path, flags)
        .or_else(|_| {
            let uri = format!("file:{}?immutable=1", db_path.to_string_lossy());
            rusqlite::Connection::open_with_flags(Path::new(&uri), flags)
        })
        .ok()?;
    conn.query_row(sql, params, |row| row.get::<_, String>(0))
        .ok()
}

/// Run an arbitrary read query returning all rows of two columns (i64, f64).
pub fn sqlite_query_rows_i64_f64(db_path: &Path, sql: &str) -> Option<Vec<(i64, f64)>> {
    use rusqlite::OpenFlags;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    let conn = rusqlite::Connection::open_with_flags(db_path, flags)
        .or_else(|_| {
            let uri = format!("file:{}?immutable=1", db_path.to_string_lossy());
            rusqlite::Connection::open_with_flags(Path::new(&uri), flags)
        })
        .ok()?;
    let mut stmt = conn.prepare(sql).ok()?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?)))
        .ok()?;
    Some(rows.filter_map(Result::ok).collect())
}

/// Read a whitelisted environment variable, trimmed.
pub fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp(label: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("spanreed-test-{label}-{n}"))
    }

    #[test]
    fn expand_tilde() {
        let home = home_dir().unwrap();
        assert_eq!(expand("~"), home);
        assert_eq!(expand("~/foo/bar"), home.join("foo/bar"));
        assert_eq!(expand("/abs/path"), PathBuf::from("/abs/path"));
    }

    #[test]
    fn read_json_parses_and_missing_is_none() {
        let dir = tmp("json");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("c.json");
        std::fs::write(&f, r#"{"a":{"key":"v"}}"#).unwrap();
        let v = read_json(&f).unwrap();
        assert_eq!(v["a"]["key"], "v");
        assert!(read_json(&dir.join("nope.json")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sqlite_query_one_and_rows() {
        let dir = tmp("sqlite");
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("state.vscdb");
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE ItemTable(key TEXT, value TEXT);
                 INSERT INTO ItemTable VALUES('cursorAuth/accessToken','tok123');
                 CREATE TABLE message(ts INTEGER, cost REAL);
                 INSERT INTO message VALUES(1000, 1.5);
                 INSERT INTO message VALUES(2000, 2.5);",
            )
            .unwrap();
        }
        let key = "cursorAuth/accessToken";
        let got = sqlite_query_one(&db, "SELECT value FROM ItemTable WHERE key = ?1", &[&key]);
        assert_eq!(got.as_deref(), Some("tok123"));

        let rows =
            sqlite_query_rows_i64_f64(&db, "SELECT ts, cost FROM message ORDER BY ts").unwrap();
        assert_eq!(rows, vec![(1000, 1.5), (2000, 2.5)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_existing_finds_present() {
        let dir = tmp("firstexist");
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::write(&b, "x").unwrap();
        assert_eq!(first_existing(&[a.clone(), b.clone()]), Some(b));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
