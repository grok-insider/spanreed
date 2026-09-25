//! Where host state lives, and the one-time move from the grok-bridge layout.
//!
//! The default directory is `spanreed/agent` under the platform state root:
//! `$XDG_STATE_HOME` (or `~/.local/state`) on Linux, `~/Library/Application
//! Support` on macOS, and `%LOCALAPPDATA%` on Windows. `FABRIALS_AGENT_STATE_DIR`
//! overrides it, as do the legacy `GROK_BRIDGE_STATE_DIR` and
//! `GROK_LIGHT_STATE_DIR`.
//!
//! Installs made by the standalone `grok-bridge` kept their state in
//! `grok-bridge` under the same roots. [`migrate_legacy_state`] copies the
//! identity, enrolments and journal from there so the install id, port, and
//! therefore every bookmark and pairing origin, survive the move.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::instance::{InstanceError, ensure_private_directory};
use crate::journal::JOURNAL_FILE_NAME;
use crate::state::{self, ORIGIN_FILE_NAME, StateError};
use crate::workspace::WORKSPACES_FILE_NAME;

/// Environment variable that overrides the state directory.
pub const STATE_DIR_ENV: &str = "FABRIALS_AGENT_STATE_DIR";

/// Legacy overrides still honoured, in precedence order after [`STATE_DIR_ENV`].
pub const LEGACY_STATE_DIR_ENVS: [&str; 2] = ["GROK_BRIDGE_STATE_DIR", "GROK_LIGHT_STATE_DIR"];

/// Path components of the default state directory under the platform root.
pub const STATE_DIR_COMPONENTS: [&str; 2] = ["spanreed", "agent"];

/// Path component of the legacy grok-bridge state directory.
pub const LEGACY_STATE_DIR_COMPONENT: &str = "grok-bridge";

/// Files copied by [`migrate_legacy_state`]. The identity comes last, so an
/// interrupted migration is retried on the next start.
pub const MIGRATED_FILES: [&str; 3] = [WORKSPACES_FILE_NAME, JOURNAL_FILE_NAME, ORIGIN_FILE_NAME];

/// The resolved state directory and, when it is the platform default, the
/// legacy directory to migrate from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDirs {
    /// Directory the host uses.
    pub state_dir: PathBuf,
    /// Legacy grok-bridge directory. `None` when an override chose the state
    /// directory, so an explicit location never adopts another install's state.
    pub legacy_state_dir: Option<PathBuf>,
}

/// Errors produced while resolving the state directory.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum StateDirError {
    /// No override is set and the platform root cannot be determined.
    #[error("cannot determine the state directory: {0} is not set")]
    NoPlatformRoot(&'static str),
}

/// Resolve the state directory from the process environment.
///
/// # Errors
///
/// Returns [`StateDirError::NoPlatformRoot`] when no override is set and the
/// platform root variables are missing.
pub fn resolve_state_dirs() -> Result<StateDirs, StateDirError> {
    resolve_state_dirs_from(|name| std::env::var_os(name))
}

/// Resolve the state directory from an explicit environment lookup.
///
/// # Errors
///
/// Returns [`StateDirError::NoPlatformRoot`] when no override is set and the
/// platform root variables are missing.
pub fn resolve_state_dirs_from(
    lookup: impl Fn(&str) -> Option<OsString>,
) -> Result<StateDirs, StateDirError> {
    let non_empty = |name: &str| lookup(name).filter(|value| !value.is_empty());
    for name in std::iter::once(STATE_DIR_ENV).chain(LEGACY_STATE_DIR_ENVS) {
        if let Some(explicit) = non_empty(name) {
            return Ok(StateDirs {
                state_dir: PathBuf::from(explicit),
                legacy_state_dir: None,
            });
        }
    }
    let root = platform_state_root(&non_empty)?;
    let mut state_dir = root.clone();
    state_dir.extend(STATE_DIR_COMPONENTS);
    Ok(StateDirs {
        state_dir,
        legacy_state_dir: Some(root.join(LEGACY_STATE_DIR_COMPONENT)),
    })
}

