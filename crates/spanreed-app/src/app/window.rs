//! Hand-off between front ends and the desktop window (routes and alerts).
use crate::context::AppContext;

pub struct PendingAlert {
    pub id: String,
    pub title: String,
    pub body: String,
}

/// Port: the desktop window's route and alert inbox (files shared by the
/// CLI, the tray and the running desktop app).
pub trait DesktopBridge: Send + Sync {
    /// Ask the desktop app (starting it when needed) to show `page`.
    fn open(&self, page: &str) -> Result<(), String>;
    fn peek(&self) -> Option<String>;
    fn take(&self) -> Option<String>;
    fn mark_running(&self);
    fn unmark_running(&self);
    /// Hand an alert to a running desktop app; `true` when one took it.
    fn hand_off_alert(&self, title: &str, body: &str) -> bool;
    fn take_alert(&self) -> Option<PendingAlert>;
    fn ack_alert(&self, id: &str);
    /// Script that moves the renderer to `href`, for allowed routes.
    fn route_location_script(&self, href: &str) -> Option<String>;
}

fn bridge(ctx: &AppContext) -> &dyn DesktopBridge {
    ctx.services().window.as_ref()
}

pub fn open(ctx: &AppContext, page: &str) -> Result<(), String> {
    bridge(ctx).open(page)
}

pub fn peek(ctx: &AppContext) -> Option<String> {
    bridge(ctx).peek()
}

pub fn take(ctx: &AppContext) -> Option<String> {
    bridge(ctx).take()
}

pub fn mark_running(ctx: &AppContext) {
    bridge(ctx).mark_running()
}

pub fn unmark_running(ctx: &AppContext) {
    bridge(ctx).unmark_running()
}

pub fn hand_off_alert(ctx: &AppContext, title: &str, body: &str) -> bool {
    bridge(ctx).hand_off_alert(title, body)
}

pub fn take_alert(ctx: &AppContext) -> Option<PendingAlert> {
    bridge(ctx).take_alert()
}

pub fn ack_alert(ctx: &AppContext, id: &str) {
    bridge(ctx).ack_alert(id)
}

pub fn route_location_script(ctx: &AppContext, href: &str) -> Option<String> {
    bridge(ctx).route_location_script(href)
}
