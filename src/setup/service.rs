//! User-level capture service (systemd / launchd / Windows Scheduled Task).

use std::path::PathBuf;
use std::process::Command;

use super::paths;

pub fn kind_label() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "systemd-user"
    }
    #[cfg(target_os = "macos")]
    {
        "launchd"
    }
    #[cfg(target_os = "windows")]
    {
        "windows-hkcu-run"
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        "unsupported"
    }
}

pub fn status() -> String {
    platform::status()
}

pub fn enable(dry_run: bool) -> Result<String, String> {
    let bin = resolve_bin_for_service()?;
    platform::enable(&bin, dry_run)
}

pub fn disable(dry_run: bool) -> Result<String, String> {
    platform::disable(dry_run)
}

/// True when both default capture listeners accept TCP connections.
pub fn ports_up() -> bool {
    ports_listening_default()
}

/// Start capture if ports are down. Does not re-register autostart.
pub fn ensure(dry_run: bool) -> Result<String, String> {
    if ports_up() {
        return Ok(format!(
            "already listening on {} and {}",
            crate::grok_proxy::DEFAULT_GROK_CLI_BIND,
            crate::grok_proxy::DEFAULT_XAI_API_BIND
        ));
    }
    if dry_run {
        return Ok("would start `spanreed capture serve`".into());
    }
    let bin = resolve_bin_for_service()?;
    platform::start_now(&bin)?;
    for _ in 0..15 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if ports_up() {
            return Ok(format!(
                "started capture ({})",
                bin.display()
            ));
        }
    }
    Err(format!(
        "started capture but ports still down — check `{} capture serve` logs; fix: spanreed capture ensure",
        bin.display()
    ))
}

/// TCP connect check used by status / ensure / probe messaging.
pub fn ports_listening_default() -> bool {
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;
    let addrs = [
        crate::grok_proxy::DEFAULT_GROK_CLI_BIND,
        crate::grok_proxy::DEFAULT_XAI_API_BIND,
    ];
    addrs.iter().all(|a| {
        a.parse::<SocketAddr>()
            .ok()
            .and_then(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(200)).ok())
            .is_some()
    })
}

