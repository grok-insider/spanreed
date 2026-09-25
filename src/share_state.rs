//! Share endpoint and due-day bookkeeping, shared by share, login and sync.

use std::path::PathBuf;

use crate::util;

pub(crate) const DEFAULT_API_BASE: &str = "https://fabrials.com/api/spanreed";
const ENV_API_BASE: &str = "SPANREED_API_BASE";
const ENV_OFFLINE: &str = "SPANREED_OFFLINE";
const LAST_SHARE_DAY_FILE: &str = "last_share_day";

pub fn api_base() -> String {
    std::env::var(ENV_API_BASE).unwrap_or_else(|_| DEFAULT_API_BASE.into())
}

pub fn is_offline() -> bool {
    matches!(
        std::env::var(ENV_OFFLINE).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

fn last_share_path() -> PathBuf {
    crate::app::config_dir().join(LAST_SHARE_DAY_FILE)
}

/// Last successfully recorded share day (`YYYY-MM-DD`), if any.
pub fn last_shared_day() -> Option<String> {
    let raw = crate::creds::read_file(&last_share_path())?;
    let day = raw.trim();
    if day.len() == 10 && day.as_bytes()[4] == b'-' && day.as_bytes()[7] == b'-' {
        Some(day.to_string())
    } else {
        None
    }
}

/// Whether a share is still due given last recorded day and today's key.
pub fn is_due_for_day(last: Option<&str>, today: &str) -> bool {
    match last {
        None => true,
        Some(d) => d != today,
    }
}

/// Whether a share is still due for the current product day.
pub fn is_due_today() -> bool {
    is_due_for_day(last_shared_day().as_deref(), &util::today_day_key_madrid())
}

/// Persist successful (or server-already-counted) share for the product day.
pub fn mark_shared_day(day: &str) -> Result<(), String> {
    let p = last_share_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir last_share_day: {e}"))?;
    }
    std::fs::write(&p, format!("{day}\n")).map_err(|e| format!("write last_share_day: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_gate_skips_same_day_only() {
        assert!(is_due_for_day(None, "2026-08-10"));
        assert!(is_due_for_day(Some("2026-08-09"), "2026-08-10"));
        assert!(!is_due_for_day(Some("2026-08-10"), "2026-08-10"));
    }
}
