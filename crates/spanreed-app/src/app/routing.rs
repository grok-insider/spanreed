//! Local routing policies (autosteer, exhaustion thresholds).
use crate::context::AppContext;

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct RoutingPolicy {
    pub provider: String,
    pub autosteer: bool,
    pub exhausted_pct: f64,
    pub mode: String,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct PoolAccount {
    pub alias: String,
    pub plan: Option<String>,
    pub used_pct: Option<f64>,
    pub resets_at: Option<String>,
    pub exhausted: bool,
    pub role: String,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct ProviderPool {
    pub provider: String,
    pub route: String,
    pub autosteer: bool,
    pub accounts: Vec<PoolAccount>,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct RoutingLimits {
    pub mode: String,
    pub exhausted_pct: f64,
    pub providers: Vec<ProviderPool>,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct RoutingSnapshot {
    pub policies: Vec<RoutingPolicy>,
    pub limits: RoutingLimits,
}

/// Port: routing configuration (config.json) and account pool state.
pub trait RoutingStore: Send + Sync {
    /// Providers with a routing policy.
    fn providers(&self) -> &'static [&'static str];
    fn policy(&self, provider: &str) -> Result<RoutingPolicy, String>;
    fn limits(&self) -> Result<RoutingLimits, String>;
    fn set_policy(&self, provider: &str, on: bool, threshold: Option<f64>) -> Result<(), String>;
}

pub fn routing(ctx: &AppContext) -> Result<RoutingSnapshot, String> {
    let store = ctx.services().routing.as_ref();
    let policies = store
        .providers()
        .iter()
        .map(|provider| store.policy(provider))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RoutingSnapshot {
        policies,
        limits: store.limits()?,
    })
}

pub fn set_routing(
    ctx: &AppContext,
    provider: &str,
    on: bool,
    threshold: f64,
) -> Result<(), String> {
    ctx.services()
        .routing
        .set_policy(provider, on, Some(threshold))
}
