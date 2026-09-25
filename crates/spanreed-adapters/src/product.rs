//! Product identity: Spanreed.

use std::path::PathBuf;

use crate::creds;

pub use spanreed_app::app::PRODUCT_ID as APP_ID;
pub use spanreed_app::app::updates::DEFAULT_REPO as GITHUB_REPO;

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
