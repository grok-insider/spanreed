//! Product identity: Spanreed.

use std::path::PathBuf;

use crate::creds;

pub const APP_ID: &str = "spanreed";
pub const APP_NAME: &str = "Spanreed";
pub const GITHUB_REPO: &str = "grok-insider/spanreed";

pub fn bin_name() -> &'static str {
    if cfg!(windows) {
        "spanreed.exe"
    } else {
        "spanreed"
    }
}

pub fn config_dir() -> PathBuf {
    creds::config_home().join(APP_ID)
}

pub fn data_dir() -> PathBuf {
    creds::data_home().join(APP_ID)
}

pub fn cache_dir() -> PathBuf {
    creds::cache_home().join(APP_ID)
}

pub fn user_agent() -> String {
    format!(
        "{APP_ID}/{} (+{})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS
    )
}

pub fn env_offline() -> bool {
    creds::env("SPANREED_OFFLINE").is_some()
}
