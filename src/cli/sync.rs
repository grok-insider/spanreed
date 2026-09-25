//! `spanreed sync [link-codex]`.

use std::process::ExitCode;

use crate::app::{AppContext, sync};

pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "spanreed sync — synchronize selected private usage sources with Fabrials.\nSelect sources in Spanreed Settings and explicitly enable private history synchronization.\nNo provider credentials or request bodies are uploaded."
        );
        return ExitCode::SUCCESS;
    }
    let result = if args.first().is_some_and(|arg| arg == "link-codex") {
        sync::link_codex(ctx)
    } else {
        sync::run(ctx)
    };
    super::report("", result)
}
