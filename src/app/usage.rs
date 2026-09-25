//! Usage: probes, the shared snapshot, history and local consumption.
use crate::context::AppContext;

pub use crate::model::{self, ProbeView, ProviderOutput};
pub use crate::output::{plain, plain_with_view, waybar};
pub use crate::usage::{UsageReport, catalog_view, connections, discovery};
pub use crate::util::now_ms;

pub fn format_history(samples: &[crate::history::HistorySample]) -> String {
    crate::history::format_table(samples)
}

/// LiteLLM prices plus the models.dev OpenCode Go channel, as JSON.
pub fn fetch_price_table() -> Result<String, String> {
    crate::pricing::fetch_filtered()
}

pub fn history() -> Result<Vec<crate::history::HistorySample>, String> {
    crate::history::local_samples(None, 500)
}

pub fn hops() -> Result<Vec<fabrials_types::HopRecord>, String> {
    crate::grok_ledger::recent_hops()
}

pub fn snapshot(ctx: &AppContext, force: bool) -> Vec<ProviderOutput> {
    ctx.snapshot(force)
}

/// Reset-expiry alerts for the current snapshot; probes only when enabled.
pub fn deliver_notifications(
    ctx: &AppContext,
    send: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<u32, String> {
    if !crate::notifications::settings()?.reset_expiry {
        return Ok(0);
    }
    crate::notifications::deliver_outputs(&ctx.snapshot(false), send)
}

/// Probe the detected providers, all of them (`force`), or one id.
pub fn probe(ctx: &AppContext, id: Option<&str>, force: bool) -> Option<Vec<ProviderOutput>> {
    let outputs = match id {
        Some(id) => vec![ctx.probe_one(id)?],
        None if force => ctx.probe_all(),
        None => ctx.probe_detected(),
    };
    if crate::history::should_record_on_probe() {
        crate::history::record(&outputs);
    }
    Some(outputs)
}

/// The tray usage card for `outputs`, with local-log cost priced by the context.
pub fn card_html(ctx: &AppContext, card: CardInput<'_>) -> String {
    crate::tray_card::present(
        card.outputs,
        card.capture_up,
        card.status,
        crate::util::now_ms(),
        &ctx.pricing().table(),
    )
}

pub struct CardInput<'a> {
    pub outputs: &'a [ProviderOutput],
    pub capture_up: bool,
    pub status: Option<&'a str>,
}

pub use crate::tray_card::can_use_reset;

/// Daemon cache when a local API is up, else a fresh probe.
pub fn cached_or_probe(ctx: &AppContext) -> Vec<ProviderOutput> {
    ctx.cached_or_probe()
}

pub fn detected(ctx: &AppContext) -> Vec<ProviderOutput> {
    ctx.probe_detected()
}

pub fn report(
    ctx: &AppContext,
    filter: fabrials_types::consumption::UsageFilter,
    force: bool,
) -> Result<UsageReport, String> {
    crate::usage::report(filter, force, &ctx.pricing().table())
}

pub fn sources() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "clients": catalog_view(),
        "settings": discovery::settings()?,
        "connections": connections::status()?,
    }))
}

pub fn samples(
    provider: Option<&str>,
    limit: usize,
) -> Result<Vec<crate::history::HistorySample>, String> {
    crate::history::local_samples(provider, limit)
}

/// A provider row of `spanreed list`.
pub struct Listed {
    pub id: String,
    pub name: String,
    pub state: &'static str,
}

/// Built-in providers, host accounts and addon providers.
pub fn list() -> Vec<Listed> {
    let mut rows: Vec<Listed> = crate::providers::all()
        .into_iter()
        .map(|p| Listed {
            id: p.id().into(),
            name: p.name().into(),
            state: if p.detect() { "detected" } else { "—" },
        })
        .collect();
    rows.extend(
        crate::accounts::list_provider("grok")
            .into_iter()
            .map(|acc| Listed {
                name: format!("Grok ({})", acc.alias),
                state: if acc.active { "active" } else { "account" },
                id: acc.id,
            }),
    );
    rows.extend(
        crate::addons::extra_provider_ids()
            .into_iter()
            .map(|(id, name, detected)| Listed {
                id,
                name: format!("{name} (addon)"),
                state: if detected { "detected" } else { "—" },
            }),
    );
    rows
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
