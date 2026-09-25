//! Opt-in reset-credit expiry notifications.
use crate::context::AppContext;

pub use crate::notifications::{Settings, deliver_user_visible, set_enabled, settings};

/// Alerts for the current snapshot; probes only when enabled.
pub fn check(
    ctx: &AppContext,
    send: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<u32, String> {
    if !settings()?.reset_expiry {
        return Ok(0);
    }
    crate::notifications::deliver_outputs(&ctx.snapshot(false), send)
}

pub fn test(send: impl FnOnce(&str, &str) -> Result<(), String>) -> Result<(), String> {
    if !settings()?.reset_expiry {
        return Err("Enable and save reset notifications first".into());
    }
    send(
        "Spanreed notification test",
        "System notifications are working. This is a test, not a reset expiry alert.",
    )
}
