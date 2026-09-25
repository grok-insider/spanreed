//! Installing the binary on the user PATH and creating the ledger store.

use super::{install_bin_path, paths};

pub(super) struct InstallResult {
    pub(super) path: std::path::PathBuf,
    pub(super) path_updated: bool,
}

pub(super) fn install_cli(
    dry_run: bool,
    force_from_current: bool,
) -> Result<InstallResult, String> {
    let dest = install_bin_path();
    let current = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    // Same path (e.g. install.ps1 already copied us into place, then re-exec'd setup):
    // never try to copy a running Windows PE onto itself (error 32).
    let same_file = paths::same_file(&current, &dest);
    let need_copy = !same_file
        && (force_from_current
            || !dest.exists()
            || paths::canonicalize_opt(&current) != paths::canonicalize_opt(&dest));

    if dry_run {
        return Ok(InstallResult {
            path: dest,
            path_updated: false,
        });
    }

    if need_copy {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir install dir: {e}"))?;
        }
        match std::fs::copy(&current, &dest) {
            Ok(_) => {}
            Err(e) if dest.exists() && same_file_or_busy(&e) => {
                // Dest already good / locked by our own process — treat as installed.
            }
            Err(e) => {
                return Err(format!(
                    "copy {} → {}: {e}",
                    current.display(),
                    dest.display()
                ));
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&dest)
                .map_err(|e| format!("stat dest: {e}"))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&dest, perms).map_err(|e| format!("chmod: {e}"))?;
        }
    }

    let path_updated = paths::ensure_install_dir_on_user_path(dry_run)?;
    Ok(InstallResult {
        path: dest,
        path_updated,
    })
}

fn same_file_or_busy(err: &std::io::Error) -> bool {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied => true,
        _ => {
            let msg = err.to_string().to_ascii_lowercase();
            msg.contains("being used by another process")
                || msg.contains("os error 32")
                || msg.contains("text file busy")
        }
    }
}

pub(super) fn ensure_ledger(dry_run: bool) -> Result<std::path::PathBuf, String> {
    let path = crate::grok_ledger::ledger_path();
    if let Some(parent) = path.parent() {
        if dry_run {
            return Ok(path);
        }
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir ledger dir: {e}"))?;
    }
    if !dry_run {
        crate::grok_ledger::ensure_store()?;
    }
    Ok(path)
}