fn resolve_bin_for_service() -> Result<PathBuf, String> {
    let installed = paths::install_bin_path();
    if installed.exists() {
        return Ok(installed);
    }
    std::env::current_exe().map_err(|e| format!("current_exe: {e}"))
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::io::Write;

    const UNIT_NAME: &str = "spanreed-capture.service";

    fn unit_path() -> PathBuf {
        crate::creds::expand(&format!(
            "~/.config/systemd/user/{UNIT_NAME}"
        ))
    }

    pub fn status() -> String {
        let out = Command::new("systemctl")
            .args(["--user", "is-active", UNIT_NAME])
            .output();
        match out {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() {
                    format!("systemd-user ({UNIT_NAME}: unknown)")
                } else {
                    format!("systemd-user ({UNIT_NAME}: {s})")
                }
            }
            Err(_) => format!("systemd-user (systemctl missing; unit {})", unit_path().display()),
        }
    }

    pub fn enable(bin: &std::path::Path, dry_run: bool) -> Result<String, String> {
        let unit = unit_path();
        let body = format!(
            "[Unit]\n\
             Description=spanreed Grok/xAI usage capture proxy\n\
             After=network-online.target\n\
             Wants=network-online.target\n\
             \n\
             [Service]\n\
             ExecStart={bin} capture serve --watchdog\n\
             Restart=on-failure\n\
             RestartSec=3\n\
             \n\
             [Install]\n\
             WantedBy=default.target\n",
            bin = bin.display()
        );
        if dry_run {
            return Ok(format!("would install {}", unit.display()));
        }
        if let Some(parent) = unit.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir systemd user: {e}"))?;
        }
        {
            let mut f = std::fs::File::create(&unit).map_err(|e| format!("write unit: {e}"))?;
            f.write_all(body.as_bytes())
                .map_err(|e| format!("write unit: {e}"))?;
        }
        run(&["--user", "daemon-reload"])?;
        run(&["--user", "enable", "--now", UNIT_NAME])?;
        Ok(format!("enabled {UNIT_NAME}"))
    }

    pub fn start_now(bin: &std::path::Path) -> Result<(), String> {
        // Prefer systemd if unit exists; else detach a process.
        let unit = unit_path();
        if unit.exists() {
            let _ = Command::new("systemctl")
                .args(["--user", "start", UNIT_NAME])
                .output();
            if super::ports_up() {
                return Ok(());
            }
        }
        Command::new(bin)
            .args(["capture", "serve", "--watchdog"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn capture: {e}"))?;
        Ok(())
    }

    pub fn disable(dry_run: bool) -> Result<String, String> {
        if dry_run {
            return Ok(format!("would disable {UNIT_NAME}"));
        }
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", UNIT_NAME])
            .output();
        let unit = unit_path();
        if unit.exists() {
            let _ = std::fs::remove_file(&unit);
            let _ = Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .output();
        }
        Ok(format!("disabled {UNIT_NAME}"))
    }

    fn run(args: &[&str]) -> Result<(), String> {
        let out = Command::new("systemctl")
            .args(args)
            .output()
            .map_err(|e| format!("systemctl: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "systemctl {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    const LABEL: &str = "net.spanreed.capture";

    fn plist_path() -> PathBuf {
        crate::creds::expand(&format!(
            "~/Library/LaunchAgents/{LABEL}.plist"
        ))
    }

    pub fn status() -> String {
        let out = Command::new("launchctl")
            .args(["print", &format!("gui/{}/{}", uid(), LABEL)])
            .output();
        match out {
            Ok(o) if o.status.success() => format!("launchd ({LABEL}: loaded)"),
            _ => {
                if plist_path().exists() {
                    format!("launchd ({LABEL}: plist present, not loaded?)")
                } else {
                    format!("launchd ({LABEL}: not installed)")
                }
            }
        }
    }

    fn uid() -> u32 {
        Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
            .unwrap_or(501)
    }

    pub fn enable(bin: &std::path::Path, dry_run: bool) -> Result<String, String> {
        let plist = plist_path();
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
    <string>capture</string>
    <string>serve</string>
    <string>--watchdog</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{log}/spanreed-capture.log</string>
  <key>StandardErrorPath</key>
  <string>{log}/spanreed-capture.err</string>
</dict>
</plist>
"#,
            bin = bin.display(),
            log = crate::creds::expand("~/Library/Logs").display(),
        );
        if dry_run {
            return Ok(format!("would install {}", plist.display()));
        }
        if let Some(parent) = plist.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir LaunchAgents: {e}"))?;
        }
        let _ = std::fs::create_dir_all(crate::creds::expand("~/Library/Logs"));
        std::fs::write(&plist, body).map_err(|e| format!("write plist: {e}"))?;
        // Unload if present, then load.
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("gui/{}", uid()), plist.to_str().unwrap_or("")])
            .output();
        let out = Command::new("launchctl")
            .args(["bootstrap", &format!("gui/{}", uid()), plist.to_str().unwrap_or("")])
            .output()
            .map_err(|e| format!("launchctl bootstrap: {e}"))?;
        if !out.status.success() {
            // Fallback older API
            let _ = Command::new("launchctl")
                .args(["load", "-w", plist.to_str().unwrap_or("")])
                .output();
        }
        Ok(format!("enabled {LABEL}"))
    }

    pub fn start_now(bin: &std::path::Path) -> Result<(), String> {
        let plist = plist_path();
        if plist.exists() {
            let _ = Command::new("launchctl")
                .args([
                    "kickstart",
                    "-k",
                    &format!("gui/{}/{}", uid(), LABEL),
                ])
                .output();
            if super::ports_up() {
                return Ok(());
            }
        }
        Command::new(bin)
            .args(["capture", "serve", "--watchdog"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn capture: {e}"))?;
        Ok(())
    }

    pub fn disable(dry_run: bool) -> Result<String, String> {
        let plist = plist_path();
        if dry_run {
            return Ok(format!("would disable {LABEL}"));
        }
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("gui/{}", uid()), plist.to_str().unwrap_or("")])
            .output();
        let _ = Command::new("launchctl")
            .args(["unload", "-w", plist.to_str().unwrap_or("")])
            .output();
        if plist.exists() {
            let _ = std::fs::remove_file(&plist);
        }
        Ok(format!("disabled {LABEL}"))
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::os::windows::process::CommandExt;

    const TASK_NAME: &str = "OpenUsageCapture";
    const RUN_VALUE: &str = "OpenUsageCapture";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn run_command_value(bin: &std::path::Path) -> String {
        // Watchdog restarts the worker if it exits mid-session.
        format!("\"{}\" capture serve --watchdog", bin.display())
    }

    pub fn status() -> String {
        let run = read_run_key();
        let listening = ports_listening();
        let sch = schtasks_status();
        match (run, listening, sch.as_str()) {
            (true, true, _) => format!("hkcu-run ({RUN_VALUE}: installed, ports up)"),
            (true, false, _) => format!("hkcu-run ({RUN_VALUE}: installed, not listening)"),
            (false, true, _) => "capture ports up (manual process?)".into(),
            (false, false, s) if s != "missing" => format!("windows-task ({TASK_NAME}: {s})"),
            _ => format!("not installed (no HKCU Run / task {TASK_NAME})"),
        }
    }

    fn schtasks_status() -> String {
        let out = Command::new("schtasks")
            .args(["/Query", "/TN", TASK_NAME, "/FO", "LIST"])
            .output();
        match out {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                text.lines()
                    .find(|l| l.starts_with("Status:"))
                    .map(|l| l.trim().to_string())
                    .unwrap_or_else(|| "present".into())
            }
            _ => "missing".into(),
        }
    }

    fn ports_listening() -> bool {
        super::ports_listening_default()
    }

    fn read_run_key() -> bool {
        let script = format!(
            "$ErrorActionPreference='Stop'; \
             $p='HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run'; \
             if (Get-ItemProperty -Path $p -Name '{RUN_VALUE}' -ErrorAction SilentlyContinue) {{ '1' }} else {{ '0' }}"
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .ok();
        out.map(|o| String::from_utf8_lossy(&o.stdout).contains('1'))
            .unwrap_or(false)
    }

    fn set_run_key(bin: &std::path::Path) -> Result<(), String> {
        let val = run_command_value(bin).replace('\'', "''");
        let script = format!(
            "$ErrorActionPreference='Stop'; \
             Set-ItemProperty -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' \
               -Name '{RUN_VALUE}' -Value '{val}'"
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .map_err(|e| format!("powershell Run key: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "HKCU Run set failed: {}",
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
            .output();
        Ok(())
    }

    pub fn start_now(bin: &std::path::Path) -> Result<(), String> {
        // PowerShell Start-Process detaches cleanly (no console, survives parent exit).
        // --watchdog keeps capture alive if the worker process exits.
        let path = bin.display().to_string().replace('\'', "''");
        let script = format!(
            "Start-Process -FilePath '{path}' -ArgumentList 'capture','serve','--watchdog' -WindowStyle Hidden"
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Start-Process capture: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "Start-Process capture failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    }

    pub fn enable(bin: &std::path::Path, dry_run: bool) -> Result<String, String> {
        let tr = run_command_value(bin);
        if dry_run {
            return Ok(format!("would register HKCU Run {RUN_VALUE} → {tr}"));
        }
        // Prefer HKCU Run (no admin). schtasks ONLOGON often returns Access denied
        // for non-elevated users on locked-down Windows 11.
        set_run_key(bin)?;
        // Best-effort schtasks (ignore failure).
        let _ = try_schtasks(bin);
        start_now(bin)?;
        // Brief settle so status can see listeners.
        std::thread::sleep(std::time::Duration::from_millis(400));
        Ok(format!("enabled HKCU Run {RUN_VALUE} + started capture"))
    }

    fn try_schtasks(bin: &std::path::Path) -> Result<(), String> {
        let tr = run_command_value(bin);
        let _ = Command::new("schtasks")
            .args(["/Delete", "/TN", TASK_NAME, "/F"])
            .output();
        let out = Command::new("schtasks")
            .args([
                "/Create",
                "/TN",
                TASK_NAME,
                "/SC",
                "ONLOGON",
                "/RL",
                "LIMITED",
                "/F",
                "/TR",
                &tr,
            ])
            .output()
            .map_err(|e| format!("schtasks: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
        let _ = Command::new("schtasks")
            .args(["/Run", "/TN", TASK_NAME])
            .output();
        Ok(())
    }

    pub fn disable(dry_run: bool) -> Result<String, String> {
        if dry_run {
            return Ok(format!("would remove HKCU Run {RUN_VALUE} / task {TASK_NAME}"));
        }
        clear_run_key()?;
        let _ = Command::new("schtasks")
            .args(["/Delete", "/TN", TASK_NAME, "/F"])
            .output();
        // Stop listening processes that look like our capture (best-effort).
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-CimInstance Win32_Process -Filter \"Name='spanreed.exe'\" | \
                 Where-Object { $_.CommandLine -match 'capture' } | \
                 ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }",
            ])
            .output();
        Ok(format!("disabled capture autostart ({RUN_VALUE})"))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use super::*;

    pub fn status() -> String {
        "unsupported OS".into()
    }
    pub fn enable(_bin: &std::path::Path, _dry_run: bool) -> Result<String, String> {
        Err("capture user service not supported on this OS".into())
    }
    pub fn start_now(bin: &std::path::Path) -> Result<(), String> {
        Command::new(bin)
            .args(["capture", "serve", "--watchdog"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn capture: {e}"))?;
        Ok(())
    }
    pub fn disable(_dry_run: bool) -> Result<String, String> {
        Ok("nothing to disable".into())
    }
}
