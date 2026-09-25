//! `spanreed share [login|logout|status] [--force]`.

use std::process::ExitCode;

use crate::app::{AppContext, sharing};

pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "spanreed share — authenticated upload of plan/quota metrics\n\n\
             Requires a Grok Insider account (Sign in with X) linked once via:\n\
               spanreed share login\n\n\
             Sends aggregated provider+plan lines to the public community pool\n\
             on fabrials.com. Server identity is your account (not install id).\n\n\
             At most one local send per day (product TZ {tz}) unless --force.\n\
             Same-day re-send upserts on the server.\n\n\
             Subcommands: login | logout | status\n\
             Optional SPANREED_API_BASE (default {base}).\n\
             SPANREED_OFFLINE=1 skips the network call.\n\
             --force  bypass local same-day skip.",
            tz = sharing::TIME_ZONE_LABEL,
            base = sharing::DEFAULT_API_BASE,
        );
        return ExitCode::SUCCESS;
    }

    if let Some(sub) = args.first().map(String::as_str) {
        match sub {
            "login" => return login(),
            "logout" => return logout(),
            "status" => return status(),
            _ => {}
        }
    }

    let force = args.iter().any(|a| a == "--force");
    match sharing::share_now(ctx, force) {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) if e.starts_with("share: SPANREED_OFFLINE") => {
            eprintln!("{e}");
            ExitCode::SUCCESS
        }
        Err(e) if e.contains("already sent") => {
            eprintln!("{e}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Device authorization login (RFC 8628-style).
fn login() -> ExitCode {
    let pending = match sharing::begin_link() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("share login: {e}");
            return if e.contains("OFFLINE") {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
    };

    println!("spanreed share login\n");
    println!("  1. Open:  {}", pending.verification_uri);
    println!("  2. Code:  {}", pending.user_code);
    println!("  3. Sign in with X and approve this CLI\n");
    println!("Waiting for approval (up to {}s)…", pending.expires_in);

    match sharing::wait_link(&pending) {
        Ok(()) => {
            println!("Logged in. Daily share can run without the browser.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn logout() -> ExitCode {
    match sharing::unlink() {
        Ok(()) => {
            println!("share: logged out (local session removed)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("share logout: {e}");
            ExitCode::FAILURE
        }
    }
}

fn status() -> ExitCode {
    if !sharing::has_refresh_session() {
        println!("share: not logged in — run: spanreed share login");
        return ExitCode::FAILURE;
    }
    println!("share: logged in (refresh present)");
    if let Some(day) = sharing::last_shared_day() {
        println!("share: last shared day {day}");
    } else {
        println!("share: last shared day: never");
    }
    ExitCode::SUCCESS
}
