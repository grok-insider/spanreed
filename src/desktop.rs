//! Typed desktop boundary. The renderer receives only presentation data.
use crate::model::ProviderOutput;
use std::sync::OnceLock;
use std::time::Duration;

pub fn history() -> Result<Vec<crate::history::HistorySample>, String> {
    crate::history::local_samples(None, 500)
}

pub fn hops() -> Result<Vec<fabrials_model::UsageRecord>, String> {
    crate::grok_ledger::recent_hops()
}

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

pub fn snapshot(force: bool) -> Vec<ProviderOutput> {
    static SNAPSHOT: OnceLock<fabrials_runtime::Snapshot<Vec<ProviderOutput>>> = OnceLock::new();
    SNAPSHOT
        .get_or_init(|| fabrials_runtime::Snapshot::new(Duration::from_secs(120)))
        .refresh(force, || {
            let outputs = if force {
                crate::probe::probe_detected()
            } else {
                crate::api::fetch_cached().unwrap_or_else(crate::probe::probe_detected)
            };
            crate::history::record(&outputs);
            outputs
        })
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct Detection {
    pub id: String,
    pub name: String,
    pub detected: bool,
}

pub fn detection() -> Vec<Detection> {
    crate::providers::all()
        .into_iter()
        .map(|provider| Detection {
            id: provider.id().into(),
            name: provider.name().into(),
            detected: provider.detect(),
        })
        .collect()
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub account_id: String,
    pub observed_at_ms: i64,
    pub models: Vec<String>,
}
pub fn models(account_id: &str) -> Result<ModelCatalog, String> {
    let models = crate::local_relay::models_for_account(account_id)?;
    Ok(ModelCatalog {
        account_id: account_id.into(),
        observed_at_ms: crate::util::now_ms(),
        models,
    })
}

#[cfg(all(test, feature = "contracts"))]
#[test]
fn frontend_contracts_match_rust() {
    assert_eq!(
        fabrials_model::contracts::typescript(),
        include_str!("../desktop/vendor/fabrials-ui/src/contracts.ts"),
        "Regenerate fabrials-model bindings and synchronize shared UI sources"
    );
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct AccountSummary {
    pub id: String,
    pub provider: String,
    pub alias: String,
    pub generation: Option<String>,
    pub active: bool,
    pub plan_label: Option<String>,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct AccountsView {
    pub accounts: Vec<AccountSummary>,
}
pub fn accounts() -> Result<AccountsView, String> {
    Ok(AccountsView {
        accounts: crate::accounts::routing_registry()?
            .accounts
            .into_iter()
            .map(|account| AccountSummary {
                id: account.id,
                provider: account.provider,
                alias: account.alias,
                generation: account.generation,
                active: account.active,
                plan_label: account.plan_label,
            })
            .collect(),
    })
}
