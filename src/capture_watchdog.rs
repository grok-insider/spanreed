//! Supervisor that keeps `capture serve` running and restarts it on exit.
//!
//! ```text
//! spanreed capture serve --watchdog
//! ```
//!
//! The parent process does not bind ports. It loops: if listeners are down,
//! spawn `capture serve` (without `--watchdog`), redirect output to the capture
//! log, wait for exit, then restart after a short backoff.
//!
//! On Windows the watchdog detaches from any console so HKCU Run / Task
//! Scheduler logon starts do not leave a visible `cmd` window. Diagnostics go
//! only to the capture log file.

use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::capture_log;
use crate::setup;

const BACKOFF_SECS: u64 = 2;
const HEALTHY_POLL_SECS: u64 = 15;

/// CREATE_NO_WINDOW — spawn worker without a console window (Windows).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Detach from the parent console so login autostart is silent.
///
/// Console-subsystem binaries launched from HKCU Run always get a window;
/// FreeConsole releases it immediately. Safe to call when no console is
/// attached (returns false).
#[cfg(windows)]
fn detach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn FreeConsole() -> i32;
    }
    // Ignore failure (already detached / no console).
    unsafe {
        FreeConsole();
    }
}

/// Run forever: ensure a capture worker is listening.
pub fn run(serve_args: &[String]) -> Result<(), String> {
    #[cfg(windows)]
    detach_console();

    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    capture_log::append(&format!(
        "watchdog start exe={} log={}",
        exe.display(),
        capture_log::capture_log_path().display()
    ));
    // Prefer the log file so service mode never needs a TTY. When run
    // interactively in a real terminal (non-Windows, or attached console),
    // still print a one-line banner for discoverability.
    #[cfg(not(windows))]
    eprintln!(
        "spanreed capture watchdog (log: {})",
        capture_log::capture_log_path().display()
    );

    loop {
        if setup::capture_ports_up() {
            thread::sleep(Duration::from_secs(HEALTHY_POLL_SECS));
            continue;
        }

        capture_log::append("ports down - starting capture worker");
        #[cfg(not(windows))]
        eprintln!("capture watchdog: ports down, starting worker...");

        let log = capture_log::open_append()?;
        let log_err = log
            .try_clone()
            .map_err(|e| format!("clone log handle: {e}"))?;

        let mut cmd = Command::new(&exe);
        cmd.arg("capture").arg("serve");
        for a in serve_args {
            if a == "--watchdog" {
                continue;
            }
            cmd.arg(a);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn capture serve: {e}"))?;
        let pid = child.id();
        capture_log::append(&format!("worker pid={pid}"));

        // Give the worker a moment to bind before we consider it failed.
        thread::sleep(Duration::from_millis(500));
        if !setup::capture_ports_up() {
            // Still down — wait for process exit to see the error in the log.
            capture_log::append("worker started but ports not up yet; waiting");
        }

        match child.wait() {
            Ok(status) => {
                capture_log::append(&format!("worker pid={pid} exited: {status}"));
                #[cfg(not(windows))]
                eprintln!(
                    "capture watchdog: worker exited ({status}), restarting in {BACKOFF_SECS}s"
                );
            }
            Err(e) => {
                capture_log::append(&format!("worker wait error: {e}"));
                #[cfg(not(windows))]
                eprintln!("capture watchdog: wait error: {e}");
            }
        }
        thread::sleep(Duration::from_secs(BACKOFF_SECS));
    }
}
