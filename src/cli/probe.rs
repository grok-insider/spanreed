//! `spanreed probe [id] [--force] [--cost …]`.

use std::process::ExitCode;

use crate::app::{self, AppContext};

fn parse_probe_view(args: &[String]) -> app::usage::ProbeView {
    let all = args.iter().any(|a| a == "--all");
    app::usage::ProbeView {
        cost: all || args.iter().any(|a| a == "--cost"),
        models: all || args.iter().any(|a| a == "--models"),
        cache: all || args.iter().any(|a| a == "--cache"),
        trend: all || args.iter().any(|a| a == "--trend"),
        plan: all || args.iter().any(|a| a == "--plan"),
        all,
    }
}

pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    let force = args.iter().any(|a| a == "--force");
    let view = parse_probe_view(args);
    let id = args.iter().find(|a| !a.starts_with("--"));

    let Some(outputs) = app::usage::probe(ctx, id.map(String::as_str), force) else {
        eprintln!(
            "unknown provider: {}",
            id.map(String::as_str).unwrap_or_default()
        );
        return ExitCode::FAILURE;
    };

    if outputs.is_empty() {
        println!("No providers detected. Try `spanreed list` or `spanreed probe <id> --force`.");
        return ExitCode::SUCCESS;
    }

    let text = if view.all {
        app::usage::plain(&outputs)
    } else {
        app::usage::plain_with_view(&outputs, view)
    };
    println!("{text}");
    let any_err = outputs.iter().any(app::usage::ProviderOutput::has_error);
    if any_err {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
