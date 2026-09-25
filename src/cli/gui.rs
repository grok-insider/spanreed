//! `spanreed gui`: open the desktop window.

use std::process::ExitCode;

use crate::app::{self, AppContext};

pub(super) fn run(ctx: &AppContext, _args: &[String]) -> ExitCode {
    match app::window::open(ctx, "overview") {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
