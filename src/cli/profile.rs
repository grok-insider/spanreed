//! `spanreed profile …` status-bar fragments and `spanreed widget json|panel`.

use std::path::Path;
use std::process::ExitCode;

use crate::app::{self, AppContext};

pub(super) fn run(_ctx: &AppContext, args: &[String]) -> ExitCode {
    let result = match args.first().map(String::as_str) {
        None | Some("list") => {
            println!("waybar\neww\neww-panel\nsketchybar");
            return ExitCode::SUCCESS;
        }
        Some("show") if args.len() == 2 => app::profiles::files(&args[1])
            .ok_or_else(|| "Unknown profile".into())
            .map(|files| {
                for (name, body) in files {
                    println!("--- {name} ---\n{body}");
                }
            }),
        Some("install") if args.len() == 4 && args[2] == "--output" => {
            app::profiles::install(&args[1], Path::new(&args[3]))
        }
        _ => Err("Usage: spanreed profile list | show NAME | install NAME --output DIR".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

pub(super) fn widget(ctx: &AppContext, args: &[String]) -> ExitCode {
    if args.first().is_some_and(|arg| arg == "panel") {
        println!("{}", app::profiles::panel());
        return ExitCode::SUCCESS;
    }
    if args.first().is_some_and(|arg| arg != "json") {
        eprintln!("Usage: spanreed widget json | panel");
        return ExitCode::FAILURE;
    }
    let mut outputs = app::usage::snapshot(ctx, false);
    for output in &mut outputs {
        for line in &mut output.lines {
            if let app::usage::model::MetricLine::Progress {
                label,
                used,
                limit,
                format,
                ..
            } = line
            {
                let value = match format {
                    app::usage::model::ProgressFormat::Percent => format!("{used:.0}%"),
                    app::usage::model::ProgressFormat::Dollars => {
                        format!("${used:.2} / ${limit:.2}")
                    }
                    app::usage::model::ProgressFormat::Count { suffix } => {
                        format!("{used:.0} / {limit:.0} {suffix}")
                    }
                };
                *line = app::usage::model::MetricLine::text(
                    app::usage::model::MetricKind::Quota,
                    label.clone(),
                    value,
                );
            }
        }
    }
    match serde_json::to_string(&outputs) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(_) => ExitCode::FAILURE,
    }
}
