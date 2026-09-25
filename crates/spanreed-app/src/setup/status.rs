//! Installation, service and client-wiring state for `spanreed setup status`.

use super::{
    detect, install_bin_path, service, share_schedule, state, tray_autostart, wire_grok,
    wire_opencode,
};

pub struct SetupStatus {
    pub install_path: String,
    pub binary_present: bool,
    pub current_exe: Option<std::path::PathBuf>,
    pub ledger: std::path::PathBuf,
    pub ledger_exists: bool,
    pub capture_service: String,
    pub tray_autostart: String,
    pub share_schedule: String,
    pub grok_detected: bool,
    pub grok_wiring: String,
    pub opencode_detected: bool,
    pub opencode_wiring: String,
    pub capture_up: bool,
    /// Some client is wired to the local capture proxy.
    pub clients_wired: bool,
}

pub fn status() -> SetupStatus {
    let detection = detect::scan();
    let state = state::load().unwrap_or_default();
    let bin = install_bin_path();
    let ledger = crate::grok_ledger::ledger_path();
    SetupStatus {
        install_path: state
            .install_path
            .clone()
            .unwrap_or_else(|| bin.display().to_string()),
        binary_present: bin.exists(),
        current_exe: std::env::current_exe().ok(),
        ledger_exists: ledger.exists(),
        ledger,
        capture_service: service::status(),
        tray_autostart: tray_autostart::status(),
        share_schedule: share_schedule::status(),
        grok_detected: detection.grok.detected,
        grok_wiring: wire_grok::status_line(&state),
        opencode_detected: detection.opencode.detected,
        opencode_wiring: wire_opencode::status_line(&detection, &state),
        capture_up: service::ports_up(),
        clients_wired: wire_grok::is_wired_to_capture(&state)
            || wire_opencode::is_wired_to_capture(&detection, &state),
    }
}

/// Hints shown next to the interactive setup questions.
pub struct PromptHints {
    pub install_path: String,
    pub grok: String,
    pub opencode: String,
}

pub fn prompt_hints() -> PromptHints {
    let detection = detect::scan();
    PromptHints {
        install_path: install_bin_path().display().to_string(),
        grok: detection.grok.hint(),
        opencode: detection.opencode.hint(),
    }
}

pub fn tray_default(capture_service_on: bool) -> bool {
    tray_autostart::default_enabled(capture_service_on)
}
