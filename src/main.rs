//! spanreed: Linux-native AI coding subscription usage tracker.
//!
//! Subcommands:
//!   spanreed list                 List known providers and detection state.
//!   spanreed probe [id] [--force] Probe all detected providers (or one).
//!   spanreed waybar               Emit Waybar custom-module JSON (one shot).
//!   spanreed json                 Emit raw JSON of all detected providers.
//!   spanreed serve [--interval S] Run the local HTTP API on 127.0.0.1:6736.
//!   spanreed update-pricing [out] Fetch + filter the upstream price table.

mod activity;
mod api;
mod cost;
mod creds;
mod grok_ledger;
mod grok_proxy;
mod http;
mod model;
mod output;
mod pricing;
mod probe;
mod proc;
mod providers;
mod secret;
mod util;

use std::process::ExitCode;

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

fn main() -> ExitCode {
    reset_sigpipe();
    // Capture the local timezone offset while single-threaded (used for daily
    // cost buckets); `time` can't read it reliably once threads exist.
    util::init_local_offset();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("probe");
    let rest = if args.is_empty() { &[][..] } else { &args[1..] };

    match cmd {
        "list" => cmd_list(),
        "probe" => cmd_probe(rest),
        "waybar" => cmd_waybar(),
        "json" => cmd_json(),
        "serve" => cmd_serve(rest),
        "grok-proxy" => cmd_grok_proxy(rest),
        "update-pricing" => cmd_update_pricing(rest),
        "help" | "-h" | "--help" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command: {other}\n");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "spanreed — Linux AI subscription usage tracker\n\n\
         USAGE:\n\
         \tspanreed list                 Show providers and whether they're detected\n\
         \tspanreed probe [id] [--force] Probe detected providers, or a single id\n\
         \tspanreed waybar               Waybar custom-module JSON (one shot)\n\
         \tspanreed json                 Raw JSON of detected provider outputs\n\
         \tspanreed serve [--interval S] Local HTTP API on 127.0.0.1:6736\n\
         \tspanreed grok-proxy [--bind HOST:PORT]\n\
         \t                               Capture official Grok API usage into a local\n\
         \t                               ledger (for accurate Last 30 Days on probe)\n\
         \tspanreed update-pricing [out] Fetch + filter the LiteLLM price table\n\
         \t                               (writes to stdout, or to [out]; used to\n\
         \t                               refresh the embedded src/pricing-data.json)\n\n\
         PROVIDERS: claude, codex, cursor, grok, opencode-go, amp, zai, minimax,\n\
         \t           synthetic, kimi, copilot, factory, devin,\n\
         \t           jetbrains-ai-assistant, kiro, antigravity, perplexity"
    );
}

fn cmd_list() -> ExitCode {
    for p in providers::all() {
        let detected = if p.detect() { "detected" } else { "—" };
        println!("{:<14} {:<12} {}", p.id(), detected, p.name());
    }
    ExitCode::SUCCESS
}

fn cmd_probe(args: &[String]) -> ExitCode {
    let force = args.iter().any(|a| a == "--force");
    let id = args.iter().find(|a| !a.starts_with("--"));

    let outputs = match id {
        Some(id) => match probe::probe_one(id) {
            Some(out) => vec![out],
            None => {
                eprintln!("unknown provider: {id}");
                return ExitCode::FAILURE;
            }
        },
        None => {
            if force {
                probe::probe_all()
            } else {
                probe::probe_detected()
            }
        }
    };

    if outputs.is_empty() {
        println!("No providers detected. Try `spanreed list` or `spanreed probe <id> --force`.");
        return ExitCode::SUCCESS;
    }

    println!("{}", output::plain(&outputs));
    let any_err = outputs.iter().any(model::ProviderOutput::has_error);
    if any_err {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_waybar() -> ExitCode {
    // Prefer the running daemon's cached data (instant); fall back to probing.
    let outputs = api::fetch_cached().unwrap_or_else(probe::probe_detected);
    let json = output::waybar(&outputs);
    println!("{json}");
    ExitCode::SUCCESS
}

fn cmd_json() -> ExitCode {
    let outputs = probe::probe_detected();
    match serde_json::to_string_pretty(&outputs) {
        Ok(s) => {
            println!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("serialize error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_update_pricing(args: &[String]) -> ExitCode {
    let json = match pricing::fetch_filtered() {
        Ok(j) => j,
        Err(e) => {
            eprintln!("update-pricing failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    match args.first() {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &json) {
                eprintln!("write {path} failed: {e}");
                return ExitCode::FAILURE;
            }
            eprintln!("wrote {path}");
        }
        None => println!("{json}"),
    }
    ExitCode::SUCCESS
}

fn cmd_serve(args: &[String]) -> ExitCode {
    let interval = args
        .iter()
        .position(|a| a == "--interval")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300);

    match api::serve(interval) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("server error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_grok_proxy(args: &[String]) -> ExitCode {
    let bind = args
        .iter()
        .position(|a| a == "--bind")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str);
    let upstream = args
        .iter()
        .position(|a| a == "--upstream")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str);
    match grok_proxy::run(bind, upstream) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("grok-proxy error: {e}");
            ExitCode::FAILURE
        }
    }
}
