//! Provider protocol adapters. Hosts own HTTP, credentials and persistence.
pub mod claude;
pub mod codex;
pub mod grok;
pub mod kimi;
pub mod nous;

pub mod device_flow;

pub mod catalog;

pub mod oauth;

pub mod usage;

/// An absolute http(s) origin without its trailing slash, or the pinned default.
pub(crate) fn origin_or(candidate: Option<&str>, pinned: &str) -> String {
    candidate
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
        .unwrap_or_else(|| pinned.trim_end_matches('/').to_string())
}
