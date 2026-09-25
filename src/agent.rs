//! `spanreed agent …`: the local host for desktop.grok.me (formerly the
//! standalone `grok-bridge`). It supervises the user's Grok Build CLI and
//! serves the browser contract on its own loopback port, separate from the
//! capture relay.

use std::process::ExitCode;

use fabrials_agent_host::{CliDefaults, HostIdentity};

/// How `/healthz` and the CLI name this host.
pub const HOST: HostIdentity = HostIdentity::new("spanreed", env!("CARGO_PKG_VERSION"));

/// The command users type to reach the agent host.
pub const COMMAND: &str = "spanreed agent";

/// Program names that run straight into `spanreed agent` (installed as links
/// so existing `grok-bridge …` scripts and autostart entries keep working).
const LEGACY_PROGRAMS: &[&str] = &["grok-bridge", "grok-bridge.exe"];

pub fn defaults() -> CliDefaults {
    CliDefaults::new(HOST, COMMAND)
}

pub fn cmd(args: &[String]) -> ExitCode {
    if args.first().map(String::as_str) == Some("service") {
        return service::cmd(&args[1..]);
    }
    fabrials_agent_host::cli::run(args, defaults())
}

/// Keep `spanreed agent serve` running at login (macOS LaunchAgent).
pub mod service {
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;

    pub const LABEL: &str = "com.fabrials.spanreed.agent";

    /// The LaunchAgent definition for `program agent serve`, logging under `logs`.
    pub fn launch_agent_plist(program: &Path, logs: &Path) -> String {
        let escape = |text: &str| {
            text.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        };
        let program = escape(&program.display().to_string());
        let log = escape(&logs.join("agent.log").display().to_string());
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{program}</string>
    <string>agent</string>
    <string>serve</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>ThrottleInterval</key>
  <integer>10</integer>
  <key>ProcessType</key>
  <string>Interactive</string>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#
        )
    }

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    pub fn plist_path(home: &Path) -> PathBuf {
        home.join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist"))
    }

    pub fn logs_dir(home: &Path) -> PathBuf {
        home.join("Library/Logs/spanreed")
    }

    const USAGE: &str = "usage: spanreed agent service print|install|uninstall";

    pub fn cmd(args: &[String]) -> ExitCode {
        let Some(home) = home() else {
            eprintln!("spanreed agent service: HOME is not set");
            return ExitCode::FAILURE;
        };
        let program = match std::env::current_exe() {
            Ok(program) => program,
            Err(error) => {
                eprintln!("spanreed agent service: {error}");
                return ExitCode::FAILURE;
            }
        };
        let result = match args.first().map(String::as_str) {
            Some("print") => {
                print!("{}", launch_agent_plist(&program, &logs_dir(&home)));
                Ok(())
            }
            Some("install") => install(&home, &program),
            Some("uninstall") => uninstall(&home),
            _ => Err(USAGE.to_string()),
        };
        match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("spanreed agent service: {error}");
                ExitCode::FAILURE
            }
        }
    }

    #[cfg(unix)]
    fn launchctl(args: &[&str]) -> Result<(), String> {
        let output = std::process::Command::new("launchctl")
            .args(args)
            .output()
            .map_err(|error| format!("launchctl: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "launchctl {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    #[cfg(unix)]
    fn domain() -> String {
        format!("gui/{}", current_uid())
    }

    #[cfg(unix)]
    fn current_uid() -> u32 {
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|uid| uid.trim().parse().ok())
            .unwrap_or(501)
    }

    fn install(home: &Path, program: &Path) -> Result<(), String> {
        if cfg!(target_os = "macos") {
            install_launch_agent(home, program)
        } else {
            Err(NOT_MACOS.into())
        }
    }

    fn uninstall(home: &Path) -> Result<(), String> {
        if cfg!(target_os = "macos") {
            uninstall_launch_agent(home)
        } else {
            Err(NOT_MACOS.into())
        }
    }

    const NOT_MACOS: &str =
        "the login service is a macOS LaunchAgent; use `spanreed setup` elsewhere";

    #[cfg(unix)]
    fn install_launch_agent(home: &Path, program: &Path) -> Result<(), String> {
        let path = plist_path(home);
        let logs = logs_dir(home);
        std::fs::create_dir_all(&logs).map_err(|error| format!("{}: {error}", logs.display()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        std::fs::write(&path, launch_agent_plist(program, &logs))
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let _ = launchctl(&["bootout", &format!("{}/{LABEL}", domain())]);
        launchctl(&["bootstrap", &domain(), &path.display().to_string()])?;
        println!(
            "installed {} (runs `spanreed agent serve` at login)",
            path.display()
        );
        Ok(())
    }

    #[cfg(unix)]
    fn uninstall_launch_agent(home: &Path) -> Result<(), String> {
        let path = plist_path(home);
        let _ = launchctl(&["bootout", &format!("{}/{LABEL}", domain())]);
        match std::fs::remove_file(&path) {
            Ok(()) => println!("removed {}", path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn install_launch_agent(_home: &Path, _program: &Path) -> Result<(), String> {
        Err(NOT_MACOS.into())
    }

    #[cfg(not(unix))]
    fn uninstall_launch_agent(_home: &Path) -> Result<(), String> {
        Err(NOT_MACOS.into())
    }
}

/// `argv[0]` names the old `grok-bridge` binary.
pub fn invoked_as_legacy_bridge(argv0: &str) -> bool {
    std::path::Path::new(argv0)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| LEGACY_PROGRAMS.contains(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_reports_spanreed_and_its_version() {
        let defaults = defaults();
        assert_eq!(defaults.identity.name, "spanreed");
        assert_eq!(defaults.identity.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(defaults.command, "spanreed agent");
        assert!(
            defaults
                .allowed_origins
                .iter()
                .any(|origin| origin == "https://desktop.grok.me")
        );
    }

    #[test]
    fn the_launch_agent_keeps_the_host_serving() {
        let plist = service::launch_agent_plist(
            std::path::Path::new("/Applications/Spanreed & Co/spanreed"),
            std::path::Path::new("/Users/me/Library/Logs/spanreed"),
        );
        assert!(plist.contains("<string>com.fabrials.spanreed.agent</string>"));
        assert!(plist.contains(
            "<string>/Applications/Spanreed &amp; Co/spanreed</string>\n    <string>agent</string>\n    <string>serve</string>"
        ));
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(plist.contains("<key>SuccessfulExit</key>\n    <false/>"));
        assert!(plist.contains("<string>/Users/me/Library/Logs/spanreed/agent.log</string>"));
        assert_eq!(
            service::plist_path(std::path::Path::new("/Users/me")),
            std::path::Path::new(
                "/Users/me/Library/LaunchAgents/com.fabrials.spanreed.agent.plist"
            )
        );
    }

    #[test]
    fn the_legacy_program_name_is_recognised() {
        assert!(invoked_as_legacy_bridge(
            "/home/user/.local/bin/grok-bridge"
        ));
        assert!(invoked_as_legacy_bridge("grok-bridge.exe"));
        assert!(!invoked_as_legacy_bridge("/usr/bin/spanreed"));
        assert!(!invoked_as_legacy_bridge("grok-bridge-old"));
    }
}
