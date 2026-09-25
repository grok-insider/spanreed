//! `spanreed auth copilot | auth logout copilot`.

use std::process::ExitCode;

use crate::app::{self, AppContext};

pub(super) fn run(_ctx: &AppContext, args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("copilot") => match app::accounts::link_copilot(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("auth copilot: {e}");
                ExitCode::FAILURE
            }
        },
        Some("logout") => match args.get(1).map(String::as_str) {
            Some("copilot") => match app::accounts::unlink_copilot() {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("auth logout copilot: {e}");
                    ExitCode::FAILURE
                }
            },
            other => {
                eprintln!(
                    "unknown auth logout target: {}\nusage: spanreed auth logout copilot",
                    other.unwrap_or("(none)")
                );
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!(
                "unknown auth target: {}\nusage:\n  spanreed auth copilot [--user LOGIN | --token-stdin]\n  spanreed auth logout copilot",
                other.unwrap_or("(none)")
            );
            ExitCode::FAILURE
        }
    }
}
