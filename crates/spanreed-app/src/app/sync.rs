//! Private usage history synchronization with Fabrials.
use crate::context::AppContext;

pub use crate::sync::{
    SyncSettings, SyncStatus, link_codex, recent, save_settings, settings, status,
};

pub fn run(ctx: &AppContext) -> Result<String, String> {
    crate::sync::run(&ctx.pricing().table())
}
