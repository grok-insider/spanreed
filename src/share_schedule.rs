//! Daily anonymous share schedule: 23:00 Europe/Madrid.
//!
//! Installed by `spanreed setup` (default on) so contributions to the
//! public plan pool happen automatically — no login, no manual share.
//!
//! Platforms:
//! - Linux: systemd user timer (`OnCalendar=… Europe/Madrid`)
//! - macOS: LaunchAgent calendar interval (23:00 local; use system TZ=Madrid for Spain)
//! - Windows: Scheduled Task daily 23:00 local (Spain hosts should use Romance Standard Time)

use std::path::PathBuf;
use std::process::Command;

use crate::setup;

const HOUR: u8 = 23;
const MINUTE: u8 = 0;
/// IANA zone for systemd; human label for status strings.
pub const TZ_LABEL: &str = "Europe/Madrid";

pub fn kind_label() -> &'static str {
    platform::kind_label()
}

pub fn status() -> String {
    platform::status()
}

/// Register daily share at 23:00 Europe/Madrid (or local 23:00 where the OS lacks IANA zones).
pub fn enable(dry_run: bool) -> Result<String, String> {
    let bin = resolve_bin()?;
    platform::enable(&bin, dry_run)
}

pub fn disable(dry_run: bool) -> Result<String, String> {
    platform::disable(dry_run)
}