#[cfg(target_os = "windows")]
fn platform_state_root(
    lookup: &impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf, StateDirError> {
    lookup("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or(StateDirError::NoPlatformRoot("LOCALAPPDATA"))
}

#[cfg(target_os = "macos")]
fn platform_state_root(
    lookup: &impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf, StateDirError> {
    macos_state_root(lookup)
}

/// `~/Library/Application Support`, the macOS root for per-user app state.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_state_root(lookup: &impl Fn(&str) -> Option<OsString>) -> Result<PathBuf, StateDirError> {
    lookup("HOME")
        .map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
        })
        .ok_or(StateDirError::NoPlatformRoot("HOME"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_state_root(
    lookup: &impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf, StateDirError> {
    // The XDG spec ignores relative values.
    if let Some(xdg) = lookup("XDG_STATE_HOME").map(PathBuf::from)
        && xdg.is_absolute()
    {
        return Ok(xdg);
    }
    lookup("HOME")
        .map(|home| PathBuf::from(home).join(".local").join("state"))
        .ok_or(StateDirError::NoPlatformRoot("HOME"))
}

/// What [`migrate_legacy_state`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// The new directory already has an identity; nothing was touched.
    AlreadyInitialised,
    /// The legacy directory has no identity to migrate.
    NoLegacyState,
    /// The legacy identity and its companion files were copied.
    Migrated {
        /// Install id carried over.
        install_id: String,
        /// Canonical port carried over.
        port: u16,
        /// Files copied, by name.
        files: Vec<&'static str>,
    },
}

/// Errors produced while migrating legacy state.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    /// The legacy identity exists but cannot be used.
    #[error("legacy state in {path} is unusable: {source}")]
    Legacy {
        /// Legacy directory.
        path: PathBuf,
        /// Why it is unusable.
        #[source]
        source: StateError,
    },
    /// The new state directory cannot be created or is not owner-only.
    #[error(transparent)]
    Directory(#[from] InstanceError),
    /// A file could not be copied.
    #[error("could not copy {name} from the legacy state: {source}")]
    Copy {
        /// File name.
        name: &'static str,
        /// Underlying failure.
        #[source]
        source: StateError,
    },
}

/// Copy a grok-bridge install's state into a new, uninitialised directory.
///
/// Runs only when `new_dir` has no `origin.json` and `legacy_dir` has a valid
/// one. Copies `workspaces.json` and `journal.json` when present and not
/// already in `new_dir`, then `origin.json`, owner-only. The lock, control
/// socket and anything else stay behind. A second call is a no-op.
///
/// # Errors
///
/// Returns [`MigrationError::Legacy`] when the legacy identity is malformed,
/// [`MigrationError::Directory`] when the new directory is unusable, and
/// [`MigrationError::Copy`] when a file cannot be copied.
pub fn migrate_legacy_state(
    new_dir: &Path,
    legacy_dir: &Path,
) -> Result<MigrationOutcome, MigrationError> {
    if new_dir.join(ORIGIN_FILE_NAME).exists() {
        return Ok(MigrationOutcome::AlreadyInitialised);
    }
    let identity = match state::load(legacy_dir) {
        Ok(Some(identity)) => identity,
        Ok(None) => return Ok(MigrationOutcome::NoLegacyState),
        Err(source) => {
            return Err(MigrationError::Legacy {
                path: legacy_dir.to_path_buf(),
                source,
            });
        }
    };
    ensure_private_directory(new_dir)?;

    let mut files = Vec::new();
    for name in MIGRATED_FILES {
        if name == ORIGIN_FILE_NAME {
            state::persist(new_dir, &identity)
                .map_err(|source| MigrationError::Copy { name, source })?;
            files.push(name);
            continue;
        }
        let from = legacy_dir.join(name);
        let to = new_dir.join(name);
        if !from.is_file() || to.exists() {
            continue;
        }
        copy_private(&from, new_dir, name)
            .map_err(|source| MigrationError::Copy { name, source })?;
        files.push(name);
    }
    Ok(MigrationOutcome::Migrated {
        install_id: identity.install_id,
        port: identity.port,
        files,
    })
}

fn copy_private(from: &Path, directory: &Path, name: &str) -> Result<(), StateError> {
    let bytes = std::fs::read(from).map_err(StateError::Io)?;
    let temporary = directory.join(format!("{name}.tmp"));
    state::write_private(&temporary, &bytes)?;
    std::fs::rename(&temporary, directory.join(name)).map_err(StateError::Io)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{LEGACY_STATE_DIR_COMPONENT, StateDirError, resolve_state_dirs_from};

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), OsString::from(value)))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn the_new_override_wins_over_the_legacy_ones() {
        let dirs = resolve_state_dirs_from(env(&[
            ("FABRIALS_AGENT_STATE_DIR", "/a"),
            ("GROK_BRIDGE_STATE_DIR", "/b"),
            ("GROK_LIGHT_STATE_DIR", "/c"),
        ]))
        .expect("resolve");
        assert_eq!(dirs.state_dir, PathBuf::from("/a"));
        assert_eq!(dirs.legacy_state_dir, None);
    }

    #[test]
    fn legacy_overrides_are_still_honoured_in_order() {
        let dirs = resolve_state_dirs_from(env(&[
            ("GROK_BRIDGE_STATE_DIR", "/b"),
            ("GROK_LIGHT_STATE_DIR", "/c"),
        ]))
        .expect("resolve");
        assert_eq!(dirs.state_dir, PathBuf::from("/b"));
        let dirs =
            resolve_state_dirs_from(env(&[("GROK_LIGHT_STATE_DIR", "/c")])).expect("resolve");
        assert_eq!(dirs.state_dir, PathBuf::from("/c"));
        assert_eq!(dirs.legacy_state_dir, None);
    }

    #[test]
    fn an_empty_override_is_ignored() {
        let dirs = resolve_state_dirs_from(env(&[
            ("FABRIALS_AGENT_STATE_DIR", ""),
            ("GROK_BRIDGE_STATE_DIR", "/b"),
        ]))
        .expect("resolve");
        assert_eq!(dirs.state_dir, PathBuf::from("/b"));
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn linux_defaults_to_xdg_state_home_then_local_state() {
        let dirs = resolve_state_dirs_from(env(&[("XDG_STATE_HOME", "/xdg"), ("HOME", "/home/u")]))
            .expect("resolve");
        assert_eq!(dirs.state_dir, PathBuf::from("/xdg/spanreed/agent"));
        assert_eq!(
            dirs.legacy_state_dir,
            Some(PathBuf::from("/xdg").join(LEGACY_STATE_DIR_COMPONENT))
        );

        let dirs = resolve_state_dirs_from(env(&[("XDG_STATE_HOME", "rel"), ("HOME", "/home/u")]))
            .expect("resolve");
        assert_eq!(
            dirs.state_dir,
            PathBuf::from("/home/u/.local/state/spanreed/agent")
        );
        assert_eq!(
            dirs.legacy_state_dir,
            Some(PathBuf::from("/home/u/.local/state/grok-bridge"))
        );

        assert_eq!(
            resolve_state_dirs_from(env(&[])),
            Err(StateDirError::NoPlatformRoot("HOME"))
        );
    }

    #[test]
    fn the_macos_root_is_application_support_on_every_platform() {
        assert_eq!(
            super::macos_state_root(&env(&[("HOME", "/Users/u")])).expect("root"),
            PathBuf::from("/Users/u/Library/Application Support")
        );
        assert_eq!(
            super::macos_state_root(&env(&[])),
            Err(StateDirError::NoPlatformRoot("HOME"))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_defaults_to_application_support() {
        let dirs = resolve_state_dirs_from(env(&[("HOME", "/Users/u")])).expect("resolve");
        assert_eq!(
            dirs.state_dir,
            PathBuf::from("/Users/u/Library/Application Support/spanreed/agent")
        );
        assert_eq!(
            dirs.legacy_state_dir,
            Some(PathBuf::from(
                "/Users/u/Library/Application Support/grok-bridge"
            ))
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_defaults_to_local_app_data() {
        let dirs = resolve_state_dirs_from(env(&[("LOCALAPPDATA", r"C:\Users\u\AppData\Local")]))
            .expect("resolve");
        assert_eq!(
            dirs.state_dir,
            PathBuf::from(r"C:\Users\u\AppData\Local")
                .join("spanreed")
                .join("agent")
        );
        assert_eq!(
            resolve_state_dirs_from(env(&[])),
            Err(StateDirError::NoPlatformRoot("LOCALAPPDATA"))
        );
    }
}
