//! Cheap last-used activity signals for Waybar provider selection.
//!
//! O(1) `stat` of a few known files per provider — no recursive session walks.

use crate::creds;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Latest local activity for a bar provider, as unix milliseconds.
/// Returns `None` when no signal file exists (or cannot be statted).
pub fn last_activity_ms(provider_id: &str) -> Option<i64> {
    max_mtime_ms(&signal_paths(provider_id))
}

fn signal_paths(provider_id: &str) -> Vec<PathBuf> {
    match provider_id {
        "claude" => claude_history_paths(),
        "codex" => codex_history_paths(),
        "grok" => vec![creds::expand("~/.grok/active_sessions.json")],
        _ => Vec::new(),
    }
}

fn claude_history_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dir) = creds::env("CLAUDE_CONFIG_DIR") {
        paths.push(creds::expand(&dir).join("history.jsonl"));
    }
    paths.push(creds::config_home().join("claude").join("history.jsonl"));
    paths.push(creds::expand("~/.claude/history.jsonl"));
    paths
}

fn codex_history_paths() -> Vec<PathBuf> {
    if let Some(home) = creds::env("CODEX_HOME") {
        return vec![creds::expand(&home).join("history.jsonl")];
    }
    vec![
        creds::config_home().join("codex").join("history.jsonl"),
        creds::expand("~/.codex/history.jsonl"),
    ]
}

/// Max mtime among existing paths, in unix ms.
pub fn max_mtime_ms(paths: &[PathBuf]) -> Option<i64> {
    let mut best: Option<i64> = None;
    for path in paths {
        if let Some(ms) = file_mtime_ms(path) {
            best = Some(match best {
                Some(b) if b >= ms => b,
                _ => ms,
            });
        }
    }
    best
}

fn file_mtime_ms(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn max_mtime_none_when_missing() {
        let p = PathBuf::from("/tmp/spanreed-activity-does-not-exist-xyz");
        assert!(max_mtime_ms(&[p]).is_none());
    }

    #[test]
    fn max_mtime_picks_newest() {
        let dir = std::env::temp_dir().join(format!(
            "spanreed-activity-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let older = dir.join("older.jsonl");
        let newer = dir.join("newer.jsonl");
        std::fs::write(&older, b"old").unwrap();
        std::thread::sleep(Duration::from_millis(30));
        std::fs::write(&newer, b"new").unwrap();

        let ms = max_mtime_ms(&[older.clone(), newer.clone()]).unwrap();
        let newer_ms = file_mtime_ms(&newer).unwrap();
        assert_eq!(ms, newer_ms);
        assert!(ms >= file_mtime_ms(&older).unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_provider_has_no_activity() {
        assert!(last_activity_ms("copilot").is_none());
        assert!(last_activity_ms("cursor").is_none());
    }
}
