//! Composition root: each process entry (CLI, tray, desktop host) builds one
//! `AppContext` and passes it down instead of reaching for globals.

use std::sync::Arc;
use std::time::Duration;

use crate::model::ProviderOutput;
use crate::ports::{AfterProbe, Notifier, ProbePorts, SnapshotProvider};

/// Probe cache lifetime for `snapshot(false)`.
const SNAPSHOT_TTL: Duration = Duration::from_secs(120);

/// Directories resolved once from the environment when the context is built.
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub config: std::path::PathBuf,
    pub data: std::path::PathBuf,
    pub cache: std::path::PathBuf,
}

impl AppPaths {
    pub fn from_env() -> Self {
        Self {
            config: crate::product::config_dir(),
            data: crate::product::data_dir(),
            cache: crate::product::cache_dir(),
        }
    }
}

/// Process-level services. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct AppContext {
    inner: Arc<Inner>,
}

struct Inner {
    paths: AppPaths,
    proxy: crate::desktop_runtime::ProxyControl,
    agent: crate::desktop_runtime::AgentControl,
    reviews: crate::client_configuration::Reviews,
    hosted_reviews: crate::hosted_client_configuration::HostedReviews,
    logins: crate::account_login::Logins,
    pricing: Arc<crate::pricing::Catalog>,
    cost: crate::cost::LocalCost,
    history_sync: crate::sync::HistorySync,
    notifier: Arc<dyn Notifier>,
    snapshot: fabrials_fabric::Snapshot<Vec<ProviderOutput>>,
}

impl Default for AppContext {
    fn default() -> Self {
        Self::new()
    }
}

impl AppContext {
    pub fn new() -> Self {
        let pricing = Arc::new(crate::pricing::Catalog::load());
        Self {
            inner: Arc::new(Inner {
                cost: crate::cost::LocalCost::new(Arc::clone(&pricing)),
                history_sync: crate::sync::HistorySync::new(Arc::clone(&pricing)),
                paths: AppPaths::from_env(),
                pricing,
                logins: Default::default(),
                proxy: Default::default(),
                agent: Default::default(),
                reviews: Default::default(),
                hosted_reviews: Default::default(),
                notifier: Arc::new(crate::notifications::ResetExpiryNotifier),
                snapshot: fabrials_fabric::Snapshot::new(SNAPSHOT_TTL),
            }),
        }
    }

    pub fn paths(&self) -> &AppPaths {
        &self.inner.paths
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

    /// Price and context-window tables; reload explicitly after the user or
    /// a refresh changes them.
    pub fn pricing(&self) -> &crate::pricing::Catalog {
        &self.inner.pricing
    }

    /// Device authorization in progress (desktop account login).
    pub fn logins(&self) -> &crate::account_login::Logins {
        &self.inner.logins
    }

    /// The local proxy this process owns (desktop).
    pub fn proxy(&self) -> &crate::desktop_runtime::ProxyControl {
        &self.inner.proxy
    }

    /// The agent host this process runs (desktop and tray).
    pub fn agent(&self) -> &crate::desktop_runtime::AgentControl {
        &self.inner.agent
    }

    /// Pending client configuration review (desktop).
    pub fn reviews(&self) -> &crate::client_configuration::Reviews {
        &self.inner.reviews
    }

    /// Pending hosted client configuration review (desktop).
    pub fn hosted_reviews(&self) -> &crate::hosted_client_configuration::HostedReviews {
        &self.inner.hosted_reviews
    }

    pub fn reload_pricing(&self) {
        self.inner.pricing.reload();
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
