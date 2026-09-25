//! Opt-in reset-credit expiry notifications and user-visible alerts.
use serde::{Deserialize, Serialize};

use crate::context::AppContext;
use crate::model::ProviderOutput;

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[cfg_attr(feature = "contracts", ts(rename = "ResetNotificationSettings"))]
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub reset_expiry: bool,
}

/// Port: notification settings, delivery bookkeeping and the OS notifier.
pub trait Notifications: Send + Sync {
    fn settings(&self) -> Result<Settings, String>;
    fn set_enabled(&self, enabled: bool) -> Result<(), String>;
    /// Alert once per expiring reset credit in `outputs` through `send`.
    fn deliver(
        &self,
        outputs: &[ProviderOutput],
        send: &mut dyn FnMut(&str, &str) -> Result<(), String>,
    ) -> Result<u32, String>;
    /// An OS notification, with a dialog where the banner is not proof.
    fn deliver_user_visible(&self, title: &str, body: &str) -> Result<(), String>;
}

fn port(ctx: &AppContext) -> &dyn Notifications {
    ctx.services().notifications.as_ref()
}

pub fn settings(ctx: &AppContext) -> Result<Settings, String> {
    port(ctx).settings()
}

pub fn set_enabled(ctx: &AppContext, enabled: bool) -> Result<(), String> {
    port(ctx).set_enabled(enabled)
}

pub fn deliver_user_visible(ctx: &AppContext, title: &str, body: &str) -> Result<(), String> {
    port(ctx).deliver_user_visible(title, body)
}

/// Alerts for the current snapshot through `send`; probes only when enabled.
pub fn check(
    ctx: &AppContext,
    mut send: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<u32, String> {
    if !settings(ctx)?.reset_expiry {
        return Ok(0);
    }
    port(ctx).deliver(&ctx.snapshot(false), &mut send)
}

/// Alerts for the current snapshot through the OS notifier.
pub fn check_user_visible(ctx: &AppContext) -> Result<u32, String> {
    check(ctx, |title, body| deliver_user_visible(ctx, title, body))
}

pub fn test(ctx: &AppContext) -> Result<(), String> {
    if !settings(ctx)?.reset_expiry {
        return Err("Enable and save reset notifications first".into());
    }
    deliver_user_visible(
        ctx,
        "Spanreed notification test",
        "System notifications are working. This is a test, not a reset expiry alert.",
    )
}
