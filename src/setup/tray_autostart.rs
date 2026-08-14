//! Optional login autostart for `spanreed tray` (separate from capture).
//!
//! Default policy in setup: on for Windows/macOS when capture service is on;
//! off on Linux (Waybar-first).

use std::path::Path;

#[cfg(any(windows, target_os = "macos"))]
use std::process::Command;

use super::paths;

#[cfg(windows)]
const RUN_VALUE: &str = "SpanreedTray";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn kind_label() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "xdg-autostart"
    }
    #[cfg(target_os = "macos")]
    {
        "launchd-tray"
    }
    #[cfg(target_os = "windows")]
    {
        "hkcu-run-tray"
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        "unsupported"
    }
}

/// Default answer for the setup prompt.
pub fn default_enabled(capture_service_on: bool) -> bool {
    if !capture_service_on {
        return false;
    }
    // Linux users typically use Waybar; tray is opt-in.
    !cfg!(target_os = "linux")
}

pub fn status() -> String {
    platform::status()
}

pub fn enable(bin: &Path, dry_run: bool) -> Result<String, String> {
    platform::enable(bin, dry_run)
}

pub fn disable(dry_run: bool) -> Result<String, String> {
    platform::disable(dry_run)
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::process::CommandExt;

    pub fn status() -> String {
        if read_run_key() {
            format!("hkcu-run ({RUN_VALUE}: installed)")
        } else {
            format!("not installed (no HKCU Run {RUN_VALUE})")
        }
    }

    pub fn enable(bin: &Path, dry_run: bool) -> Result<String, String> {
        let val = format!("\"{}\" tray", bin.display());
        if dry_run {
            return Ok(format!("would set HKCU Run {RUN_VALUE}={val}"));
        }
        set_run_key(bin)?;
        // Start once now (hidden).
        let path = bin.display().to_string().replace('\'', "''");
        let script =
            format!("Start-Process -FilePath '{path}' -ArgumentList 'tray' -WindowStyle Hidden");
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        Ok(format!("tray autostart: HKCU Run {RUN_VALUE}"))
    }

    pub fn disable(dry_run: bool) -> Result<String, String> {
        if dry_run {
            return Ok(format!("would remove HKCU Run {RUN_VALUE}"));
        }
        clear_run_key()?;
        Ok(format!("tray autostart removed ({RUN_VALUE})"))
    }

    fn read_run_key() -> bool {
        let script = format!(
            "$ErrorActionPreference='Stop'; \
             $p='HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run'; \
             if (Get-ItemProperty -Path $p -Name '{RUN_VALUE}' -ErrorAction SilentlyContinue) {{ '1' }} else {{ '0' }}"
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok();
        out.map(|o| String::from_utf8_lossy(&o.stdout).contains('1'))
            .unwrap_or(false)
    }

    fn set_run_key(bin: &Path) -> Result<(), String> {
        let val = format!("\"{}\" tray", bin.display()).replace('\'', "''");
        let script = format!(
            "$ErrorActionPreference='Stop'; \
             Set-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' \
               -Name '{RUN_VALUE}' -Value '{val}'"
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("powershell Run key: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "HKCU Run tray set failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    }

    fn clear_run_key() -> Result<(), String> {
        let script = format!(
            "Remove-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' \
             -Name '{RUN_VALUE}' -ErrorAction SilentlyContinue"
        );
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    const LABEL: &str = "net.grokinsider.spanreed-tray";

    fn plist_path() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist"))
    }

    pub fn status() -> String {
        if plist_path().exists() {
            format!("launchd ({LABEL}: installed)")
        } else {
            format!("not installed ({LABEL})")
        }
    }

    pub fn enable(bin: &Path, dry_run: bool) -> Result<String, String> {
        let path = plist_path();
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>tray</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><false/>
</dict>
</plist>
"#,
            bin.display()
        );
        if dry_run {
            return Ok(format!("would write {}", path.display()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
        let _ = Command::new("launchctl")
            .args(["load", "-w", &path.display().to_string()])
            .output();
        Ok(format!("tray autostart: {}", path.display()))
    }

    pub fn disable(dry_run: bool) -> Result<String, String> {
        let path = plist_path();
        if dry_run {
            return Ok(format!("would remove {}", path.display()));
        }
        let _ = Command::new("launchctl")
            .args(["unload", "-w", &path.display().to_string()])
            .output();
        let _ = std::fs::remove_file(&path);
        Ok(format!("tray autostart removed ({LABEL})"))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;

    fn desktop_path() -> std::path::PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("autostart")
            .join("spanreed-tray.desktop")
    }

    pub fn status() -> String {
        if desktop_path().exists() {
            "xdg-autostart (spanreed-tray.desktop)".into()
        } else {
            "not installed (no spanreed-tray.desktop)".into()
        }
    }

    pub fn enable(bin: &Path, dry_run: bool) -> Result<String, String> {
        let path = desktop_path();
        let body = format!(
            "[Desktop Entry]\nType=Application\nName=spanreed tray\n\
             Exec=\"{}\" tray\nX-GNOME-Autostart-enabled=true\n",
            bin.display()
        );
        if dry_run {
            return Ok(format!("would write {}", path.display()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
        Ok(format!("tray autostart: {}", path.display()))
    }

    pub fn disable(dry_run: bool) -> Result<String, String> {
        let path = desktop_path();
        if dry_run {
            return Ok(format!("would remove {}", path.display()));
        }
        let _ = std::fs::remove_file(&path);
        Ok("tray autostart removed".into())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use super::*;
    pub fn status() -> String {
        "unsupported".into()
    }
    pub fn enable(_bin: &Path, _dry_run: bool) -> Result<String, String> {
        Err("tray autostart unsupported on this OS".into())
    }
    pub fn disable(_dry_run: bool) -> Result<String, String> {
        Ok("noop".into())
    }
}

/// Resolve binary path for tray (same rules as capture service).
pub fn resolve_bin() -> Result<std::path::PathBuf, String> {
    let installed = paths::install_bin_path();
    if installed.is_file() {
        return Ok(installed);
    }
    std::env::current_exe().map_err(|e| format!("current_exe: {e}"))
}
