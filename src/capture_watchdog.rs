//! Supervisor that keeps `capture serve` running and restarts it on exit.
//!
//! ```text
//! spanreed capture serve --watchdog
//! ```
//!
//! The parent process does not bind ports. It loops: if listeners are down,
//! spawn `capture serve` (without `--watchdog`), redirect output to the capture
//! log, wait for exit, then restart after a short backoff.

use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::capture_log;
use crate::setup;

const BACKOFF_SECS: u64 = 2;
const HEALTHY_POLL_SECS: u64 = 15;

/// Run forever: ensure a capture worker is listening.
pub fn run(serve_args: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    capture_log::append(&format!(
        "watchdog start exe={} log={}",
        exe.display(),
        capture_log::capture_log_path().display()
    ));
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

        let mut child = cmd.spawn().map_err(|e| format!("spawn capture serve: {e}"))?;
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
                eprintln!("capture watchdog: worker exited ({status}), restarting in {BACKOFF_SECS}s");
            }
            Err(e) => {
                capture_log::append(&format!("worker wait error: {e}"));
                eprintln!("capture watchdog: wait error: {e}");
            }
        }
        thread::sleep(Duration::from_secs(BACKOFF_SECS));
    }
}
