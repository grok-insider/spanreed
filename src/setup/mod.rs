//! Interactive install / wire / capture-service setup for spanreed.
//!
//! ```text
//! spanreed setup                 Interactive
//! spanreed setup --yes           Non-interactive defaults
//! spanreed setup --dry-run
//! spanreed setup status
//! spanreed setup uninstall
//! ```

mod apply;
mod detect;
mod grok_environment;
mod install;
pub(crate) mod jsonc;
mod paths;
pub mod review;
mod service;
pub mod share_schedule;
mod state;
mod status;
mod tray_autostart;
mod uninstall;
mod wire_grok;
mod wire_opencode;

pub use apply::{Line, Outcome, SetupOptions, SetupPlan, apply};
pub use paths::install_bin_path;
pub use status::{PromptHints, SetupStatus, prompt_hints, status, tray_default};
pub use uninstall::{UninstallOptions, uninstall};

/// Re-export for `main` capture ensure/status without exposing the whole module tree.
pub fn service_ensure(dry_run: bool) -> Result<String, String> {
    service::ensure(dry_run)
}

pub fn capture_ports_up() -> bool {
    service::ports_up()
}

/// Target base URL for Grok Build (`GROK_CLI_CHAT_PROXY_BASE_URL`).
pub const GROK_CAPTURE_BASE_URL: &str = "http://127.0.0.1:18736/v1";
/// Target base URL for OpenCode `provider.xai.options.baseURL`.
pub const OPENCODE_XAI_CAPTURE_BASE_URL: &str = "http://127.0.0.1:18736/xai/v1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_urls_include_v1() {
        assert!(GROK_CAPTURE_BASE_URL.ends_with("/v1"));
        assert!(OPENCODE_XAI_CAPTURE_BASE_URL.contains("18736/xai"));
    }
}