fn resolve_bin() -> Result<PathBuf, String> {
    let installed = setup::install_bin_path();
    if installed.exists() {
        return Ok(installed);
    }
    std::env::current_exe().map_err(|e| format!("current_exe: {e}"))
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::io::Write;

    const SERVICE: &str = "spanreed-share.service";
    const TIMER: &str = "spanreed-share.timer";

    pub fn kind_label() -> &'static str {
        "systemd-user-timer"
    }

    fn unit_dir() -> PathBuf {
        crate::creds::expand("~/.config/systemd/user")
    }

    fn service_path() -> PathBuf {
        unit_dir().join(SERVICE)
    }

    fn timer_path() -> PathBuf {
        unit_dir().join(TIMER)
    }

    pub fn status() -> String {
        let out = Command::new("systemctl")
            .args(["--user", "is-active", TIMER])
            .output();
        match out {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() {
                    format!("systemd-user ({TIMER}: unknown)")
                } else {
                    format!("systemd-user ({TIMER}: {s}, daily {HOUR:02}:{MINUTE:02} {TZ_LABEL})")
                }
            }
            Err(_) => format!("systemd-user (systemctl missing; {TIMER})"),
        }
    }

    pub fn enable(bin: &std::path::Path, dry_run: bool) -> Result<String, String> {
        let dir = unit_dir();
        let svc = service_path();
        let tmr = timer_path();
        let service_body = format!(
            "[Unit]\n\
             Description=spanreed anonymous daily share to grokinsider.net\n\
             \n\
             [Service]\n\
             Type=oneshot\n\
             ExecStart={bin} share\n\
             Nice=10\n",
            bin = bin.display()
        );
        let timer_body = format!(
            "[Unit]\n\
             Description=Daily spanreed share at 23:00 Europe/Madrid\n\
             \n\
             [Timer]\n\
             OnCalendar=*-*-* {HOUR:02}:{MINUTE:02}:00 {TZ_LABEL}\n\
             Persistent=true\n\
             \n\
             [Install]\n\
             WantedBy=timers.target\n"
        );
        if dry_run {
            return Ok(format!(
                "would write {} + {} and enable timer",
                svc.display(),
                tmr.display()
            ));
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir systemd user: {e}"))?;
        {
            let mut f = std::fs::File::create(&svc).map_err(|e| format!("write service: {e}"))?;
            f.write_all(service_body.as_bytes())
                .map_err(|e| format!("write service: {e}"))?;
        }
        {
            let mut f = std::fs::File::create(&tmr).map_err(|e| format!("write timer: {e}"))?;
            f.write_all(timer_body.as_bytes())
                .map_err(|e| format!("write timer: {e}"))?;
        }
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();
        let out = Command::new("systemctl")
            .args(["--user", "enable", "--now", TIMER])
            .output()
            .map_err(|e| format!("systemctl enable: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "systemctl enable {TIMER}: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(format!(
            "enabled {TIMER} (daily {HOUR:02}:{MINUTE:02} {TZ_LABEL})"
        ))
    }

    pub fn disable(dry_run: bool) -> Result<String, String> {
        if dry_run {
            return Ok(format!("would disable/remove {TIMER} + {SERVICE}"));
        }
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", TIMER])
            .output();
        let _ = std::fs::remove_file(timer_path());
        let _ = std::fs::remove_file(service_path());
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();
        Ok(format!("disabled {TIMER}"))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::io::Write;

    const LABEL: &str = "net.grokinsider.spanreed-share";

    pub fn kind_label() -> &'static str {
        "launchd"
    }

    fn plist_path() -> PathBuf {
        crate::creds::expand(&format!("~/Library/LaunchAgents/{LABEL}.plist"))
    }

    pub fn status() -> String {
        let path = plist_path();
        if !path.exists() {
            return format!("launchd ({LABEL}: not installed)");
        }
        let out = Command::new("launchctl").args(["list", LABEL]).output();
        match out {
            Ok(o) if o.status.success() => {
                format!("launchd ({LABEL}: loaded, daily {HOUR:02}:{MINUTE:02} local ≈ {TZ_LABEL})")
            }
            _ => format!("launchd ({LABEL}: plist present, not loaded)"),
        }
    }

    pub fn enable(bin: &std::path::Path, dry_run: bool) -> Result<String, String> {
        let path = plist_path();
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{bin}</string>
    <string>share</string>
  </array>
  <key>StartCalendarInterval</key>
  <dict>
    <key>Hour</key>
    <integer>{HOUR}</integer>
    <key>Minute</key>
    <integer>{MINUTE}</integer>
  </dict>
  <key>RunAtLoad</key>
  <false/>
</dict>
</plist>
"#,
            bin = bin.display()
        );
        if dry_run {
            return Ok(format!("would write {} and launchctl load", path.display()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir LaunchAgents: {e}"))?;
        }
        {
            let mut f = std::fs::File::create(&path).map_err(|e| format!("write plist: {e}"))?;
            f.write_all(body.as_bytes())
                .map_err(|e| format!("write plist: {e}"))?;
        }
        let _ = Command::new("launchctl")
            .args(["unload", &path.display().to_string()])
            .output();
        let out = Command::new("launchctl")
            .args(["load", &path.display().to_string()])
            .output()
            .map_err(|e| format!("launchctl load: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "launchctl load: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(format!(
            "enabled {LABEL} (daily {HOUR:02}:{MINUTE:02} local; set TZ={TZ_LABEL} for Spain)"
        ))
    }

    pub fn disable(dry_run: bool) -> Result<String, String> {
        let path = plist_path();
        if dry_run {
            return Ok(format!("would unload/remove {}", path.display()));
        }
        let _ = Command::new("launchctl")
            .args(["unload", &path.display().to_string()])
            .output();
        let _ = std::fs::remove_file(&path);
        Ok(format!("disabled {LABEL}"))
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::os::windows::process::CommandExt;

    const TASK_NAME: &str = "OpenUsageShare";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn kind_label() -> &'static str {
        "windows-task"
    }

    pub fn status() -> String {
        let out = Command::new("schtasks")
            .args(["/Query", "/TN", TASK_NAME, "/FO", "LIST", "/V"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        match out {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                let st = text
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("status:"))
                    .map(|l| l.trim().to_string())
                    .unwrap_or_else(|| "present".into());
                format!(
                    "windows-task ({TASK_NAME}: {st}, daily {HOUR:02}:{MINUTE:02} local ≈ {TZ_LABEL})"
                )
            }
            _ => format!("windows-task ({TASK_NAME}: missing)"),
        }
    }

    pub fn enable(bin: &std::path::Path, dry_run: bool) -> Result<String, String> {
        // Quote path for cmd; schtasks /TR runs via CreateProcess.
        let tr = format!("\"{}\" share", bin.display());
        let st = format!("{HOUR:02}:{MINUTE:02}");
        if dry_run {
            return Ok(format!(
                "would register schtasks {TASK_NAME} DAILY {st} → {tr}"
            ));
        }
        let _ = Command::new("schtasks")
            .args(["/Delete", "/TN", TASK_NAME, "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        let out = Command::new("schtasks")
            .args([
                "/Create", "/TN", TASK_NAME, "/SC", "DAILY", "/ST", &st, "/RL", "LIMITED", "/F",
                "/TR", &tr,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("schtasks: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "schtasks create {TASK_NAME}: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(format!(
            "enabled {TASK_NAME} (daily {st} local; use Windows TZ Spain/Madrid ≈ {TZ_LABEL})"
        ))
    }

    pub fn disable(dry_run: bool) -> Result<String, String> {
        if dry_run {
            return Ok(format!("would delete schtasks {TASK_NAME}"));
        }
        let _ = Command::new("schtasks")
            .args(["/Delete", "/TN", TASK_NAME, "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        Ok(format!("disabled {TASK_NAME}"))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use super::*;

    pub fn kind_label() -> &'static str {
        "unsupported"
    }
    pub fn status() -> String {
        "share schedule unsupported on this OS".into()
    }
    pub fn enable(_bin: &std::path::Path, _dry_run: bool) -> Result<String, String> {
        Err("share schedule not supported on this OS".into())
    }
    pub fn disable(_dry_run: bool) -> Result<String, String> {
        Ok("nothing to disable".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_is_23_europe_madrid() {
        assert_eq!(HOUR, 23);
        assert_eq!(MINUTE, 0);
        assert_eq!(TZ_LABEL, "Europe/Madrid");
    }
}
