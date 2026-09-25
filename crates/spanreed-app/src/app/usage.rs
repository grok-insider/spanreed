//! Usage: probes, the shared snapshot, history and local consumption.
use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::context::AppContext;

pub use crate::model::{self, ProbeView, ProviderOutput};
pub use crate::output::{plain, plain_with_view};
pub use crate::util::now_ms;
pub use fabrials_fabric::history::HistorySample;
pub use fabrials_pricing::PricingMap;
pub use fabrials_types::HopRecord;
pub use fabrials_types::consumption::{ConsumptionReport as UsageReport, UsageFilter};

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub client: String,
    pub account: String,
    pub credential: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UsageSettings {
    pub additional_roots: BTreeMap<String, Vec<String>>,
    pub disabled_clients: Vec<String>,
}

pub struct CardInput<'a> {
    pub outputs: &'a [ProviderOutput],
    pub capture_up: bool,
    pub status: Option<&'a str>,
}

/// Port: usage ledgers and stores (quota history, capture hops, local
/// consumption) and the renderers that read them.
pub trait UsageStore: Send + Sync {
    fn record_history(&self, outputs: &[ProviderOutput]);
    /// `SPANREED_HISTORY` asks `probe` to record too.
    fn should_record_on_probe(&self) -> bool;
    fn history_samples(
        &self,
        provider: Option<&str>,
        limit: usize,
    ) -> Result<Vec<HistorySample>, String>;
    fn format_history(&self, samples: &[HistorySample]) -> String;
    fn recent_hops(&self) -> Result<Vec<HopRecord>, String>;
    fn report(
        &self,
        filter: UsageFilter,
        force: bool,
        pricing: &PricingMap,
    ) -> Result<UsageReport, String>;
    /// Client catalog, discovery settings and connections, as JSON.
    fn sources(&self) -> Result<serde_json::Value, String>;
    fn save_connection(&self, connection: Connection) -> Result<(), String>;
    fn disconnect(&self, client: &str) -> Result<(), String>;
    fn discovery_settings(&self) -> Result<UsageSettings, String>;
    fn save_discovery(&self, settings: &UsageSettings) -> Result<(), String>;
    /// The tray card document, with local-log cost priced by `pricing`.
    fn card_html(&self, card: CardInput<'_>, pricing: &PricingMap) -> String;
    fn can_use_reset(&self, output: &ProviderOutput) -> bool;
    /// Waybar JSON; the primary provider is the most recently used one.
    fn waybar(&self, outputs: &[ProviderOutput]) -> serde_json::Value;
}

/// Port: price and context-window tables.
pub trait PricingSource: Send + Sync {
    fn table(&self) -> Arc<PricingMap>;
    /// Re-read the cached and user tables.
    fn reload(&self);
    /// Refresh the remote cache when stale, then reload if it changed.
    fn refresh(&self);
    /// LiteLLM prices plus the models.dev OpenCode Go channel, as JSON.
    fn fetch_upstream(&self) -> Result<String, String>;
}

fn store(ctx: &AppContext) -> &dyn UsageStore {
    ctx.services().usage.as_ref()
}

pub fn format_history(ctx: &AppContext, samples: &[HistorySample]) -> String {
    store(ctx).format_history(samples)
}

pub fn fetch_price_table(ctx: &AppContext) -> Result<String, String> {
    ctx.pricing().fetch_upstream()
}

pub fn history(ctx: &AppContext) -> Result<Vec<HistorySample>, String> {
    store(ctx).history_samples(None, 500)
}

pub fn samples(
    ctx: &AppContext,
    provider: Option<&str>,
    limit: usize,
) -> Result<Vec<HistorySample>, String> {
    store(ctx).history_samples(provider, limit)
}

pub fn hops(ctx: &AppContext) -> Result<Vec<HopRecord>, String> {
    store(ctx).recent_hops()
}

pub fn snapshot(ctx: &AppContext, force: bool) -> Vec<ProviderOutput> {
    ctx.snapshot(force)
}

/// Probe the detected providers, all of them (`force`), or one id.
pub fn probe(ctx: &AppContext, id: Option<&str>, force: bool) -> Option<Vec<ProviderOutput>> {
    let outputs = match id {
        Some(id) => vec![ctx.probe_one(id)?],
        None if force => ctx.probe_all(),
        None => ctx.probe_detected(),
    };
    if store(ctx).should_record_on_probe() {
        store(ctx).record_history(&outputs);
    }
    Some(outputs)
}

/// The tray usage card for `outputs`, with local-log cost priced by the context.
pub fn card_html(ctx: &AppContext, card: CardInput<'_>) -> String {
    store(ctx).card_html(card, &ctx.pricing().table())
}

pub fn can_use_reset(ctx: &AppContext, output: &ProviderOutput) -> bool {
    store(ctx).can_use_reset(output)
}

pub fn waybar(ctx: &AppContext, outputs: &[ProviderOutput]) -> serde_json::Value {
    store(ctx).waybar(outputs)
}

/// Daemon cache when a local API is up, else a fresh probe.
pub fn cached_or_probe(ctx: &AppContext) -> Vec<ProviderOutput> {
    ctx.cached_or_probe()
}

pub fn detected(ctx: &AppContext) -> Vec<ProviderOutput> {
    ctx.probe_detected()
}

pub fn report(ctx: &AppContext, filter: UsageFilter, force: bool) -> Result<UsageReport, String> {
    store(ctx).report(filter, force, &ctx.pricing().table())
}

pub fn sources(ctx: &AppContext) -> Result<serde_json::Value, String> {
    store(ctx).sources()
}

pub fn save_connection(ctx: &AppContext, connection: Connection) -> Result<(), String> {
    store(ctx).save_connection(connection)
}

pub fn disconnect(ctx: &AppContext, client: &str) -> Result<(), String> {
    store(ctx).disconnect(client)
}

pub fn discovery_settings(ctx: &AppContext) -> Result<UsageSettings, String> {
    store(ctx).discovery_settings()
}

pub fn save_discovery(ctx: &AppContext, settings: &UsageSettings) -> Result<(), String> {
    store(ctx).save_discovery(settings)
}

/// A provider row of `spanreed list`.
pub struct Listed {
    pub id: String,
    pub name: String,
    pub state: &'static str,
}

/// Built-in providers, host accounts and addon providers.
pub fn list(ctx: &AppContext) -> Vec<Listed> {
    let catalog = ctx.services().providers.as_ref();
    let mut rows: Vec<Listed> = catalog
        .providers()
        .into_iter()
        .map(|p| Listed {
            id: p.id().into(),
            name: p.name().into(),
            state: if p.detect() { "detected" } else { "—" },
        })
        .collect();
    rows.extend(catalog.listed().into_iter().map(|row| Listed {
        id: row.id,
        name: row.name,
        state: row.state,
    }));
    rows
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct Detection {
    pub id: String,
    pub name: String,
    pub detected: bool,
}

pub fn detection(ctx: &AppContext) -> Vec<Detection> {
    ctx.services()
        .providers
        .providers()
        .into_iter()
        .map(|provider| Detection {
            id: provider.id().into(),
            name: provider.name().into(),
            detected: provider.detect(),
        })
        .collect()
}
