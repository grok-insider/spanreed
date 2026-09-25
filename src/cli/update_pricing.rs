//! `spanreed update-pricing [out]`: fetch the upstream price table.

use std::process::ExitCode;

use crate::app::{self, AppContext};

pub(super) fn run(_ctx: &AppContext, args: &[String]) -> ExitCode {
    let json = match app::usage::fetch_price_table() {
        Ok(j) => j,
        Err(e) => {
            eprintln!("update-pricing failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    match args.first() {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &json) {
                eprintln!("write {path} failed: {e}");
                return ExitCode::FAILURE;
            }
            eprintln!("wrote {path}");
        }
        None => println!("{json}"),
    }
    ExitCode::SUCCESS
}
