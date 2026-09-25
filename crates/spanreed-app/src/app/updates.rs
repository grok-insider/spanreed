//! Self-update from GitHub Releases.
use crate::context::AppContext;

pub const DEFAULT_REPO: &str = "grok-insider/spanreed";

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub current: String,
    pub latest: String,
    pub newer: bool,
    pub tag: String,
    pub asset_name: String,
    pub asset_url: String,
    pub sha_url: String,
}

/// Port: the GitHub Releases updater (HTTP download, binary replacement).
pub trait SelfUpdater: Send + Sync {
    fn check_for_update(&self) -> Result<CheckResult, String>;
    fn apply_update(&self, result: &CheckResult, dry_run: bool) -> Result<String, String>;
    fn can_apply_self_update(&self) -> bool;
    fn apply_blocked_reason(&self) -> Option<&'static str>;
    /// `SPANREED_OFFLINE` is set.
    fn offline(&self) -> bool;
}

fn updater(ctx: &AppContext) -> &dyn SelfUpdater {
    ctx.services().updates.as_ref()
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn check_for_update(ctx: &AppContext) -> Result<CheckResult, String> {
    updater(ctx).check_for_update()
}

pub fn apply_update(
    ctx: &AppContext,
    result: &CheckResult,
    dry_run: bool,
) -> Result<String, String> {
    updater(ctx).apply_update(result, dry_run)
}

pub fn can_apply_self_update(ctx: &AppContext) -> bool {
    updater(ctx).can_apply_self_update()
}

pub fn apply_blocked_reason(ctx: &AppContext) -> Option<&'static str> {
    updater(ctx).apply_blocked_reason()
}

pub fn offline(ctx: &AppContext) -> bool {
    updater(ctx).offline()
}
