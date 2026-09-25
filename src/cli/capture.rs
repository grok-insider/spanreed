//! `spanreed capture serve|ensure|status`.

use std::process::ExitCode;

use crate::app::{AppContext, capture};

pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("serve") => serve(ctx, &args[1..]),
        None => serve(ctx, args),
        Some("ensure") => {
            let dry = super::args::has(args, "--dry-run");
            super::report("capture ensure", capture::ensure(ctx, dry))
        }
        Some("status") => status(ctx),
        Some(other) => {
            eprintln!("unknown capture subcommand: {other}");
            eprintln!(
                "usage: spanreed capture serve [--watchdog] [--grok-cli-bind A] [--xai-api-bind B]\n\
                 \t spanreed capture ensure [--dry-run]\n\
                 \t spanreed capture status\n\
                 \t default fabric: {}\n\
                 \t --xai-api-bind: optional compat (e.g. {})",
                capture::DEFAULT_GROK_CLI_BIND,
                capture::DEFAULT_XAI_API_BIND
            );
            ExitCode::FAILURE
        }
    }
}

/// Optional overrides: --grok-cli-bind, --xai-api-bind, --watchdog.
fn serve(ctx: &AppContext, args: &[String]) -> ExitCode {
    let options = match capture::Options::parse(args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("capture: {error}");
            return ExitCode::FAILURE;
        }
    };
    let worker_args: Vec<String> = args
        .iter()
        .filter(|a| a.as_str() != "--watchdog")
        .cloned()
        .collect();
    match capture::serve(ctx, &options, &worker_args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let prefix = if options.watchdog {
                "capture watchdog"
            } else {
                "capture"
            };
            eprintln!("{prefix}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn status(ctx: &AppContext) -> ExitCode {
    let up = capture::is_up(ctx);
    println!(
        "capture: {}",
        if up {
            "listening (127.0.0.1:18736)"
        } else {
            "DOWN — run `spanreed capture ensure`"
        }
    );
    println!("log:     {}", capture::log_path(ctx).display());
    if up {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
