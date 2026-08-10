//! Persistent setup state for uninstall / status.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::paths;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SetupState {
    pub version: u32,
    #[serde(default)]
    pub install_path: Option<String>,
    #[serde(default)]
    pub service: Option<ServiceState>,
    /// Daily anonymous share at 23:00 Europe/Madrid.
    #[serde(default)]
    pub share_schedule: Option<ServiceState>,
    #[serde(default)]
    pub wired: WiredState,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceState {
    pub enabled: bool,
    pub kind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WiredState {
    #[serde(default)]
    pub grok: Option<GrokWireState>,
    #[serde(default)]
    pub opencode_xai: Option<OpenCodeWireState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GrokWireState {
    pub previous: Option<String>,
    pub current: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenCodeWireState {
    pub path: String,
    pub previous_base_url: Option<String>,
}

pub fn state_path() -> PathBuf {
    paths::data_dir().join("setup-state.json")
}

pub fn load() -> Option<SetupState> {
    let path = state_path();
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim()).ok()
}

pub fn save(state: &SetupState) -> Result<(), String> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir state: {e}"))?;
    }
    let mut s = state.clone();
    if s.version == 0 {
        s.version = 1;
    }
    let text = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|e| format!("write state: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_state_json() {
        let mut s = SetupState {
            version: 1,
            ..Default::default()
        };
        s.install_path = Some("/tmp/spanreed".into());
        s.wired.grok = Some(GrokWireState {
            previous: None,
            current: "http://127.0.0.1:18736/v1".into(),
        });
        let text = serde_json::to_string(&s).unwrap();
        let back: SetupState = serde_json::from_str(&text).unwrap();
        assert_eq!(back.install_path.as_deref(), Some("/tmp/spanreed"));
        assert_eq!(
            back.wired.grok.as_ref().unwrap().current,
            "http://127.0.0.1:18736/v1"
        );
    }
}
