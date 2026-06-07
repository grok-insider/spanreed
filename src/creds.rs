//! Linux-native credential and local-state discovery:
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

/// A running process discovered from `/proc`.
pub struct ProcInfo {
    pub pid: i32,
    /// Full command line (argv joined by spaces).
    pub cmdline: String,
}

/// Scan `/proc` for processes whose cmdline contains all of `needles`
/// (case-insensitive). Returns matches with their full command line so callers
/// can extract flags like `--csrf_token`.
pub fn find_processes(needles: &[&str]) -> Vec<ProcInfo> {
    let mut out = Vec::new();
    let proc = match std::fs::read_dir("/proc") {
        Ok(p) => p,
        Err(_) => return out,
    };
    for entry in proc.flatten() {
        let name = entry.file_name();
        let pid: i32 = match name.to_str().and_then(|s| s.parse().ok()) {
            Some(p) => p,
            None => continue,
        };
        let cmdline_path = entry.path().join("cmdline");
        let raw = match std::fs::read(&cmdline_path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        // /proc cmdline is NUL-separated.
        let cmdline: String = raw
            .split(|b| *b == 0)
            .map(|seg| String::from_utf8_lossy(seg).into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        let lower = cmdline.to_lowercase();
        if needles.iter().all(|n| lower.contains(&n.to_lowercase())) {
            out.push(ProcInfo { pid, cmdline });
        }
    }
    out
}

/// Extract a CLI flag value from a command line. Handles `--flag value` and
/// `--flag=value`.
pub fn extract_flag(cmdline: &str, flag: &str) -> Option<String> {
    let parts: Vec<&str> = cmdline.split_whitespace().collect();
    let flag_eq = format!("{flag}=");
    for (i, part) in parts.iter().enumerate() {
        if *part == flag {
            if let Some(next) = parts.get(i + 1) {
                return Some(next.to_string());
            }
        } else if let Some(rest) = part.strip_prefix(&flag_eq) {
            return Some(rest.to_string());
        }
    }
    None
}

/// Find TCP ports a pid is listening on by parsing `/proc/<pid>/net/tcp`
/// against the pid's socket inodes. Returns localhost listening ports.
pub fn listening_ports(pid: i32) -> Vec<u16> {
    use std::collections::HashSet;

    // Collect socket inodes owned by the pid.
    let fd_dir = format!("/proc/{pid}/fd");
    let mut inodes: HashSet<u64> = HashSet::new();
    if let Ok(fds) = std::fs::read_dir(&fd_dir) {
        for fd in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd.path()) {
                let t = target.to_string_lossy();
                if let Some(inode) = t.strip_prefix("socket:[").and_then(|s| s.strip_suffix(']')) {
                    if let Ok(n) = inode.parse::<u64>() {
                        inodes.insert(n);
                    }
                }
            }
        }
    }
    if inodes.is_empty() {
        return Vec::new();
    }

    let mut ports = HashSet::new();
    for tcp_file in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let content = match std::fs::read_to_string(tcp_file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in content.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            // local_address  rem_address  st ... inode is column 9.
            if cols.len() < 10 {
                continue;
            }
            // st == 0A means LISTEN.
            if cols[3] != "0A" {
                continue;
            }
            let inode: u64 = match cols[9].parse() {
                Ok(n) => n,
                Err(_) => continue,
            };
            if !inodes.contains(&inode) {
                continue;
            }
            // local_address is HEXIP:HEXPORT.
            if let Some(port_hex) = cols[1].split(':').nth(1) {
                if let Ok(port) = u16::from_str_radix(port_hex, 16) {
                    if port > 0 {
                        ports.insert(port);
                    }
                }
            }
        }
    }
    let mut v: Vec<u16> = ports.into_iter().collect();
    v.sort_unstable();
    v
}
