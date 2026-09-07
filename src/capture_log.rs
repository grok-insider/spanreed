//! Persistent logs for capture serve / watchdog.
//!
//! Default path (Windows): `%LOCALAPPDATA%\spanreed\logs\capture.log`
//! Elsewhere: `$XDG_DATA_HOME/spanreed/logs/capture.log` (or `~/.local/share/...`).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

#[cfg(windows)]
use crate::creds;
use crate::util;

/// Rotate when the active log exceeds this size.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

pub fn log_dir() -> PathBuf {
    #[cfg(windows)]
    {
        creds::data_local_home()
            .join(crate::app::APP_ID)
            .join("logs")
    }
    #[cfg(not(windows))]
    {
        crate::app::data_dir().join("logs")
    }
}

pub fn capture_log_path() -> PathBuf {
    log_dir().join("capture.log")
}

/// Append one line with a local timestamp. Best-effort; never panics.
pub fn append(msg: &str) {
    let path = capture_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    rotate_if_needed(&path);
    let ts = util::ms_to_iso(util::now_ms()).unwrap_or_else(|| "?".into());
    let line = format!("[{ts}] {msg}\n");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

pub fn rotate_if_needed(path: &std::path::Path) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() < MAX_LOG_BYTES {
        return;
    }
    let bak = path.with_extension("log.1");
    let _ = std::fs::remove_file(&bak);
    let _ = std::fs::rename(path, &bak);
}

/// Open the log for child stdout/stderr redirect (append).
pub fn open_append() -> Result<std::fs::File, String> {
    let path = capture_log_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir logs: {e}"))?;
    }
    rotate_if_needed(&path);
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open log {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_path_under_spanreed() {
        let p = capture_log_path();
        assert!(p.to_string_lossy().contains("spanreed"));
        assert!(p.ends_with("capture.log"));
    }
}
