//! Interactive install / wire / capture-service setup for spanreed.
//!
//! ```text
//! spanreed setup                 Interactive
//! spanreed setup --yes           Non-interactive defaults
//! spanreed setup --dry-run
//! spanreed setup status
//! spanreed setup uninstall
//! ```

mod detect;
mod paths;
mod service;
mod state;
mod wire_grok;
mod wire_opencode;

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use crate::grok_proxy;

pub use paths::install_bin_path;

/// Re-export for `main` capture ensure/status without exposing the whole module tree.
pub fn service_ensure(dry_run: bool) -> Result<String, String> {
    service::ensure(dry_run)
}

pub fn capture_ports_up() -> bool {
    service::ports_up()
}

/// Target base URL for Grok Build (`GROK_CLI_CHAT_PROXY_BASE_URL`).
pub const GROK_CAPTURE_BASE_URL: &str = "http://127.0.0.1:18736/v1";
/// Target base URL for OpenCode `provider.xai.options.baseURL`.
pub const OPENCODE_XAI_CAPTURE_BASE_URL: &str = "http://127.0.0.1:18737/v1";

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
        let mut i = 0;
        while i < args.len() {
            let a = args[i].as_str();
            match a {
                "--yes" | "-y" => flags.yes = true,
                "--dry-run" => flags.dry_run = true,
                "--service" => flags.service = true,
                "--no-wire" => flags.no_wire = true,
                "--from-current-exe" => flags.from_current_exe = true,
                "--purge-all" => flags.purge_all = true,
                "status" | "uninstall" => {
                    if sub.is_none() {
                        sub = Some(a.to_string());
                    }
                }
                other if other.starts_with('-') => {
                    eprintln!("unknown setup flag: {other}");
                }
                other => {
                    if sub.is_none() {
                        sub = Some(other.to_string());
                    }
                }
            }
            i += 1;
        }
        (sub, flags)
    }
}

