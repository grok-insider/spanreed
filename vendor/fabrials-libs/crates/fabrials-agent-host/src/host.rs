//! The embeddable host: configuration, start-up, and the serve loop.
//!
//! [`serve`] is the whole host in one call: it claims the state directory,
//! migrates grok-bridge state when asked to, binds the canonical loopback
//! origin, opens the owner-only control channel, supervises the agent, and
//! serves `light.local.v1` until a shutdown is requested. [`start`] does the
//! same but returns once the host is listening, so the caller can report the
//! origin before waiting on [`RunningHost::wait`].

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::acp::AgentCommand;
use crate::control::{self, ControlError};
use crate::instance::{InstanceError, InstanceLock};
use crate::journal::JournalError;
use crate::origin::{ALLOWED_WEB_ORIGINS, LocalOrigin};
use crate::picker::{DirectoryPicker, platform_picker};
use crate::server::{self, DEFAULT_AGENT_PROGRAM, HostState, RelaunchPolicy};
use crate::state::{self, StateError};
use crate::state_dir::{MigrationError, MigrationOutcome, migrate_legacy_state};
use crate::workspace::{self, WorkspaceError};

/// Name and version of the program embedding the host.
///
/// Reported by `/healthz` as `host` and `hostVersion`, and used to prefix
/// diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostIdentity {
    /// Program name, e.g. `spanreed`.
    pub name: &'static str,
    /// Program version.
    pub version: &'static str,
}

impl HostIdentity {
    /// Identity of an embedding program.
    #[must_use]
    pub const fn new(name: &'static str, version: &'static str) -> Self {
        Self { name, version }
    }
}

impl Default for HostIdentity {
    /// This crate's own name and version.
    fn default() -> Self {
        Self::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
    }
}

/// Everything [`serve`] needs.
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// Owner-only state directory (identity, journal, enrolments, lock,
    /// control channel).
    pub state_dir: PathBuf,
    /// grok-bridge state directory to migrate from when `state_dir` has no
    /// identity yet. `None` skips migration.
    pub legacy_state_dir: Option<PathBuf>,
    /// Grok Build CLI executable, run as `<program> agent --no-leader stdio`.
    pub agent_program: String,
    /// Extra environment for the agent process.
    pub agent_env: Vec<(String, String)>,
    /// Hosted document origins allowed to call the loopback API. The first
    /// one is used in pairing URLs; when empty, pairing URLs point at the
    /// loopback SPA.
    pub allowed_origins: Vec<String>,
    /// Embedding program, reported by `/healthz`.
    pub identity: HostIdentity,
    /// Directory picker; `None` selects [`platform_picker`].
    pub picker: Option<Arc<dyn DirectoryPicker>>,
    /// Backoff for relaunching an agent that exited.
    pub relaunch: RelaunchPolicy,
}

impl HostConfig {
    /// Defaults: agent `grok`, origin `https://desktop.grok.me`, platform
    /// picker, 1 s → 30 s relaunch backoff, no migration.
    #[must_use]
    pub fn new(state_dir: impl Into<PathBuf>, identity: HostIdentity) -> Self {
        Self {
            state_dir: state_dir.into(),
            legacy_state_dir: None,
            agent_program: DEFAULT_AGENT_PROGRAM.to_owned(),
            agent_env: Vec::new(),
            allowed_origins: ALLOWED_WEB_ORIGINS
                .iter()
                .map(|origin| (*origin).to_owned())
                .collect(),
            identity,
            picker: None,
            relaunch: RelaunchPolicy::default(),
        }
    }
}

