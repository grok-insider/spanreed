//! Local routing policies (autosteer, exhaustion thresholds).

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct RoutingSnapshot {
    pub policies: Vec<crate::local_control::RoutingPolicy>,
    pub limits: crate::local_control::RoutingLimits,
}
pub fn routing() -> Result<RoutingSnapshot, String> {
    let policies = crate::local_control::PROVIDERS
        .iter()
        .map(|provider| crate::local_control::policy_view(provider))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RoutingSnapshot {
        policies,
        limits: crate::local_control::limits_view()?,
    })
}

pub fn set_routing(provider: &str, on: bool, threshold: f64) -> Result<(), String> {
    crate::local_control::set_policy(provider, on, Some(threshold))
}
