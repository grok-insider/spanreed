//! `spanreed privacy [metrics|sync on|off]`.

use std::process::ExitCode;

use crate::app::{AppContext, sharing};

pub(super) fn run(_ctx: &AppContext, args: &[String]) -> ExitCode {
    let mut consent = sharing::consent();
    match args {
        [] => {
            println!(
                "{}",
                serde_json::to_string_pretty(&consent).unwrap_or_default()
            );
            return ExitCode::SUCCESS;
        }
        [kind, value]
            if matches!(kind.as_str(), "metrics" | "sync")
                && matches!(value.as_str(), "on" | "off") =>
        {
            if kind == "metrics" {
                consent.share_metrics = value == "on";
            } else {
                consent.sync_history = value == "on";
            }
        }
        _ => {
            eprintln!(
                "Usage: spanreed privacy [metrics|sync on|off]\nMetrics publishes aggregates; sync transfers private request history. Both are off by default."
            );
            return ExitCode::FAILURE;
        }
    }
    match sharing::save_consent(&consent) {
        Ok(()) => {
            println!("Sharing preference saved.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Could not save sharing preference: {error}");
            ExitCode::FAILURE
        }
    }
}
