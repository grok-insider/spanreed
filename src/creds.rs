//! Linux-native credential and local-state discovery.
//!
//! Replaces the macOS Keychain / `~/Library` machinery from upstream with:
//! - XDG paths (`~/.config`, `~/.local/share`)
//! - plain credential files (JSON)
//! - app SQLite state DBs (via rusqlite, read-only)
//! - the Secret Service via `secret-tool` (libsecret CLI) as a keyring fallback

use std::path::{Path, PathBuf};
use std::process::Command;

/// Expand a leading `~` to the user's home directory.
pub fn expand(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

/// `$XDG_CONFIG_HOME` or `~/.config`.
pub fn config_home() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| expand("~/.config"))
}

/// `$XDG_DATA_HOME` or `~/.local/share`.
pub fn data_home() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| expand("~/.local/share"))
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
pub fn sqlite_query_one(db_path: &Path, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Option<String> {
    use rusqlite::OpenFlags;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    // Try a normal read-only open first (sees WAL), then immutable as a fallback.
    let conn = rusqlite::Connection::open_with_flags(db_path, flags)
        .or_else(|_| {
            let uri = format!("file:{}?immutable=1", db_path.to_string_lossy());
            rusqlite::Connection::open_with_flags(Path::new(&uri), flags)
        })
        .ok()?;
    conn.query_row(sql, params, |row| row.get::<_, String>(0)).ok()
}

/// Run an arbitrary read query returning all rows of two columns (i64, f64).
pub fn sqlite_query_rows_i64_f64(
    db_path: &Path,
    sql: &str,
) -> Option<Vec<(i64, f64)>> {
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
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
        })
        .ok()?;
    Some(rows.filter_map(Result::ok).collect())
}

/// Read a secret from the Secret Service via `secret-tool` (libsecret).
///
/// `attributes` are key/value pairs, e.g. `[("service", "Codex Auth")]`.
/// Returns None if `secret-tool` is missing or no matching item exists.
pub fn secret_tool_lookup(attributes: &[(&str, &str)]) -> Option<String> {
    let mut cmd = Command::new("secret-tool");
    cmd.arg("lookup");
    for (k, v) in attributes {
        cmd.arg(k).arg(v);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Read a whitelisted environment variable, trimmed.
pub fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
