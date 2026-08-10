//! Daily anonymous share schedule (once per product day).
//!
//! Installed by `spanreed setup` (default on) so contributions to the
//! public plan pool happen automatically — no login, no manual share.
//!
//! **Semantics:** at most one successful sample per Europe/Madrid product day
//! (client due-gate in [`crate::share`]). OS jobs may fire more often (evening
//! timer + login / missed-run catch-up); extras no-op.
//!
//! Platforms:
//! - Linux: systemd user timer (`OnCalendar=… Europe/Madrid`, `Persistent=true`)
//!   + oneshot at session start (`default.target`)
//! - macOS: LaunchAgent calendar 23:00 local + `RunAtLoad` for login catch-up
//! - Windows: Scheduled Task — daily 23:00 local + logon, `StartWhenAvailable`

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
    let sched = platform::status();
    match crate::share::last_shared_day() {
        Some(d) => format!("{sched}; last shared {d}"),
        None => format!("{sched}; last shared: never"),
    }
}

/// Register daily share (evening timer + catch-up when the machine is on).
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
    /// Session-start catch-up (due-gate makes this safe if the timer already ran).
    const LOGIN_SERVICE: &str = "spanreed-share-login.service";

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

    fn login_service_path() -> PathBuf {
        unit_dir().join(LOGIN_SERVICE)
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
                    format!(
                        "systemd-user ({TIMER}: {s}, daily {HOUR:02}:{MINUTE:02} {TZ_LABEL} + login catch-up)"
                    )
                }
            }
            Err(_) => format!("systemd-user (systemctl missing; {TIMER})"),
        }
    }

    pub fn enable(bin: &std::path::Path, dry_run: bool) -> Result<String, String> {
        let dir = unit_dir();
        let svc = service_path();
        let tmr = timer_path();
        let login = login_service_path();
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
        // Runs once when the user session reaches default.target (login).
        // Combined with Persistent= timer, covers machines powered off at 23:00.
        let login_body = format!(
            "[Unit]\n\
             Description=spanreed share catch-up at login\n\
             After=network-online.target\n\
             Wants=network-online.target\n\
             \n\
             [Service]\n\
             Type=oneshot\n\
             ExecStart={bin} share\n\
             Nice=10\n\
             \n\
             [Install]\n\
             WantedBy=default.target\n",
            bin = bin.display()
        );
        if dry_run {
            return Ok(format!(
                "would write {} + {} + {} and enable timer/login",
                svc.display(),
                tmr.display(),
                login.display()
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
        {
            let mut f =
                std::fs::File::create(&login).map_err(|e| format!("write login service: {e}"))?;
            f.write_all(login_body.as_bytes())
                .map_err(|e| format!("write login service: {e}"))?;
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
        let out_login = Command::new("systemctl")
            .args(["--user", "enable", LOGIN_SERVICE])
            .output()
            .map_err(|e| format!("systemctl enable login: {e}"))?;
        if !out_login.status.success() {
            return Err(format!(
                "systemctl enable {LOGIN_SERVICE}: {}",
                String::from_utf8_lossy(&out_login.stderr)
            ));
        }
        Ok(format!(
            "enabled {TIMER} + {LOGIN_SERVICE} (daily {HOUR:02}:{MINUTE:02} {TZ_LABEL}, catch-up on login / missed run)"
        ))
    }

    pub fn disable(dry_run: bool) -> Result<String, String> {
        if dry_run {
            return Ok(format!(
                "would disable/remove {TIMER} + {SERVICE} + {LOGIN_SERVICE}"
            ));
        }
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", TIMER])
            .output();
        let _ = Command::new("systemctl")
            .args(["--user", "disable", LOGIN_SERVICE])
            .output();
        let _ = std::fs::remove_file(timer_path());
        let _ = std::fs::remove_file(service_path());
        let _ = std::fs::remove_file(login_service_path());
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();
        Ok(format!("disabled {TIMER} + {LOGIN_SERVICE}"))
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
                format!(
                    "launchd ({LABEL}: loaded, daily {HOUR:02}:{MINUTE:02} local + RunAtLoad ≈ {TZ_LABEL})"
                )
            }
            _ => format!("launchd ({LABEL}: plist present, not loaded)"),
        }
    }

    pub fn enable(bin: &std::path::Path, dry_run: bool) -> Result<String, String> {
        let path = plist_path();
        // RunAtLoad: catch-up when the agent loads (login). Due-gate skips if
        // already shared today. Calendar interval: preferred evening sample.
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
  <true/>
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
            "enabled {LABEL} (daily {HOUR:02}:{MINUTE:02} local + login; set TZ={TZ_LABEL} for Spain)"
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
                    "windows-task ({TASK_NAME}: {st}, daily {HOUR:02}:{MINUTE:02} local + logon catch-up ≈ {TZ_LABEL})"
                )
            }
            _ => format!("windows-task ({TASK_NAME}: missing)"),
        }
    }

    /// Task Scheduler XML: daily evening + logon, StartWhenAvailable for missed runs.
    fn task_xml(bin: &std::path::Path) -> String {
        let cmd = bin.display().to_string();
        let cmd = cmd
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        let st = format!("{HOUR:02}:{MINUTE:02}:00");
        format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>spanreed anonymous daily share to grokinsider.net (evening + login catch-up)</Description>
  </RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>2020-01-01T{st}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByDay>
        <DaysInterval>1</DaysInterval>
      </ScheduleByDay>
    </CalendarTrigger>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>true</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT10M</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{cmd}</Command>
      <Arguments>share</Arguments>
    </Exec>
  </Actions>
</Task>
"#
        )
    }

    pub fn enable(bin: &std::path::Path, dry_run: bool) -> Result<String, String> {
        let st = format!("{HOUR:02}:{MINUTE:02}");
        if dry_run {
            return Ok(format!(
                "would register schtasks {TASK_NAME} DAILY {st} + logon, StartWhenAvailable → share"
            ));
        }
        match try_schtasks_xml(bin) {
            Ok(()) => Ok(format!(
                "enabled {TASK_NAME} (daily {st} local + logon, StartWhenAvailable; use Windows TZ Spain ≈ {TZ_LABEL})"
            )),
            Err(xml_err) => {
                // Fallback: classic daily (no catch-up) so setup still succeeds.
                try_schtasks_daily_tr(bin).map_err(|e| {
                    format!("schtasks XML failed ({xml_err}); TR fallback: {e}")
                })?;
                Ok(format!(
                    "enabled {TASK_NAME} (daily {st} local TR fallback; re-run setup if catch-up needed)"
                ))
            }
        }
    }

    fn try_schtasks_xml(bin: &std::path::Path) -> Result<(), String> {
        let _ = Command::new("schtasks")
            .args(["/Delete", "/TN", TASK_NAME, "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        let xml = task_xml(bin);
        let dir = std::env::temp_dir();
        let path = dir.join(format!("spanreed-share-{}.xml", std::process::id()));
        let mut utf16: Vec<u8> = vec![0xFF, 0xFE];
        for u in xml.encode_utf16() {
            utf16.extend_from_slice(&u.to_le_bytes());
        }
        std::fs::write(&path, &utf16).map_err(|e| format!("write task xml: {e}"))?;

        let path_s = path.display().to_string();
        let out = Command::new("schtasks")
            .args(["/Create", "/TN", TASK_NAME, "/XML", &path_s, "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("schtasks /Create /XML: {e}"))?;
        let _ = std::fs::remove_file(&path);
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }

    fn try_schtasks_daily_tr(bin: &std::path::Path) -> Result<(), String> {
        let tr = format!("\"{}\" share", bin.display());
        let st = format!("{HOUR:02}:{MINUTE:02}");
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
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        Ok(())
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

    #[cfg(test)]
    mod tests {
        use super::task_xml;
        use std::path::Path;

        #[test]
        fn task_xml_has_catch_up_and_share() {
            let xml = task_xml(Path::new(r"C:\Users\me\bin\spanreed.exe"));
            assert!(xml.contains("StartWhenAvailable>true"));
            assert!(xml.contains("LogonTrigger"));
            assert!(xml.contains("CalendarTrigger"));
            assert!(xml.contains("<Arguments>share</Arguments>"));
            assert!(xml.contains("RunOnlyIfNetworkAvailable>true"));
        }

        #[test]
        fn task_xml_escapes_ampersand() {
            let xml = task_xml(Path::new(r"C:\a&b\spanreed.exe"));
            assert!(xml.contains("a&amp;b"));
        }
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
