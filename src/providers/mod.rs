//! Provider registry. Each provider is a native Rust module implementing
//! [`Provider`].

use crate::model::ProviderOutput;

pub mod claude;
pub mod codex;
pub mod cursor;
pub mod grok;
pub mod opencode_go;

/// A usage provider (Claude, Codex, ...).
pub trait Provider: Send + Sync {
    /// Stable id used on the CLI and local API (e.g. "claude").
    fn id(&self) -> &'static str;

    /// Human-friendly name (e.g. "Claude").
    fn name(&self) -> &'static str;

    /// Whether this provider has any local signal (creds/state) on this machine.
    /// Used to hide providers the user doesn't use. Probing a non-detected
    /// provider is allowed but typically yields an error line.
    fn detect(&self) -> bool;

    /// Fetch current usage. Implementations should return an error *line*
    /// (via `ProviderOutput::error`) rather than panicking.
    fn probe(&self) -> ProviderOutput;
}

/// All known providers, in display order.
pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(claude::Claude),
        Box::new(codex::Codex),
        Box::new(cursor::Cursor),
        Box::new(grok::Grok),
        Box::new(opencode_go::OpenCodeGo),
    ]
}

/// Look up a single provider by id.
pub fn by_id(id: &str) -> Option<Box<dyn Provider>> {
    all().into_iter().find(|p| p.id() == id)
}