/// Entry from `main`.
pub fn cmd(args: &[String]) -> ExitCode {
    let (sub, flags) = SetupFlags::parse(args);
    match sub.as_deref() {
        None | Some("install") => run_setup(flags),
        Some("status") => print_status(),
        Some("uninstall") => run_uninstall(flags),
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

fn run_setup(flags: SetupFlags) -> ExitCode {
    let det = detect::scan();
    println!("spanreed setup\n");

    let (do_install, do_ledger, do_service, do_share_schedule, do_wire_grok, do_wire_opencode) =
        if !flags.yes {
            let do_install = prompt_yn(
                "Install CLI to user PATH?",
                true,
                Some(format_hint_install()),
            );
            let do_ledger = prompt_yn("Create ledger directory?", true, None);
            let do_service = prompt_yn(
                "Start capture proxy at login (user service)?",
                flags.service,
                Some(format!(
                    "listens {} + {}",
                    grok_proxy::DEFAULT_GROK_CLI_BIND,
                    grok_proxy::DEFAULT_XAI_API_BIND
                )),
            );
            let do_share_schedule = prompt_yn(
                "Enable daily anonymous plan share (23:00 Europe/Madrid)?",
                true,
                Some(
                    "uploads provider+plan metrics to the public pool (no login, no user id)"
                        .into(),
                ),
            );
            let (do_wire_grok, do_wire_opencode) = if flags.no_wire {
                (false, false)
            } else {
                (
                    prompt_yn(
                        "Wire Grok Build → capture :18736?",
                        det.grok.detected,
                        Some(det.grok.hint()),
                    ),
                    prompt_yn(
                        "Wire OpenCode xAI → capture :18737?",
                        det.opencode.detected,
                        Some(det.opencode.hint()),
                    ),
                )
            };
            if !prompt_yn("Apply?", true, None) {
                println!("Aborted.");
                return ExitCode::SUCCESS;
            }
            println!();
            (
                do_install,
                do_ledger,
                do_service,
                do_share_schedule,
                do_wire_grok,
                do_wire_opencode,
            )
        } else {
            // --yes: service only if --service; share schedule ON by default;
            // wire only if detected and not --no-wire
            let do_install = true;
            let do_ledger = true;
            let do_service = flags.service;
            let do_share_schedule = true;
            let do_wire_grok = !flags.no_wire && det.grok.detected;
            let do_wire_opencode = !flags.no_wire && det.opencode.detected;
            println!(
                "Non-interactive: install={do_install} ledger={do_ledger} service={do_service} \
                 share_schedule={do_share_schedule} wire_grok={do_wire_grok} \
                 wire_opencode={do_wire_opencode} dry_run={}\n",
                flags.dry_run
            );
            (
                do_install,
                do_ledger,
                do_service,
                do_share_schedule,
                do_wire_grok,
                do_wire_opencode,
            )
        };

    let mut state = state::load().unwrap_or_default();
    let mut path_changed = false;
    let mut errors: Vec<String> = Vec::new();

    if do_install {
        match install_cli(flags.dry_run, flags.from_current_exe) {
            Ok(InstallResult { path, path_updated }) => {
                println!("  CLI:      {}", path.display());
                state.install_path = Some(path.display().to_string());
                path_changed = path_updated;
            }
            Err(e) => {
                eprintln!("  CLI:      error: {e}");
                errors.push(e);
            }
        }
    }

    if do_ledger {
        match ensure_ledger(flags.dry_run) {
            Ok(p) => println!("  Ledger:   {}", p.display()),
            Err(e) => {
                eprintln!("  Ledger:   error: {e}");
                errors.push(e);
            }
        }
    }

    if do_service {
        match service::enable(flags.dry_run) {
            Ok(msg) => {
                println!("  Capture:  {msg}");
                state.service = Some(state::ServiceState {
                    enabled: true,
                    kind: service::kind_label().into(),
                });
            }
            Err(e) => {
                eprintln!("  Capture:  error: {e}");
                errors.push(e);
            }
        }
    } else if do_wire_grok || do_wire_opencode {
        // Wiring without a live proxy silently breaks clients — ensure process at least.
        match service::ensure(flags.dry_run) {
            Ok(msg) => println!("  Capture:  {msg} (no autostart; use --service for login)"),
            Err(e) => {
                eprintln!("  Capture:  warning: {e}");
                eprintln!("            clients may fail until: spanreed capture ensure");
            }
        }
    } else {
        println!("  Capture:  not enabled (run `spanreed capture ensure` or `--service`)");
    }

    // Always ensure anonymous install id (used by share anti-abuse).
    if !flags.dry_run {
        match crate::client_id::ensure() {
            Ok(id) => println!(
                "  Client:   {}… (anonymous install id)",
                id.chars().take(8).collect::<String>()
            ),
            Err(e) => {
                eprintln!("  Client:   error: {e}");
                errors.push(e);
            }
        }
    }

    if do_share_schedule {
        match crate::share_schedule::enable(flags.dry_run) {
            Ok(msg) => {
                println!("  Share:    {msg}");
                state.share_schedule = Some(state::ServiceState {
                    enabled: true,
                    kind: crate::share_schedule::kind_label().into(),
                });
            }
            Err(e) => {
                eprintln!("  Share:    error: {e}");
                errors.push(e);
            }
        }
    } else {
        println!("  Share:    schedule not enabled (daily 23:00 Europe/Madrid off)");
    }

    if do_wire_grok {
        match wire_grok::wire(flags.dry_run, &mut state) {
            Ok(msg) => println!("  Grok:     {msg}"),
            Err(e) => {
                eprintln!("  Grok:     error: {e}");
                errors.push(e);
            }
        }
    }

    if do_wire_opencode {
        match wire_opencode::wire(flags.dry_run, &mut state, &det) {
            Ok(msg) => println!("  OpenCode: {msg}"),
            Err(e) => {
                eprintln!("  OpenCode: error: {e}");
                errors.push(e);
            }
        }
    }

    if !flags.dry_run {
        if let Err(e) = state::save(&state) {
            eprintln!("  warning: could not save setup state: {e}");
        }
    }

    println!();
    if flags.dry_run {
        println!("Dry run — no changes written.");
    } else if errors.is_empty() {
        println!("Done.");
        if path_changed {
            println!("  Note: open a new terminal so PATH picks up the binary.");
        }
        if do_wire_grok {
            println!("  Note: restart Grok (or open a new shell) so the capture URL applies.");
        }
        println!("  Try: spanreed probe grok --cost");
    } else {
        println!("Finished with {} error(s).", errors.len());
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run_uninstall(flags: SetupFlags) -> ExitCode {
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

    let mut state = state::load().unwrap_or_default();
    let mut errors = Vec::new();

    match service::disable(flags.dry_run) {
        Ok(msg) => println!("  Capture:  {msg}"),
        Err(e) => {
            eprintln!("  Capture:  {e}");
            errors.push(e);
        }
    }
    state.service = Some(state::ServiceState {
        enabled: false,
        kind: service::kind_label().into(),
    });

    match crate::share_schedule::disable(flags.dry_run) {
        Ok(msg) => println!("  Share:    {msg}"),
        Err(e) => {
            eprintln!("  Share:    {e}");
            errors.push(e);
        }
    }
    state.share_schedule = Some(state::ServiceState {
        enabled: false,
        kind: crate::share_schedule::kind_label().into(),
    });

    match wire_grok::unwire(flags.dry_run, &mut state) {
        Ok(msg) => println!("  Grok:     {msg}"),
        Err(e) => {
            eprintln!("  Grok:     {e}");
            errors.push(e);
        }
    }

    match wire_opencode::unwire(flags.dry_run, &mut state) {
        Ok(msg) => println!("  OpenCode: {msg}"),
        Err(e) => {
            eprintln!("  OpenCode: {e}");
            errors.push(e);
        }
    }

    if flags.purge_all {
        let ledger = crate::grok_ledger::ledger_path();
        if flags.dry_run {
            println!("  Ledger:   would remove {}", ledger.display());
        } else if ledger.exists() {
            match std::fs::remove_file(&ledger) {
                Ok(()) => println!("  Ledger:   removed {}", ledger.display()),
                Err(e) => {
                    let m = format!("remove ledger: {e}");
                    eprintln!("  Ledger:   {m}");
                    errors.push(m);
                }
            }
        }
        let bin = install_bin_path();
        if flags.dry_run {
            println!("  CLI:      would remove {}", bin.display());
        } else if bin.exists() {
            match std::fs::remove_file(&bin) {
                Ok(()) => println!("  CLI:      removed {}", bin.display()),
                Err(e) => {
                    let m = format!("remove binary: {e}");
                    eprintln!("  CLI:      {m}");
                    errors.push(m);
                }
            }
        }
        state.install_path = None;
    }

    if !flags.dry_run {
        let _ = state::save(&state);
    }

    if errors.is_empty() {
        println!("Uninstall complete.");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_status() -> ExitCode {
    let det = detect::scan();
    let state = state::load().unwrap_or_default();
    let bin = install_bin_path();
    let ledger = crate::grok_ledger::ledger_path();
    let ports_up = service::ports_up();
    let grok_wired = wire_grok::is_wired_to_capture(&state);
    let oc_wired = wire_opencode::is_wired_to_capture(&det, &state);

    println!("spanreed setup status\n");
    println!(
        "  CLI install path: {}",
        state
            .install_path
            .as_deref()
            .unwrap_or(&bin.display().to_string())
    );
    println!(
        "  Binary present:   {}",
        if bin.exists() { "yes" } else { "no" }
    );
    println!(
        "  Current exe:      {}",
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "?".into())
    );
    println!(
        "  Ledger:           {} ({})",
        ledger.display(),
        if ledger.exists() { "exists" } else { "missing" }
    );

    let svc = service::status();
    println!("  Capture service:  {svc}");
    println!(
        "  Share schedule:   {}",
        crate::share_schedule::status()
    );

    println!(
        "  Grok Build:       {} — {}",
        if det.grok.detected {
            "detected"
        } else {
            "not found"
        },
        wire_grok::status_line(&state)
    );
    println!(
        "  OpenCode xAI:     {} — {}",
        if det.opencode.detected {
            "detected"
        } else {
            "not found"
        },
        wire_opencode::status_line(&det, &state)
    );
    println!(
        "\n  Capture targets:\n    Grok CLI  http://{} → {}\n    api.x.ai  http://{} → {}",
        grok_proxy::DEFAULT_GROK_CLI_BIND,
        grok_proxy::UPSTREAM_GROK_CLI,
        grok_proxy::DEFAULT_XAI_API_BIND,
        grok_proxy::UPSTREAM_XAI_API,
    );

    if (grok_wired || oc_wired) && !ports_up {
        eprintln!(
            "\n  ERROR: clients are wired to the local capture proxy, but ports are DOWN.\n\
             \tGrok Build / OpenCode will fail to connect.\n\
             \tFix: spanreed capture ensure\n\
             \t  or: spanreed setup --yes --service"
        );
        return ExitCode::FAILURE;
    }
    if !ports_up {
        eprintln!(
            "\n  note: capture is not listening. Token capture inactive.\n\
             \tStart: spanreed capture ensure"
        );
    }
    ExitCode::SUCCESS
}

struct InstallResult {
    path: std::path::PathBuf,
    path_updated: bool,
}

fn install_cli(dry_run: bool, force_from_current: bool) -> Result<InstallResult, String> {
    let dest = install_bin_path();
    let current = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    // Same path (e.g. install.ps1 already copied us into place, then re-exec'd setup):
    // never try to copy a running Windows PE onto itself (error 32).
    let same_file = paths::same_file(&current, &dest);
    let need_copy = !same_file
        && (force_from_current
            || !dest.exists()
            || paths::canonicalize_opt(&current) != paths::canonicalize_opt(&dest));

    if dry_run {
        return Ok(InstallResult {
            path: dest,
            path_updated: false,
        });
    }

    if need_copy {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir install dir: {e}"))?;
        }
        match std::fs::copy(&current, &dest) {
            Ok(_) => {}
            Err(e) if dest.exists() && same_file_or_busy(&e) => {
                // Dest already good / locked by our own process — treat as installed.
            }
            Err(e) => {
                return Err(format!(
                    "copy {} → {}: {e}",
                    current.display(),
                    dest.display()
                ));
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&dest)
                .map_err(|e| format!("stat dest: {e}"))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&dest, perms).map_err(|e| format!("chmod: {e}"))?;
        }
    }

    let path_updated = paths::ensure_install_dir_on_user_path(dry_run)?;
    Ok(InstallResult {
        path: dest,
        path_updated,
    })
}

fn same_file_or_busy(err: &std::io::Error) -> bool {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied => true,
        _ => {
            let msg = err.to_string().to_ascii_lowercase();
            msg.contains("being used by another process")
                || msg.contains("os error 32")
                || msg.contains("text file busy")
        }
    }
}

fn ensure_ledger(dry_run: bool) -> Result<std::path::PathBuf, String> {
    let path = crate::grok_ledger::ledger_path();
    if let Some(parent) = path.parent() {
        if dry_run {
            return Ok(path);
        }
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir ledger dir: {e}"))?;
    }
    if !dry_run && !path.exists() {
        // Touch empty ledger so path is visible.
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("create ledger: {e}"))?;
    }
    Ok(path)
}

fn format_hint_install() -> String {
    format!("{}", install_bin_path().display())
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

    #[test]
    fn capture_urls_include_v1() {
        assert!(GROK_CAPTURE_BASE_URL.ends_with("/v1"));
        assert!(OPENCODE_XAI_CAPTURE_BASE_URL.contains("18737"));
    }
}
