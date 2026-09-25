//! Ports: the outbound interfaces of the application. `spanreed-adapters`
//! implements them; the process entry (the `spanreed` binary, the desktop
//! host) composes an `AppContext` from a [`Services`] bundle of adapters.
//!
//! Probe-time ports (`CostSource`, `AfterProbe`, `ProbePorts`, …) come from
//! `spanreed-domain`; provider drivers are [`Provider`] behind a
//! [`ProviderCatalog`]; each facade area defines the port it calls.

use std::sync::Arc;

pub use spanreed_domain::ports::*;

use crate::model::ProviderOutput;

pub use crate::app::accounts::{AccountStore, DeviceLogins};
pub use crate::app::addons::AddonHost;
pub use crate::app::agent::AgentRuntime;
pub use crate::app::capture::CaptureService;
pub use crate::app::clients::ClientConfigurator;
pub use crate::app::fabrials::FabrialsLink;
pub use crate::app::local_api::LocalApiServer;
pub use crate::app::migration::Migrations;
pub use crate::app::notifications::Notifications;
pub use crate::app::profiles::StatusBarProfiles;
pub use crate::app::proxy::ProxyRuntime;
pub use crate::app::routing::RoutingStore;
pub use crate::app::setup::Installer;
pub use crate::app::sharing::Sharing;
pub use crate::app::sync::PrivateSync;
pub use crate::app::updates::SelfUpdater;
pub use crate::app::usage::{PricingSource, UsageStore};
pub use crate::app::window::DesktopBridge;

/// A usage provider driver (Claude, Codex, ...).
pub trait Provider: Send + Sync {
    /// Stable id used on the CLI and local API (e.g. "claude").
    fn id(&self) -> &'static str;

    /// Human-friendly name (e.g. "Claude").
    fn name(&self) -> &'static str;

    /// Whether this provider has any local signal (creds/state) on this machine.
    /// Used to hide providers the user doesn't use. Probing a non-detected
    /// provider is allowed but typically yields an error line.
    fn detect(&self) -> bool;

    /// Fetch current usage. Implementations should return an error *line*
    /// (via `ProviderOutput::error`) rather than panicking.
    fn probe(&self, ports: ProbePorts<'_>) -> ProviderOutput;
}

/// A row of `spanreed list` that is not a built-in provider.
pub struct ListedSource {
    pub id: String,
    pub name: String,
    pub state: &'static str,
}

/// Provider drivers: built-in providers, host accounts and addon providers.
pub trait ProviderCatalog: Send + Sync {
    /// Built-in providers, in display order.
    fn providers(&self) -> Vec<Box<dyn Provider>>;
    /// Every host account (Grok, Codex, ...).
    fn account_outputs(&self, ports: ProbePorts<'_>) -> Vec<ProviderOutput>;
    /// One host account by output id, probing only as far as needed.
    fn account_output(&self, ports: ProbePorts<'_>, id: &str) -> Option<ProviderOutput>;
    /// Detected addon providers.
    fn addon_outputs(&self) -> Vec<ProviderOutput>;
    fn addon_output(&self, id: &str) -> Option<ProviderOutput>;
    /// Host accounts and addon providers for `spanreed list`.
    fn listed(&self) -> Vec<ListedSource>;
}

/// Directories resolved once from the environment when the context is built.
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub config: std::path::PathBuf,
    pub data: std::path::PathBuf,
    pub cache: std::path::PathBuf,
}

/// Everything an `AppContext` needs, supplied by the composition root.
#[derive(Clone)]
pub struct Services {
    pub paths: AppPaths,
    pub providers: Arc<dyn ProviderCatalog>,
    /// Run in order after every probe (bookkeeping, private history sync, …).
    pub probe_hooks: Vec<Arc<dyn AfterProbe>>,
    pub cost: Arc<dyn CostSource>,
    /// Background reset-expiry alerts for the local API refresh.
    pub notifier: Arc<dyn Notifier>,
    pub pricing: Arc<dyn PricingSource>,
    pub usage: Arc<dyn UsageStore>,
    pub accounts: Arc<dyn AccountStore>,
    pub logins: Arc<dyn DeviceLogins>,
    pub routing: Arc<dyn RoutingStore>,
    pub notifications: Arc<dyn Notifications>,
    pub sharing: Arc<dyn Sharing>,
    pub sync: Arc<dyn PrivateSync>,
    pub fabrials: Arc<dyn FabrialsLink>,
    pub migrations: Arc<dyn Migrations>,
    pub clients: Arc<dyn ClientConfigurator>,
    pub proxy: Arc<dyn ProxyRuntime>,
    pub agent: Arc<dyn AgentRuntime>,
    pub capture: Arc<dyn CaptureService>,
    pub installer: Arc<dyn Installer>,
    pub updates: Arc<dyn SelfUpdater>,
    pub window: Arc<dyn DesktopBridge>,
    pub addons: Arc<dyn AddonHost>,
    pub profiles: Arc<dyn StatusBarProfiles>,
    pub local_api: Arc<dyn LocalApiServer>,
}
