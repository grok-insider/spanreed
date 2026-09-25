//! Built-in status-bar fragments (Waybar, Eww, SketchyBar) and the panel.
use std::path::Path;

use crate::context::AppContext;

/// Port: status-bar profile files and the panel snapshot.
pub trait StatusBarProfiles: Send + Sync {
    fn files(&self, name: &str) -> Option<Vec<(&'static str, &'static str)>>;
    /// Write a profile's files into `directory` without replacing user files.
    fn install(&self, name: &str, directory: &Path) -> Result<(), String>;
    fn panel(&self) -> serde_json::Value;
}

pub fn files(ctx: &AppContext, name: &str) -> Option<Vec<(&'static str, &'static str)>> {
    ctx.services().profiles.files(name)
}

pub fn install(ctx: &AppContext, name: &str, directory: &Path) -> Result<(), String> {
    ctx.services().profiles.install(name, directory)
}

pub fn panel(ctx: &AppContext) -> serde_json::Value {
    ctx.services().profiles.panel()
}
