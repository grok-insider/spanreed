//! `spanreed self-update [--check] [--json] [--yes] [--dry-run]`.

use std::io::{self, Write};
use std::process::ExitCode;

use crate::app::{AppContext, updates};

pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    let mut check_only = false;
    let mut json = false;
    let mut yes = false;
    let mut dry_run = false;
    for a in args {
        match a.as_str() {
            "--check" => check_only = true,
            "--json" => json = true,
            "--yes" | "-y" => yes = true,
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("self-update: unknown flag: {other}");
                print_help();
                return ExitCode::FAILURE;
            }
        }
    }

    if updates::offline(ctx) {
        eprintln!("self-update: SPANREED_OFFLINE=1 — not checking GitHub");
        return ExitCode::FAILURE;
    }

    let result = match updates::check_for_update(ctx) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("self-update: {e}");
            return ExitCode::FAILURE;
        }
    };

    if json {
        let j = CheckJson {
            current: &result.current,
            latest: &result.latest,
            newer: result.newer,
            tag: &result.tag,
            asset_name: &result.asset_name,
            asset_url: &result.asset_url,
            sha_url: &result.sha_url,
        };
        println!("{}", serde_json::to_string_pretty(&j).unwrap_or_default());
    } else if result.newer {
        println!(
            "update available: {} → {} ({})",
            result.current, result.latest, result.tag
        );
        println!("asset: {}", result.asset_name);
    } else {
        println!("up to date: {} ({})", result.current, result.tag);
    }

    if check_only {
        return if result.newer {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }

    if !result.newer {
        return ExitCode::SUCCESS;
    }

    if !dry_run && !updates::can_apply_self_update(ctx) {
        let why = updates::apply_blocked_reason(ctx).unwrap_or("self-update apply is disabled");
        eprintln!("self-update: {why}");
        return ExitCode::FAILURE;
    }

    if !yes && !dry_run && !confirm_apply(&result) {
        eprintln!("self-update: cancelled");
        return ExitCode::FAILURE;
    }

    match updates::apply_update(ctx, &result, dry_run) {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("self-update: {e}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct CheckJson<'a> {
    current: &'a str,
    latest: &'a str,
    newer: bool,
    tag: &'a str,
    asset_name: &'a str,
    asset_url: &'a str,
    sha_url: &'a str,
}

fn print_help() {
    println!(
        "spanreed self-update — check or install the latest GitHub Release\n\n\
         \t--check       Only report; exit 0 if current, 2 if newer, 1 on error\n\
         \t--json        Machine-readable check result\n\
         \t--yes / -y    Apply without interactive confirmation\n\
         \t--dry-run     Download + verify checksum; do not replace the binary\n\
         \nEnv: SPANREED_REPO (default {}), SPANREED_OFFLINE=1",
        updates::DEFAULT_REPO
    );
}

fn confirm_apply(r: &updates::CheckResult) -> bool {
    eprint!("Install spanreed {} → {} now? [y/N] ", r.current, r.latest);
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}
