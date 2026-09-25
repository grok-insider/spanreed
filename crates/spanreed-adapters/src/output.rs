//! Renderers from the domain, with Waybar's primary provider chosen by local
//! activity.

pub use spanreed_domain::output::*;

use crate::model::ProviderOutput;

/// Waybar custom-module JSON; see [`waybar_with_activity`].
pub fn waybar(outputs: &[ProviderOutput]) -> serde_json::Value {
    waybar_with_activity(outputs, crate::activity::last_activity_ms)
}
