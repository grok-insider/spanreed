//! `spanreed serve [--interval S]`: the local HTTP API.

use std::process::ExitCode;

use crate::app::{self, AppContext};

pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    let interval = super::args::value(args, "--interval")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300);

    match app::local_api::serve(ctx, interval) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("server error: {e}");
            ExitCode::FAILURE
        }
    }
}