/// Errors that stop the host from starting or serving.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The state directory is unusable, or another host holds it.
    #[error(transparent)]
    Instance(#[from] InstanceError),
    /// Legacy state could not be migrated.
    #[error(transparent)]
    Migration(#[from] MigrationError),
    /// The persisted identity is unusable.
    #[error(transparent)]
    State(#[from] StateError),
    /// The canonical port is taken. Transient: identity and pairings are kept.
    #[error("port {port} is unavailable ({source}); the origin and pairings are kept")]
    Bind {
        /// Canonical port.
        port: u16,
        /// Bind failure.
        #[source]
        source: std::io::Error,
    },
    /// The journal cannot be opened, so intent could not be recorded.
    #[error("journal is unusable: {0}")]
    Journal(#[from] JournalError),
    /// The workspace index is unusable.
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    /// The control channel could not be opened.
    #[error(transparent)]
    Control(#[from] ControlError),
    /// The HTTP server stopped with an error.
    #[error("the loopback server stopped: {0}")]
    Serve(#[source] std::io::Error),
}

/// A host that is listening. Dropping it without [`RunningHost::wait`] leaves
/// the server task running until the runtime ends.
#[derive(Debug)]
pub struct RunningHost {
    state: Arc<HostState>,
    migration: MigrationOutcome,
    ipv6: bool,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
    supervisor: tokio::task::JoinHandle<()>,
    lock: InstanceLock,
}

impl RunningHost {
    /// Canonical loopback origin.
    #[must_use]
    pub fn origin(&self) -> &LocalOrigin {
        &self.state.origin
    }

    /// Whether `[::1]` was bound next to `127.0.0.1`.
    #[must_use]
    pub fn ipv6_bound(&self) -> bool {
        self.ipv6
    }

    /// What the legacy-state migration did at start-up.
    #[must_use]
    pub fn migration(&self) -> &MigrationOutcome {
        &self.migration
    }

    /// Shared host state.
    #[must_use]
    pub fn state(&self) -> &Arc<HostState> {
        &self.state
    }

    /// Ask the host to stop; [`RunningHost::wait`] then returns.
    pub fn shutdown(&self) {
        self.state.request_shutdown();
    }

    /// Serve until a shutdown is requested (control `stop` or
    /// [`RunningHost::shutdown`]), then stop the agent and release the lock.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Serve`] when the server stopped with an error.
    pub async fn wait(self) -> Result<(), Error> {
        let result = match self.server.await {
            Ok(result) => result.map_err(Error::Serve),
            Err(join) => Err(Error::Serve(std::io::Error::other(join))),
        };
        self.state.request_shutdown();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.supervisor).await;
        drop(self.lock);
        result
    }
}

/// Start the host and return once it is listening.
///
/// # Errors
///
/// See [`Error`]. A busy port is [`Error::Bind`] and never rotates identity.
pub async fn start(config: HostConfig) -> Result<RunningHost, Error> {
    let lock = InstanceLock::acquire(&config.state_dir)?;
    let directory = lock.directory().to_path_buf();
    let migration = match &config.legacy_state_dir {
        Some(legacy) if legacy != &directory => migrate_legacy_state(&directory, legacy)?,
        _ => MigrationOutcome::NoLegacyState,
    };
    let identity = state::load_or_create(&directory)?;
    let origin = identity.origin()?;
    let listeners = server::bind(&origin).await.map_err(|source| Error::Bind {
        port: origin.port(),
        source,
    })?;
    let ipv6 = listeners.v6.is_some();

    let picker = config.picker.unwrap_or_else(platform_picker);
    let state = Arc::new(
        HostState::new(origin)
            .with_identity(config.identity)
            .with_allowed_origins(config.allowed_origins)
            .with_agent_program(config.agent_program.clone())
            .with_persistence(directory.clone(), picker)?,
    );
    let index = workspace::load(&directory)?;
    state.load_workspaces(&index).await;

    let control_listener = control::bind(&directory)?;
    tokio::spawn(control::serve(control_listener, Arc::clone(&state)));

    let command = AgentCommand::new(config.agent_program).with_env(config.agent_env);
    let supervisor = state.supervise_agent(command, config.relaunch);
    let server = tokio::spawn(server::serve(listeners, Arc::clone(&state)));

    Ok(RunningHost {
        state,
        migration,
        ipv6,
        server,
        supervisor,
        lock,
    })
}

/// Run the host until a shutdown is requested.
///
/// # Errors
///
/// See [`start`] and [`RunningHost::wait`].
pub async fn serve(config: HostConfig) -> Result<(), Error> {
    start(config).await?.wait().await
}
