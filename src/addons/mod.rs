//! Addon host: in-process trait + JSON protocol + PATH/toml discovery.

pub mod grok_bridge;
pub mod host;
pub mod protocol;

use crate::model::ProviderOutput;

pub use host::{cmd as cmd_addon, dispatch_prefix, extra_detected_outputs, extra_provider_ids};
pub use protocol::AddonHello;

/// In-process addon (same ops as the JSON protocol).
pub trait Addon: Send + Sync {
    fn hello(&self) -> AddonHello;
    fn detect(&self, provider_id: &str) -> bool;
    fn probe(&self, provider_id: &str) -> ProviderOutput;
    fn command(&self, argv: &[String]) -> (String, String, i32);
}

pub fn compiled_in() -> Vec<Box<dyn Addon>> {
    vec![Box::new(grok_bridge::GrokBridge)]
}

pub fn command_owner(prefix: &str) -> Option<Box<dyn Addon>> {
    compiled_in()
        .into_iter()
        .find(|a| a.hello().caps.commands.iter().any(|c| c == prefix))
}
