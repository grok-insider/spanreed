//! The capture fabric on 127.0.0.1:18736 (service, health and log).

pub use crate::capture::{DEFAULT_GROK_CLI_BIND, DEFAULT_XAI_API_BIND, Options};

/// Start capture (with its watchdog) when the ports are down.
pub fn ensure(dry_run: bool) -> Result<String, String> {
    crate::setup::service_ensure(dry_run)
}

pub fn is_up() -> bool {
    crate::setup::capture_ports_up()
}

pub fn log_path() -> std::path::PathBuf {
    crate::capture_log::capture_log_path()
}

pub fn log(message: &str) {
    crate::capture_log::append(message);
}

/// Serve capture in this process, or keep a worker alive with `watchdog`.
pub fn serve(options: &Options, serve_args: &[String]) -> Result<(), String> {
    if options.watchdog {
        return crate::capture_watchdog::run(serve_args).inspect_err(|error| {
            log(&format!("watchdog fatal: {error}"));
        });
    }
    log(&format!("capture serve integrated bind={}", options.bind));
    options
        .serve()
        .inspect_err(|error| log(&format!("capture error: {error}")))
}
