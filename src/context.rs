//! Composition root: each process entry (CLI, tray, desktop host) builds one
//! `AppContext` and passes it down instead of reaching for globals.

use std::sync::Arc;
use std::time::Duration;

use crate::model::ProviderOutput;
use crate::ports::{AfterProbe, Notifier, ProbePorts, SnapshotProvider};

/// Probe cache lifetime for `snapshot(false)`.
const SNAPSHOT_TTL: Duration = Duration::from_secs(120);

/// Process-level services. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct AppContext {
    inner: Arc<Inner>,
}

struct Inner {
    cost: crate::cost::LocalCost,
    history_sync: crate::sync::HistorySync,
    notifier: Arc<dyn Notifier>,
    snapshot: fabrials_runtime::Snapshot<Vec<ProviderOutput>>,
}

impl Default for AppContext {
    fn default() -> Self {
        Self::new()
    }
}

impl AppContext {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                cost: crate::cost::LocalCost,
                history_sync: crate::sync::HistorySync::default(),
                notifier: Arc::new(crate::notifications::ResetExpiryNotifier),
                snapshot: fabrials_runtime::Snapshot::new(SNAPSHOT_TTL),
            }),
        }
    }

    fn with_ports<T>(&self, run: impl FnOnce(ProbePorts<'_>) -> T) -> T {
        let after: [&dyn AfterProbe; 1] = [&self.inner.history_sync];
        run(ProbePorts {
            cost: &self.inner.cost,
            after: &after,
        })
    }

    pub fn probe_detected(&self) -> Vec<ProviderOutput> {
        self.with_ports(crate::probe::probe_detected)
    }

    pub fn probe_all(&self) -> Vec<ProviderOutput> {
        self.with_ports(crate::probe::probe_all)
    }

    pub fn probe_one(&self, id: &str) -> Option<ProviderOutput> {
        self.with_ports(|ports| crate::probe::probe_one(ports, id))
    }

    /// Daemon cache when a local API is up, else a fresh probe.
    pub fn cached_or_probe(&self) -> Vec<ProviderOutput> {
        crate::api::fetch_cached().unwrap_or_else(|| self.probe_detected())
    }

    /// Recent outputs shared by desktop, widget and notifications; recorded
    /// in history on every refresh.
    pub fn snapshot(&self, force: bool) -> Vec<ProviderOutput> {
        self.inner.snapshot.refresh(force, || {
            let outputs = if force {
                self.probe_detected()
            } else {
                self.cached_or_probe()
            };
            crate::history::record(&outputs);
            outputs
        })
    }

    pub fn notifier(&self) -> Arc<dyn Notifier> {
        Arc::clone(&self.inner.notifier)
    }

    pub fn api_services(&self) -> crate::api::Services {
        crate::api::Services {
            source: Arc::new(self.clone()),
            notifier: self.notifier(),
        }
    }
}

impl SnapshotProvider for AppContext {
    fn probe_detected(&self) -> Vec<ProviderOutput> {
        AppContext::probe_detected(self)
    }
}
