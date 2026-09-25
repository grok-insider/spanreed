//! `AppContext`: built once per process by the composition root (the
//! `spanreed` binary, the desktop host) from a [`Services`] bundle of
//! adapters, and passed down to every use case.

use std::sync::Arc;
use std::time::Duration;

use crate::model::ProviderOutput;
use crate::ports::{AfterProbe, AppPaths, Notifier, ProbePorts, Services, SnapshotProvider};

/// Probe cache lifetime for `snapshot(false)`.
const SNAPSHOT_TTL: Duration = Duration::from_secs(120);

/// Process-level services. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct AppContext {
    inner: Arc<Inner>,
}

struct Inner {
    services: Services,
    snapshot: fabrials_fabric::Snapshot<Vec<ProviderOutput>>,
}

impl AppContext {
    pub fn new(services: Services) -> Self {
        Self {
            inner: Arc::new(Inner {
                services,
                snapshot: fabrials_fabric::Snapshot::new(SNAPSHOT_TTL),
            }),
        }
    }

    /// The adapters this context was built with.
    pub fn services(&self) -> &Services {
        &self.inner.services
    }

    pub fn paths(&self) -> &AppPaths {
        &self.inner.services.paths
    }

    fn with_ports<T>(&self, run: impl FnOnce(ProbePorts<'_>) -> T) -> T {
        let services = &self.inner.services;
        let after: Vec<&dyn AfterProbe> = services
            .probe_hooks
            .iter()
            .map(|hook| hook.as_ref())
            .collect();
        run(ProbePorts {
            cost: services.cost.as_ref(),
            after: &after,
        })
    }

    /// The ports a provider probe receives (cost lines, no hooks).
    pub fn probe_ports<T>(&self, run: impl FnOnce(ProbePorts<'_>) -> T) -> T {
        run(ProbePorts {
            cost: self.inner.services.cost.as_ref(),
            after: &[],
        })
    }

    pub fn probe_detected(&self) -> Vec<ProviderOutput> {
        let catalog = self.inner.services.providers.as_ref();
        self.with_ports(|ports| crate::probe::probe_detected(catalog, ports))
    }

    pub fn probe_all(&self) -> Vec<ProviderOutput> {
        let catalog = self.inner.services.providers.as_ref();
        self.with_ports(|ports| crate::probe::probe_all(catalog, ports))
    }

    pub fn probe_one(&self, id: &str) -> Option<ProviderOutput> {
        let catalog = self.inner.services.providers.as_ref();
        self.with_ports(|ports| crate::probe::probe_one(catalog, ports, id))
    }

    /// Daemon cache when a local API is up, else a fresh probe.
    pub fn cached_or_probe(&self) -> Vec<ProviderOutput> {
        self.inner
            .services
            .local_api
            .cached()
            .unwrap_or_else(|| self.probe_detected())
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
            self.inner.services.usage.record_history(&outputs);
            outputs
        })
    }

    /// Price and context-window tables; reload explicitly after the user or
    /// a refresh changes them.
    pub fn pricing(&self) -> &dyn crate::ports::PricingSource {
        self.inner.services.pricing.as_ref()
    }

    pub fn reload_pricing(&self) {
        self.inner.services.pricing.reload();
    }

    pub fn notifier(&self) -> Arc<dyn Notifier> {
        Arc::clone(&self.inner.services.notifier)
    }
}

impl SnapshotProvider for AppContext {
    fn probe_detected(&self) -> Vec<ProviderOutput> {
        AppContext::probe_detected(self)
    }
}
