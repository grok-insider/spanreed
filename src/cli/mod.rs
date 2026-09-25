//! Command-line surface: a small dispatcher and one module per subcommand.
//! Subcommands parse arguments and print; the work is done by `crate::app`.

mod account;
mod addon;
mod agent;
mod args;
mod auth;
mod capture;
mod grok_proxy;
mod gui;
mod help;
mod history;
mod json;
mod list;
mod privacy;
mod probe;
mod profile;
mod self_update;
mod serve;
mod setup;
mod share;
mod sync;
mod tray;
mod update_pricing;
mod usage;
mod waybar;

use std::process::ExitCode;

use crate::app::AppContext;

type Handler = fn(&AppContext, &[String]) -> ExitCode;

/// Every subcommand and alias, in help order.
const COMMANDS: &[(&str, Handler)] = &[
    ("list", list::run),
    ("probe", probe::run),
    ("waybar", waybar::run),
    ("json", json::run),
    ("serve", serve::run),
    ("history", history::run),
    ("capture", capture::run),
    ("agent", agent::run),
    ("grok-proxy", grok_proxy::run),
    ("setup", setup::run),
    ("auth", auth::run),
    ("update-pricing", update_pricing::run),
    ("share", share::run),
    ("usage", usage::run),
    ("sync", sync::run),
    ("privacy", privacy::run),
    ("profile", profile::run),
    ("widget", profile::widget),
    ("gui", gui::run),
    ("self-update", self_update::run),
    ("tray", tray::run),
    ("account", account::run),
    ("addon", addon::run),
    ("plugin", addon::run),
];

/// Restore the default SIGPIPE disposition (Unix only).
///
/// Rust ignores SIGPIPE at startup, which turns a closed downstream pipe
/// (e.g. `spanreed json | head`) into a write error that the `print!` macros
/// surface as a panic. Resetting it to `SIG_DFL` makes the process exit quietly
/// on a broken pipe, like a well-behaved Unix CLI. Non-Unix targets (Windows)
/// have no SIGPIPE, so this is a no-op there.
#[cfg(unix)]
fn reset_sigpipe() {
    // SAFETY: a single libc::signal call at startup, before any threads spawn.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

pub fn run_cli() -> ExitCode {
    reset_sigpipe();
    // Capture the local timezone offset while single-threaded (used for daily
    // cost buckets); `time` can't read it reliably once threads exist.
    crate::util::init_local_offset();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let argv0 = std::env::args().next().unwrap_or_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if agent::invoked_as_legacy_bridge(&argv0) {
        return agent::cmd(&args);
    }
    dispatch(&AppContext::new(), &args)
}

fn dispatch(ctx: &AppContext, args: &[String]) -> ExitCode {
    let cmd = args.first().map(String::as_str).unwrap_or("probe");
    let rest = args.get(1..).unwrap_or_default();
    if matches!(cmd, "help" | "-h" | "--help") {
        help::print();
        return ExitCode::SUCCESS;
    }
    if let Some((_, handler)) = COMMANDS.iter().find(|(name, _)| *name == cmd) {
        return handler(ctx, rest);
    }
    if let Some(code) = crate::app::addons::dispatch_prefix(cmd, rest) {
        return code;
    }
    eprintln!("unknown command: {cmd}\n");
    help::print();
    ExitCode::FAILURE
}

/// Print a command's result: `Ok` on stdout, `Err` on stderr with `prefix`.
fn report(prefix: &str, result: Result<String, String>) -> ExitCode {
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            if prefix.is_empty() {
                eprintln!("{error}");
            } else {
                eprintln!("{prefix}: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::COMMANDS;

    #[test]
    fn every_command_is_listed_once() {
        let mut names: Vec<_> = COMMANDS.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before);
        for name in [
            "probe",
            "capture",
            "agent",
            "grok-proxy",
            "tray",
            "plugin",
            "widget",
        ] {
            assert!(names.contains(&name), "{name}");
        }
    }
}
