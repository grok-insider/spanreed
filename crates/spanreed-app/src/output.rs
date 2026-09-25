//! Renderers from the domain, with Waybar's primary provider chosen by local
//! activity.

pub use spanreed_domain::output::*;

use crate::model::ProviderOutput;

/// Waybar custom-module JSON; see [`waybar_with_activity`].
pub fn waybar(outputs: &[ProviderOutput]) -> serde_json::Value {
    waybar_with_activity(outputs, crate::activity::last_activity_ms)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_spanreed_provider_has_a_mark() {
        for provider in crate::providers::all() {
            let svg = crate::provider_icons::icon(provider.id());
            assert!(!svg.contains("data-icon=\"fallback\""), "{}", provider.id());
            assert!(svg.contains("<title>"), "{}", provider.id());
        }
    }
}
