//! `spanreed list`: providers and whether they are detected.

use std::process::ExitCode;

use crate::app::{self, AppContext};

pub(super) fn run(_ctx: &AppContext, _args: &[String]) -> ExitCode {
    for row in app::usage::list() {
        println!("{:<14} {:<12} {}", row.id, row.state, row.name);
    }
    ExitCode::SUCCESS
}
