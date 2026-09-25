//! Ports: interfaces that lower modules call without importing the modules
//! that implement them. `context::AppContext` wires the implementations.

use crate::model::{MetricLine, ProviderOutput};

/// Cost presentation for probes, priced with the context's pricing catalog.
pub trait CostSource: Send + Sync {
    /// Local-log cost lines (Claude/Codex): `Last 30 Days`, since the weekly
    /// reset when `weekly_start_ms` is known, models, cache and trend.
    fn local_cost_lines(&self, provider_id: &str, weekly_start_ms: Option<i64>) -> Vec<MetricLine>;
    /// Local-log `(tokens, usd)` since `cutoff_ms`.
    fn local_totals_since(&self, provider_id: &str, cutoff_ms: i64) -> Option<(u64, f64)>;
    /// Capture-ledger cost lines (Grok), optionally with pool-% forecasts.
    fn capture_cost_lines(&self, query: CaptureCostQuery<'_>) -> Vec<MetricLine>;
}

/// Which capture-ledger records to price and how to forecast them.
#[derive(Clone, Copy, Default)]
pub struct CaptureCostQuery<'a> {
    /// `None` = records without an account (the default Grok identity).
    pub account_id: Option<&'a str>,
    pub weekly_start_ms: Option<i64>,
    pub weekly_pct: Option<f64>,
    pub week_end_ms: Option<i64>,
}

/// Runs after every probe with its outputs (private history sync, …).
pub trait AfterProbe: Send + Sync {
    fn after_probe(&self, outputs: &[ProviderOutput]);
}

/// Tells the user about probe results (reset-credit expiry alerts, …).
pub trait Notifier: Send + Sync {
    fn notify(&self, outputs: &[ProviderOutput]) -> Result<u32, String>;
}

/// Produces fresh provider outputs for background refreshers.
pub trait SnapshotProvider: Send + Sync {
    fn probe_detected(&self) -> Vec<ProviderOutput>;
}

/// Probe-time collaborators passed to `probe`.
#[derive(Clone, Copy)]
pub struct ProbePorts<'a> {
    pub cost: &'a dyn CostSource,
    pub after: &'a [&'a dyn AfterProbe],
}

#[cfg(test)]
pub(crate) struct NoCost;

#[cfg(test)]
impl CostSource for NoCost {
    fn local_cost_lines(&self, _: &str, _: Option<i64>) -> Vec<MetricLine> {
        Vec::new()
    }
    fn local_totals_since(&self, _: &str, _: i64) -> Option<(u64, f64)> {
        None
    }
    fn capture_cost_lines(&self, _: CaptureCostQuery<'_>) -> Vec<MetricLine> {
        Vec::new()
    }
}

#[cfg(test)]
impl ProbePorts<'static> {
    /// No cost lines and no hooks.
    pub(crate) fn bare() -> Self {
        Self {
            cost: &NoCost,
            after: &[],
        }
    }
}

impl ProbePorts<'_> {
    pub fn after_probe(&self, outputs: &[ProviderOutput]) {
        for hook in self.after {
            hook.after_probe(outputs);
        }
    }
}
