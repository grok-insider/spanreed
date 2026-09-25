//! Platform services for the tray: single instance, opening URLs and files,
//! and user-visible notifications.

use std::process::Command;
use std::thread;

use super::LOCK_FILE;
use spanreed_app::app;

/// Single-instance guard: create `…/spanreed/tray.lock` with our PID.
/// Dropped on process exit (RAII removes the file).
pub(super) struct InstanceLock {
    path: std::path::PathBuf,
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(super) fn acquire_single_instance(dir: &std::path::Path) -> Result<InstanceLock, String> {
    let dir = dir.to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir tray lock: {e}"))?;
    let path = dir.join(LOCK_FILE);
    if path.exists() {
        // Stale lock from a crashed tray: if the PID is gone, take over.
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(pid) = text.trim().parse::<u32>()
            && process_alive(pid)
        {
            return Err(format!(
                "tray already running (pid {pid}); quit the existing icon first"
            ));
        }
        let _ = std::fs::remove_file(&path);
    }
    std::fs::write(&path, format!("{}\n", std::process::id()))
        .map_err(|e| format!("write tray lock: {e}"))?;
    Ok(InstanceLock { path })
}

fn process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| {
                let s = String::from_utf8_lossy(&o.stdout);
                s.contains(&pid.to_string())
            })
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        false
    }
}

pub(super) fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let u = url.replace('\'', "''");
        let script = format!("Start-Process '{u}'");
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("powershell Start-Process: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
    #[cfg(target_os = "macos")]
    {
        let st = Command::new("open")
            .arg(url)
            .status()
            .map_err(|e| format!("open: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!("open exited {st}"))
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let st = Command::new("xdg-open")
            .arg(url)
            .status()
            .map_err(|e| format!("xdg-open: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!("xdg-open exited {st}"))
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err(format!("open url not supported: {url}"))
    }
}

/// Open a path with the OS default handler. Creates an empty file if missing.
///
/// On Windows the tray is windowless, so `cmd /C start` is unreliable; use
/// PowerShell `Start-Process` instead (same pattern as capture autostart).
pub(super) fn open_path(path: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir log dir: {e}"))?;
    }
    if !path.exists() {
        std::fs::write(path, b"").map_err(|e| format!("create log file: {e}"))?;
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let p = path.display().to_string().replace('\'', "''");
        // LiteralPath opens with the default app for .log (usually Notepad).
        let script = format!("Start-Process -LiteralPath '{p}'");
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("powershell Start-Process: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(if err.trim().is_empty() {
                format!("Start-Process failed (status {})", out.status)
            } else {
                err.trim().to_string()
            });
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let st = Command::new("open")
            .arg(path)
            .status()
            .map_err(|e| format!("open: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!("open exited {st}"))
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let st = Command::new("xdg-open")
            .arg(path)
            .status()
            .map_err(|e| format!("xdg-open: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!("xdg-open exited {st}"))
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err(format!("open not supported: {}", path.display()))
    }
}

/// User-visible notification.
///
/// Every alert goes through the platform path: notify-send, a Windows toast,
/// or macOS Notification Center. macOS and Windows also show a dialog, because
/// a successful command does not prove the banner was shown.
pub(super) fn user_notify(state: &super::state::Shared, title: &str, body: &str, modal: bool) {
    log::info!("tray notify: {title}: {body}");
    if modal {
        eprintln!("spanreed tray: {title}: {body}");
    }
    let title = title.to_string();
    let body = body.to_string();
    let ctx = super::state::context(state);
    thread::spawn(move || {
        // Deliver before asking the desktop. The desktop handshake waits, and a
        // successful plugin show can still drop the banner.
        if let Err(error) = app::notifications::deliver_user_visible(&ctx, &title, &body) {
            log::warn!("tray notify failed: {error}");
        }
        let _handed_to_desktop = app::window::hand_off_alert(&ctx, &title, &body);
    });
}
