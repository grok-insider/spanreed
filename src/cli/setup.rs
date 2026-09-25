//! `spanreed setup [status|uninstall]`: interactive install, capture service
//! and client wiring.

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use crate::app::AppContext;
use crate::app::setup::{self, Line, SetupOptions, SetupPlan, UninstallOptions};

#[derive(Debug, Default, Clone)]
struct SetupFlags {
    yes: bool,
    dry_run: bool,
    service: bool,
    no_wire: bool,
    from_current_exe: bool,
    purge_all: bool,
}

impl SetupFlags {
    fn parse(args: &[String]) -> (Option<String>, Self) {
        let mut flags = Self::default();
        let mut sub: Option<String> = None;
        for arg in args {
            match arg.as_str() {
                "--yes" | "-y" => flags.yes = true,
                "--dry-run" => flags.dry_run = true,
                "--service" => flags.service = true,
                "--no-wire" => flags.no_wire = true,
                "--from-current-exe" => flags.from_current_exe = true,
                "--purge-all" => flags.purge_all = true,
                other if other.starts_with('-') => eprintln!("unknown setup flag: {other}"),
                other => {
                    if sub.is_none() {
                        sub = Some(other.to_string());
                    }
                }
            }
        }
        (sub, flags)
    }
}

pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    let (sub, flags) = SetupFlags::parse(args);
    match sub.as_deref() {
        None | Some("install") => install(ctx, &flags),
        Some("status") => status(ctx),
        Some("uninstall") => uninstall(ctx, &flags),
        Some(other) => {
            eprintln!("unknown setup subcommand: {other}");
            eprintln!(
                "usage: spanreed setup [--yes] [--dry-run] [--service] [--no-wire] [--from-current-exe]"
            );
            eprintln!("       spanreed setup status");
            eprintln!("       spanreed setup uninstall [--purge-all] [--yes] [--dry-run]");
            ExitCode::FAILURE
        }
    }
}

fn print_line(line: Line) {
    if line.stderr {
        eprintln!("{}", line.text);
    } else {
        println!("{}", line.text);
    }
}

