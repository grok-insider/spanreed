//! `spanreed waybar`: Waybar custom-module JSON.

use std::process::ExitCode;

use crate::app::{self, AppContext};

pub(super) fn run(ctx: &AppContext, _args: &[String]) -> ExitCode {
    // Prefer the running daemon's cached data (instant); fall back to probing.
    let outputs = app::usage::cached_or_probe(ctx);
    let json = app::usage::waybar(&outputs);
    println!("{json}");
    ExitCode::SUCCESS
}
