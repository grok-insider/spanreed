//! `spanreed history [id]`: recorded rate-limit samples.

use std::process::ExitCode;

use crate::app::{self, AppContext};

pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    let provider = super::args::positional(args);
    let samples = match app::usage::samples(ctx, provider, 100_000) {
        Ok(samples) => samples,
        Err(error) => {
            eprintln!("history: {error}");
            return ExitCode::FAILURE;
        }
    };
    print!("{}", app::usage::format_history(ctx, &samples));
    ExitCode::SUCCESS
}
