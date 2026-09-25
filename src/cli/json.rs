//! `spanreed json`: raw JSON of detected provider outputs.

use std::process::ExitCode;

use crate::app::{self, AppContext};

pub(super) fn run(ctx: &AppContext, _args: &[String]) -> ExitCode {
    let outputs = app::usage::detected(ctx);
    match serde_json::to_string_pretty(&outputs) {
        Ok(s) => {
            println!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("serialize error: {e}");
            ExitCode::FAILURE
        }
    }
}
