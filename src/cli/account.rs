//! `spanreed account …`: identities the host owns.

use std::process::ExitCode;

use crate::app::{self, AppContext};

pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    match app::accounts::command(ctx, args) {
        Ok(out) => {
            if !out.is_empty() {
                print!("{out}");
                if !out.ends_with('\n') {
                    println!();
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
