//! Install / backup path helpers (user-scoped, no admin).

use std::path::{Path, PathBuf};

use crate::creds;

/// Directory that holds the installed binary.
pub fn install_dir() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| creds::expand("~/AppData/Local"))
            .join("spanreed")
            .join("bin")
    }
    #[cfg(not(windows))]
    {
        creds::expand("~/.local/bin")
    }
}

/// Full path to the installed `spanreed` binary.
pub fn install_bin_path() -> PathBuf {
    let name = if cfg!(windows) {
        "spanreed.exe"
    } else {
        "spanreed"
    };
    install_dir().join(name)
}

/// Parent data dir for spanreed (`…/spanreed`).
pub fn data_dir() -> PathBuf {
    creds::data_home().join("spanreed")
}

/// Backups for rewritten client configs.
pub fn backup_dir() -> PathBuf {
    data_dir().join("backups")
}

pub fn canonicalize_opt(p: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(p).ok()
}

/// True if both paths exist and resolve to the same file (or equal as strings).
pub fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (canonicalize_opt(a), canonicalize_opt(b)) {
        (Some(ca), Some(cb)) => ca == cb,
        _ => {
            // Fallback: case-insensitive compare on Windows for non-existing dest.
            let as_ = a.to_string_lossy();
            let bs = b.to_string_lossy();
            as_.eq_ignore_ascii_case(&bs)
        }
    }
}

/// Ensure the install directory is on the user PATH. Returns true if PATH was modified.
pub fn ensure_install_dir_on_user_path(dry_run: bool) -> Result<bool, String> {
    let dir = install_dir();
    let dir_s = dir.display().to_string();

    #[cfg(windows)]
    {
        ensure_windows_user_path(&dir_s, dry_run)
    }
    #[cfg(not(windows))]
    {
        // ~/.local/bin is standard on Linux/macOS; ensure it exists and warn if not on PATH.
        if !dry_run {
            std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
        }
        let on_path = std::env::var_os("PATH")
            .map(|p| {
                std::env::split_paths(&p)
                    .any(|entry| canonicalize_opt(&entry) == canonicalize_opt(&dir) || entry == dir)
            })
            .unwrap_or(false);
        if !on_path {
            eprintln!(
                "  note: {} is not on PATH in this shell; add it if `spanreed` is not found",
                dir.display()
            );
        }
        Ok(false)
    }
}

#[cfg(windows)]
fn ensure_windows_user_path(dir: &str, dry_run: bool) -> Result<bool, String> {
    use std::process::Command;

    // Read current user PATH via PowerShell.
    let out = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Environment]::GetEnvironmentVariable('Path','User')",
        ])
        .output()
        .map_err(|e| format!("powershell PATH read: {e}"))?;
    if !out.status.success() {
        return Err("could not read user PATH".into());
    }
    let current = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let already = current
        .split(';')
        .any(|p| p.eq_ignore_ascii_case(dir) || p.trim_end_matches('\\').eq_ignore_ascii_case(dir));
    if already {
        return Ok(false);
    }
    if dry_run {
        return Ok(true);
    }
    let new_path = if current.is_empty() {
        dir.to_string()
    } else {
        format!("{current};{dir}")
    };
    // Escape single quotes for PowerShell string.
    let escaped = new_path.replace('\'', "''");
    let script = format!("[Environment]::SetEnvironmentVariable('Path','{escaped}','User')");
    let set = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("powershell PATH set: {e}"))?;
    if !set.status.success() {
        return Err(format!(
            "failed to update user PATH: {}",
            String::from_utf8_lossy(&set.stderr)
        ));
    }
    Ok(true)
}

/// Write a timestamped backup of `src` into the backups dir. Returns backup path.
pub fn backup_file(src: &Path) -> Result<PathBuf, String> {
    if !src.exists() {
        return Err(format!("nothing to backup: {}", src.display()));
    }
    let dir = backup_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir backups: {e}"))?;
    let name = src.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let ts = crate::util::now_ms();
    let dest = dir.join(format!("{name}.{ts}.bak"));
    std::fs::copy(src, &dest).map_err(|e| format!("backup copy: {e}"))?;
    Ok(dest)
}
