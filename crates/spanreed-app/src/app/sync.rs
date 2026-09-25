//! Private usage history synchronization with Fabrials.
use serde::{Deserialize, Serialize};

use crate::context::AppContext;

pub use fabrials_types::private_sync::PrivateRecentPage;

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSettings {
    pub sources: Vec<String>,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub last_success_ms: Option<i64>,
    pub uploaded: usize,
    pub downloaded: usize,
    pub error: Option<String>,
}

/// Port: private history synchronization (selection, status, upload/download).
pub trait PrivateSync: Send + Sync {
    fn settings(&self) -> Result<SyncSettings, String>;
    fn save_settings(&self, settings: SyncSettings) -> Result<(), String>;
    fn status(&self) -> SyncStatus;
    fn recent(&self, before: Option<i64>) -> Result<PrivateRecentPage, String>;
    fn run(&self, pricing: &fabrials_pricing::PricingMap) -> Result<String, String>;
    fn link_codex(&self) -> Result<String, String>;
}

fn sync(ctx: &AppContext) -> &dyn PrivateSync {
    ctx.services().sync.as_ref()
}

pub fn settings(ctx: &AppContext) -> Result<SyncSettings, String> {
    sync(ctx).settings()
}

pub fn save_settings(ctx: &AppContext, settings: SyncSettings) -> Result<(), String> {
    sync(ctx).save_settings(settings)
}

pub fn status(ctx: &AppContext) -> SyncStatus {
    sync(ctx).status()
}

pub fn recent(ctx: &AppContext, before: Option<i64>) -> Result<PrivateRecentPage, String> {
    sync(ctx).recent(before)
}

pub fn link_codex(ctx: &AppContext) -> Result<String, String> {
    sync(ctx).link_codex()
}

pub fn run(ctx: &AppContext) -> Result<String, String> {
    sync(ctx).run(&ctx.pricing().table())
}
