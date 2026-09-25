//! Agent host: supervises the user's Grok Build CLI over ACP and serves the
//! `light.local.v1` loopback API that `https://desktop.grok.me` drives.
//!
//! This is the library form of `grok-bridge`. An embedding program (Spanreed)
//! calls [`serve`] with a [`HostConfig`], or exposes [`cli::run`] as a
//! subcommand. The host executes the user's own CLI with the user's own
//! `GROK_HOME` and authority; it never ships, downloads, or sandboxes one.
//!
//! Layering:
//!
//! - [`host`] — configuration, start-up, and the serve loop.
//! - [`cli`] — `serve`/`open`/`status`/`doctor`/`stop`/`repair`/`workspace`.
//! - [`state_dir`] — state directory resolution and grok-bridge migration.
//! - [`acp`] — the supervised stdio agent transport of ADR light 0003.
//! - [`assets`] — the SPA bundle embedded in the binary.
//! - [`bounds`] — every limit, defined once.
//! - [`instance`] — single-instance lock and owner-only state directory.
//! - [`origin`] — canonical loopback origin and exact `Host`/`Origin` checks.
//! - [`pairing`] — single-use nonce, browser session tokens, CSRF.
//! - [`lease`] — the single controlling tab and its monotonic epoch.
//! - [`protocol`] — the closed `light.local.v1` operation and event surface.
//! - [`permission`] — the native ACP option matching of ADR light 0007.
//! - [`picker`] — the host-owned directory picker.
//! - [`server`] — the loopback HTTP surface, its origin policy, and agent
//!   supervision.
//! - [`control`] — the owner-only socket that mints pairing nonces.
//! - [`state`] — durable install identity, so a bookmark survives a restart.
//! - [`workspace`] — host-owned enrolment; the browser only sees opaque ids.
//! - [`session_catalog`] — read-only list/rehydrate of Grok Build sessions on disk.
//! - [`dispatch`] — the closed operation surface driving one agent session.
//! - [`journal`] — intent before effect, idempotency, event cursor, and
//!   `interrupted_needs_review`.
//! - [`projection`] — pure ACP `session/update` → `light.local.v1` events.
//! - [`xai_runtime`] — pure decode of `x.ai/task_*` (and related) notifications.
//! - [`session_runtime`] — host-side tasks/workflows runtime bag per session.
//! - [`cli_matrix`] — qualified Grok Build CLI version floor (product integrity).
//! - [`repair`] — out-of-band session history repair (light ADR 0015).
//!
//! ADR and protocol references ("ADR light NNNN", `docs/protocol.md`) point at
//! the grok-desktop-portable repository, which owns the SPA and the contract.

pub mod acp;
pub mod assets;
pub mod bash;
pub mod bounds;
pub mod cli;
pub mod cli_matrix;
pub mod context;
pub mod control;
pub mod dispatch;
pub mod host;
pub mod instance;
pub mod integrations;
pub mod journal;
pub mod lease;
pub mod models;
pub mod origin;
pub mod pairing;
pub mod permission;
pub mod picker;
pub mod projection;
pub mod protocol;
pub mod repair;
pub mod review;
pub mod server;
pub mod session_catalog;
pub mod session_runtime;
pub mod state;
pub mod state_dir;
pub mod tools;
#[cfg(windows)]
pub mod win_acl;
#[cfg(windows)]
pub mod win_job;
pub mod workspace;
pub mod xai_runtime;

pub use cli::CliDefaults;
pub use host::{Error, HostConfig, HostIdentity, RunningHost, serve, start};
pub use server::RelaunchPolicy;
pub use state_dir::{MigrationError, MigrationOutcome, migrate_legacy_state};

/// Host wall clock in milliseconds since the Unix epoch.
///
/// One definition for the whole crate: pairing expiry, session order, and the
/// control socket all need the same clock, and three private copies would be
/// three chances for them to disagree. A clock that cannot be read yields `0`
/// rather than panicking, which reads as "the beginning of time" and so never
/// makes something look newer than it is.
#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}
