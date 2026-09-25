//! `spanreed addon|plugin list`: in-process, toml and PATH addons.

use std::process::ExitCode;

use crate::app::{self, AppContext};

pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        None | Some("list") => {
            for row in app::addons::list(ctx) {
                let on = if row.enabled { "on" } else { "off" };
                let cmds = if row.commands.is_empty() {
                    String::new()
                } else {
                    format!(" cmds={}", row.commands.join(","))
                };
                println!(
                    "{:<16} {:<8} {:<12} {}{cmds}",
                    row.id, on, row.source, row.name
                );
            }
            ExitCode::SUCCESS
        }
        Some("enable") | Some("disable") => {
            eprintln!("addon enable/disable: edit ~/.config/spanreed/addons/<id>.toml");
            ExitCode::FAILURE
        }
        Some(other) => {
            eprintln!("unknown addon subcommand: {other}\nusage: spanreed addon list");
            ExitCode::FAILURE
        }
    }
}