fn install(ctx: &AppContext, flags: &SetupFlags) -> ExitCode {
    println!("{} setup\n", crate::app::PRODUCT_NAME);
    let plan = if flags.yes {
        let plan = setup::non_interactive(ctx, flags.service);
        println!(
            "Non-interactive: install={} ledger={} service={} tray={} share_schedule={} \
             wire_grok={} wire_opencode={} dry_run={}\n",
            plan.install,
            plan.ledger,
            plan.service,
            plan.tray,
            plan.share_schedule,
            plan.wire_grok,
            plan.wire_opencode,
            flags.dry_run
        );
        plan
    } else {
        let Some(plan) = ask_plan(ctx, flags) else {
            println!("Aborted.");
            return ExitCode::SUCCESS;
        };
        println!();
        plan
    };
    let options = SetupOptions {
        dry_run: flags.dry_run,
        from_current_exe: flags.from_current_exe,
    };
    let outcome = setup::apply(ctx, &plan, options, &mut print_line);
    if outcome.aborted {
        return ExitCode::FAILURE;
    }
    println!();
    if flags.dry_run {
        println!("Dry run — no changes written.");
    } else if outcome.errors == 0 {
        println!("Done.");
        if outcome.path_changed {
            println!("  Note: open a new terminal so PATH picks up the binary.");
        }
        if plan.wire_grok {
            println!("  Note: restart Grok (or open a new shell) so the capture URL applies.");
        }
        println!("  Try: spanreed probe grok --cost");
    } else {
        println!("Finished with {} error(s).", outcome.errors);
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Interactive questions; `None` when the user declines to apply.
fn ask_plan(ctx: &AppContext, flags: &SetupFlags) -> Option<SetupPlan> {
    let hints = setup::prompt_hints(ctx);
    let install = prompt_yn("Install CLI to user PATH?", true, Some(hints.install_path));
    let ledger = prompt_yn("Create ledger directory?", true, None);
    let service = prompt_yn(
        "Start capture proxy at login (user service)?",
        flags.service,
        Some(format!(
            "fabric {}  /v1 grok  /xai api.x.ai  /acct/ID",
            crate::app::capture::DEFAULT_GROK_CLI_BIND
        )),
    );
    let tray = prompt_yn(
        "Start system tray icon at login (usage + capture status)?",
        setup::tray_default(ctx, service),
        Some(
            "Spanreed icon; shows remaining quotas and whether Grok proxy is up \
             (needs a binary built with --features tray)"
                .into(),
        ),
    );
    let share_schedule = prompt_yn(
        "Enable daily plan share to fabrials.com (once per day, catch-up when PC is on)?",
        false,
        Some(
            "requires `spanreed share login` once (Sign in with X); \
             uploads provider+plan metrics to the public pool; \
             prefers evening, also runs on login if yesterday's timer was missed"
                .into(),
        ),
    );
    let (wire_grok, wire_opencode) = if flags.no_wire {
        (false, false)
    } else {
        (
            prompt_yn("Wire Grok Build → capture :18736?", false, Some(hints.grok)),
            prompt_yn(
                "Wire OpenCode xAI → capture :18736/xai?",
                false,
                Some(hints.opencode),
            ),
        )
    };
    prompt_yn("Apply?", true, None).then_some(SetupPlan {
        install,
        ledger,
        service,
        tray,
        share_schedule,
        wire_grok,
        wire_opencode,
    })
}

fn uninstall(ctx: &AppContext, flags: &SetupFlags) -> ExitCode {
    if !flags.yes
        && !flags.dry_run
        && !prompt_yn(
            "Uninstall capture service and unwire clients (keep binary + ledger)?",
            true,
            None,
        )
    {
        println!("Aborted.");
        return ExitCode::SUCCESS;
    }
    let options = UninstallOptions {
        dry_run: flags.dry_run,
        purge_all: flags.purge_all,
    };
    if setup::uninstall(ctx, options, &mut print_line).errors == 0 {
        println!("Uninstall complete.");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn status(ctx: &AppContext) -> ExitCode {
    let status = setup::status(ctx);
    println!("spanreed setup status\n");
    println!("  CLI install path: {}", status.install_path);
    println!("  Binary present:   {}", yes_no(status.binary_present));
    println!(
        "  Current exe:      {}",
        status
            .current_exe
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "?".into())
    );
    println!(
        "  Ledger:           {} ({})",
        status.ledger.display(),
        if status.ledger_exists {
            "exists"
        } else {
            "missing"
        }
    );
    println!("  Capture service:  {}", status.capture_service);
    println!("  Tray autostart:   {}", status.tray_autostart);
    println!("  Share schedule:   {}", status.share_schedule);
    println!(
        "  Grok Build:       {} — {}",
        found(status.grok_detected),
        status.grok_wiring
    );
    println!(
        "  OpenCode xAI:     {} — {}",
        found(status.opencode_detected),
        status.opencode_wiring
    );
    println!(
        "\n  Capture fabric:   http://{}\n\
         \t/v1        → {}  (SuperGrok inject)\n\
         \t/xai/v1    → {}  (client token)\n\
         \t/acct/ID/… → same, pinned account",
        crate::app::capture::DEFAULT_GROK_CLI_BIND,
        fabrials_types::endpoints::UPSTREAM_GROK_CLI,
        fabrials_types::endpoints::UPSTREAM_XAI_API,
    );
    if status.clients_wired && !status.capture_up {
        eprintln!(
            "\n  ERROR: clients are wired to the local capture proxy, but ports are DOWN.\n\
             \tGrok Build / OpenCode will fail to connect.\n\
             \tFix: spanreed capture ensure\n\
             \t  or: spanreed setup --yes --service"
        );
        return ExitCode::FAILURE;
    }
    if !status.capture_up {
        eprintln!(
            "\n  note: capture is not listening. Token capture inactive.\n\
             \tStart: spanreed capture ensure"
        );
    }
    ExitCode::SUCCESS
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn found(detected: bool) -> &'static str {
    if detected { "detected" } else { "not found" }
}

fn prompt_yn(question: &str, default: bool, hint: Option<String>) -> bool {
    let def = if default { "Y/n" } else { "y/N" };
    let hint_s = hint.map(|h| format!(" ({h})")).unwrap_or_default();
    print!("{question}{hint_s} [{def}] ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return default;
    }
    let t = line.trim().to_ascii_lowercase();
    if t.is_empty() {
        return default;
    }
    matches!(t.as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flags_yes_service() {
        let args = vec!["--yes".into(), "--service".into(), "--dry-run".into()];
        let (sub, f) = SetupFlags::parse(&args);
        assert!(sub.is_none());
        assert!(f.yes && f.service && f.dry_run);
    }

    #[test]
    fn parse_status_subcommand() {
        let args = vec!["status".into(), "--yes".into()];
        let (sub, f) = SetupFlags::parse(&args);
        assert_eq!(sub.as_deref(), Some("status"));
        assert!(f.yes);
    }
}
