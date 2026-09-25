//! Installation, the capture user service, the tray autostart, the share
//! schedule and client wiring (`spanreed setup`).

pub use crate::setup::{
    Line, Outcome, PromptHints, SetupOptions, SetupPlan, SetupStatus, UninstallOptions, apply,
    prompt_hints, status, tray_default, uninstall,
};
