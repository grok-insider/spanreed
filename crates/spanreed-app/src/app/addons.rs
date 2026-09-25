//! Addons: in-process, toml-manifest and PATH commands and providers.
use std::process::ExitCode;

use crate::context::AppContext;

#[derive(Debug, Clone)]
pub struct AddonListing {
    pub id: String,
    pub name: String,
    pub source: &'static str,
    pub enabled: bool,
    pub commands: Vec<String>,
}

/// Port: the addon host (discovery and command dispatch).
pub trait AddonHost: Send + Sync {
    fn list(&self) -> Vec<AddonListing>;
    /// Run the addon that owns the command `prefix`, if any.
    fn dispatch_prefix(&self, prefix: &str, rest: &[String]) -> Option<ExitCode>;
}

pub fn list(ctx: &AppContext) -> Vec<AddonListing> {
    ctx.services().addons.list()
}

pub fn dispatch_prefix(ctx: &AppContext, prefix: &str, rest: &[String]) -> Option<ExitCode> {
    ctx.services().addons.dispatch_prefix(prefix, rest)
}
