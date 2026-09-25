//! Ports: interfaces that lower modules call without importing the modules
//! that implement them. `context::AppContext` wires the implementations.

use crate::model::{MetricLine, ProviderOutput};

/// Local consumption cost lines for a provider (Claude/Codex logs).
pub trait CostSource: Send + Sync {
    fn local_cost_lines(&self, provider_id: &str) -> Vec<MetricLine>;
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

impl ProbePorts<'_> {
    pub fn after_probe(&self, outputs: &[ProviderOutput]) {
        for hook in self.after {
            hook.after_probe(outputs);
        }
    }
}
