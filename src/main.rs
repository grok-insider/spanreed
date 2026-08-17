//! spanreed: Linux-native AI coding subscription usage tracker.
//!
//! Subcommands:
//!   spanreed list                 List known providers and detection state.
//!   spanreed probe [id] [flags]   Probe providers (default: quotas only).
//!   spanreed waybar               Emit Waybar custom-module JSON (one shot).
//!   spanreed json                 Emit raw JSON of all detected providers.
//!   spanreed serve [--interval S] Run the local HTTP API on 127.0.0.1:6736.
//!   spanreed capture serve        Dual capture proxy (Grok CLI + api.x.ai).
//!   spanreed capture serve --watchdog  Restart capture if it exits.
//!   spanreed capture ensure       Start capture (with watchdog) if ports down.
//!   spanreed grok-proxy [...]     Single-listener capture (compat alias).
//!   spanreed setup [...]          Install CLI, optional capture service, wire Grok/OpenCode.
//!   spanreed auth copilot [...]   Opt-in link a GitHub token for Copilot.
//!   spanreed auth logout copilot  Forget the stored Copilot credential.
//!   spanreed update-pricing [out] Fetch + filter the upstream price table.
//!   spanreed self-update […]     Check/install latest GitHub Release binary.
//!   spanreed tray […]            System tray (feature `tray`: Spanreed icon).

mod accounts;
mod activity;
mod addons;
mod api;
mod app;
mod capture_log;
mod capture_watchdog;
mod client_id;
mod cost;
mod creds;
mod drivers;
mod epoch;
mod forecast;
mod grok_ledger;
mod grok_proxy;
mod history;
mod http;
mod model;
mod output;
mod pool_baseline;
mod pricing;
mod probe;
mod proc;
mod providers;
mod secret;
mod self_update;
mod setup;
mod share;
mod share_economics;
mod share_schedule;
mod share_session;
mod tray_format;
mod usage_stats;
mod util;

#[cfg(feature = "tray")]
mod tray;

use std::process::ExitCode;

/// Restore the default SIGPIPE disposition (Unix only).
///
/// Rust ignores SIGPIPE at startup, which turns a closed downstream pipe
/// (e.g. `spanreed json | head`) into a write error that the `print!` macros
/// surface as a panic. Resetting it to `SIG_DFL` makes the process exit quietly
/// on a broken pipe, like a well-behaved Unix CLI. Non-Unix targets (Windows)
/// have no SIGPIPE, so this is a no-op there.
#[cfg(unix)]
fn reset_sigpipe() {
    // SAFETY: a single libc::signal call at startup, before any threads spawn.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

fn main() -> ExitCode {
    reset_sigpipe();
    // Capture the local timezone offset while single-threaded (used for daily
    // cost buckets); `time` can't read it reliably once threads exist.
    util::init_local_offset();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("probe");
    let rest = if args.is_empty() { &[][..] } else { &args[1..] };

    match cmd {
        "list" => cmd_list(),
        "account" => accounts::cmd(rest),
        "addon" => addons::cmd_addon(rest),
        "plugin" => addons::cmd_addon(rest),
        "probe" => cmd_probe(rest),
        "waybar" => cmd_waybar(),
        "json" => cmd_json(),
        "serve" => cmd_serve(rest),
        "history" => cmd_history(rest),
        "capture" => cmd_capture(rest),
        "grok-proxy" => cmd_grok_proxy(rest),
        "setup" => setup::cmd(rest),
        "share" => share::cmd(rest),
        "auth" => cmd_auth(rest),
        "update-pricing" => cmd_update_pricing(rest),
        "self-update" => self_update::cmd(rest),
        "tray" => cmd_tray(rest),
        "help" | "-h" | "--help" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            if let Some(code) = addons::dispatch_prefix(other, rest) {
                return code;
            }
            eprintln!("unknown command: {other}\n");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "spanreed — Linux AI subscription usage tracker\n\n\
         USAGE:\n\
         \tspanreed list                 Show providers and whether they're detected\n\
         \tspanreed probe [id] [--force] Probe detected providers, or a single id\n\
         \t  (default: rate-limit quotas only)\n\
         \t  --cost --models --cache --trend --plan   Add detail blocks\n\
         \t  --all                                    Full verbose output\n\
         \tspanreed waybar               Waybar custom-module JSON (one shot)\n\
         \tspanreed json                 Raw JSON of detected provider outputs\n\
         \tspanreed serve [--interval S] Local HTTP API on 127.0.0.1:6736\n\
         \tspanreed history [id]         Show recorded rate-limit history (JSONL)\n\
         \tspanreed capture serve        Fabric :18736  /v1 grok  /xai api.x.ai  /acct/ID\n\
         \t                               (honors HTTP(S)_PROXY for upstream egress)\n\
         \t  --watchdog                   Keep capture alive (restart on exit; logs to\n\
         \t                               %%LOCALAPPDATA%%/spanreed/logs/capture.log;\n\
         \t                               Windows: windowless / FreeConsole)\n\
         \tspanreed capture ensure      Start capture+watchdog if ports are down\n\
         \tspanreed capture status      Exit 0 if listening, 1 if DOWN; print log path\n\
         \tspanreed grok-proxy [--bind HOST:PORT]\n\
         \t                               Single-listener capture (compat)\n\
         \tspanreed setup               Install CLI, ledger, optional capture service,\n\
         \t                               and wire Grok Build + OpenCode xAI to the proxy\n\
         \t  --yes / -y                   Non-interactive defaults (service off unless --service)\n\
         \t  --service                    Enable capture user service (with --yes)\n\
         \t  --dry-run --no-wire --from-current-exe\n\
         \tspanreed setup status        Show install / wire / service state\n\
         \t                               (exit 1 if clients wired but proxy DOWN)\n\
         \tspanreed setup uninstall     Unwire clients and disable capture service\n\
         \tspanreed auth copilot         Link Copilot (opt-in; pick gh user or paste)\n\
         \t  --user LOGIN                 Import token for that gh account\n\
         \t  --token-stdin                Read token from stdin\n\
         \tspanreed auth logout copilot  Remove the stored Copilot credential\n\
         \tspanreed update-pricing [out] Fetch + filter the LiteLLM price table\n\
         \t                               (writes to stdout, or to [out]; used to\n\
         \t                               refresh the embedded src/pricing-data.json)\n\
         \tspanreed share               Upload plan/quota metrics (requires X login)\n\
         \tspanreed share login         Link CLI via device code on grokinsider.net\n\
         \tspanreed share logout|status Session management\n\
         \t                               (SPANREED_API_BASE optional)\n\
         \t                               At most once per day; setup installs\n\
         \t                               evening timer + login/missed-run catch-up\n\
         \tspanreed self-update         Install latest GitHub Release (sha256 verified)\n\
         \t  --check [--json]             Report only (exit 2 if newer)\n\
         \t  --yes --dry-run              Apply without prompt / download-only verify\n\
         \tspanreed tray [--interval S] System tray companion (needs --features tray)\n\
         \tspanreed account …           Identities (add/import/use/login grok)\n\
         \tspanreed plugin list         Drivers (in-process / toml / PATH)\n\n\
         PROVIDERS: codex, cursor, grok, opencode-go, amp, zai, minimax,\n\
         \t           synthetic, kimi, copilot, factory, devin,\n\
         \t           jetbrains-ai-assistant, kiro, antigravity, perplexity\n\
         \t           (copilot requires `spanreed auth copilot`)"
    );
}

fn cmd_tray(args: &[String]) -> ExitCode {
    #[cfg(feature = "tray")]
    {
        tray::cmd(args)
    }
    #[cfg(not(feature = "tray"))]
    {
        let _ = args;
        eprintln!(
            "tray: this binary was built without the `tray` feature\n\
             rebuild with: cargo build --release --features tray"
        );
        ExitCode::FAILURE
    }
}

fn cmd_auth(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("copilot") => match providers::copilot::cmd_auth(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("auth copilot: {e}");
                ExitCode::FAILURE
            }
        },
        Some("logout") => match args.get(1).map(String::as_str) {
            Some("copilot") => match providers::copilot::cmd_logout() {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("auth logout copilot: {e}");
                    ExitCode::FAILURE
                }
            },
            other => {
                eprintln!(
                    "unknown auth logout target: {}\nusage: spanreed auth logout copilot",
                    other.unwrap_or("(none)")
                );
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!(
                "unknown auth target: {}\nusage:\n  spanreed auth copilot [--user LOGIN | --token-stdin]\n  spanreed auth logout copilot",
                other.unwrap_or("(none)")
            );
            ExitCode::FAILURE
        }
    }
}

fn cmd_list() -> ExitCode {
    for p in providers::all() {
        let detected = if p.detect() { "detected" } else { "—" };
        println!("{:<14} {:<12} {}", p.id(), detected, p.name());
    }
    for acc in accounts::list_provider("grok") {
        let flag = if acc.active { "active" } else { "account" };
        println!("{:<14} {:<12} Grok ({})", acc.id, flag, acc.alias);
    }
    for (id, name, detected) in addons::extra_provider_ids() {
        let flag = if detected { "detected" } else { "—" };
        println!("{id:<14} {flag:<12} {name} (addon)");
    }
    ExitCode::SUCCESS
}

fn parse_probe_view(args: &[String]) -> model::ProbeView {
    let all = args.iter().any(|a| a == "--all");
    model::ProbeView {
        cost: all || args.iter().any(|a| a == "--cost"),
        models: all || args.iter().any(|a| a == "--models"),
        cache: all || args.iter().any(|a| a == "--cache"),
        trend: all || args.iter().any(|a| a == "--trend"),
        plan: all || args.iter().any(|a| a == "--plan"),
        all,
    }
}

fn cmd_probe(args: &[String]) -> ExitCode {
    let force = args.iter().any(|a| a == "--force");
    let view = parse_probe_view(args);
    let id = args.iter().find(|a| !a.starts_with("--"));

    let outputs = match id {
        Some(id) => match probe::probe_one(id) {
            Some(out) => vec![out],
            None => {
                eprintln!("unknown provider: {id}");
                return ExitCode::FAILURE;
            }
        },
        None => {
            if force {
                probe::probe_all()
            } else {
                probe::probe_detected()
            }
        }
    };

    if outputs.is_empty() {
        println!("No providers detected. Try `spanreed list` or `spanreed probe <id> --force`.");
        return ExitCode::SUCCESS;
    }

    if history::should_record_on_probe() {
        history::record(&outputs);
    }

    let text = if view.all {
        output::plain(&outputs)
    } else {
        output::plain_with_view(&outputs, view)
    };
    println!("{text}");
    let any_err = outputs.iter().any(model::ProviderOutput::has_error);
    if any_err {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_history(args: &[String]) -> ExitCode {
    let provider = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(String::as_str);
    let samples = history::read_samples(&history::history_path(), provider);
    print!("{}", history::format_table(&samples));
    ExitCode::SUCCESS
}

fn cmd_waybar() -> ExitCode {
    // Prefer the running daemon's cached data (instant); fall back to probing.
    let outputs = api::fetch_cached().unwrap_or_else(probe::probe_detected);
    let json = output::waybar(&outputs);
    println!("{json}");
    ExitCode::SUCCESS
}

fn cmd_json() -> ExitCode {
    let outputs = probe::probe_detected();
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

fn cmd_update_pricing(args: &[String]) -> ExitCode {
    let json = match pricing::fetch_filtered() {
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

fn cmd_serve(args: &[String]) -> ExitCode {
    let interval = args
        .iter()
        .position(|a| a == "--interval")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300);

    match api::serve(interval) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("server error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_capture(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("serve") | None => {
            // Optional overrides: --grok-cli-bind, --xai-api-bind, --watchdog
            let rest = if args.first().map(String::as_str) == Some("serve") {
                &args[1..]
            } else {
                args
            };
            let watchdog = rest.iter().any(|a| a == "--watchdog");
            if watchdog {
                let serve_args: Vec<String> = rest
                    .iter()
                    .filter(|a| a.as_str() != "--watchdog")
                    .cloned()
                    .collect();
                return match capture_watchdog::run(&serve_args) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("capture watchdog: {e}");
                        capture_log::append(&format!("watchdog fatal: {e}"));
                        ExitCode::FAILURE
                    }
                };
            }
            let grok_bind = rest
                .iter()
                .position(|a| a == "--grok-cli-bind")
                .and_then(|i| rest.get(i + 1))
                .cloned();
            let xai_bind = rest
                .iter()
                .position(|a| a == "--xai-api-bind")
                .and_then(|i| rest.get(i + 1))
                .cloned();
            let grok_tokens: grok_proxy::TokenSource =
                std::sync::Arc::new(|alias: Option<&str>| drivers::grok::resolve_token(alias));
            let fabric = grok_bind.unwrap_or_else(|| grok_proxy::DEFAULT_GROK_CLI_BIND.into());
            let mut listeners = vec![grok_proxy::ListenerConfig {
                bind: fabric.clone(),
                upstream: grok_proxy::UPSTREAM_GROK_CLI.into(),
                label: "fabric".into(),
                inject_bearer: None,
                token_source: Some(grok_tokens),
                force_xai: false,
            }];
            // Compat: old OpenCode still on :18737 → same as /xai on the fabric.
            if let Some(xai) = xai_bind.or_else(|| Some(grok_proxy::DEFAULT_XAI_API_BIND.into())) {
                if xai != fabric {
                    listeners.push(grok_proxy::ListenerConfig {
                        bind: xai,
                        upstream: grok_proxy::UPSTREAM_XAI_API.into(),
                        label: "xai-compat".into(),
                        inject_bearer: None,
                        token_source: None,
                        force_xai: true,
                    });
                }
            }
            capture_log::append(&format!("capture serve start fabric={}", listeners[0].bind));
            match grok_proxy::run_capture(&listeners) {
                Ok(()) => {
                    capture_log::append("capture serve exit ok");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("capture error: {e}");
                    capture_log::append(&format!("capture serve error: {e}"));
                    ExitCode::FAILURE
                }
            }
        }
        Some("ensure") => {
            let dry = args.iter().any(|a| a == "--dry-run");
            match setup::service_ensure(dry) {
                Ok(msg) => {
                    println!("{msg}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("capture ensure: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("status") => {
            let up = setup::capture_ports_up();
            let log = capture_log::capture_log_path();
            println!(
                "capture: {}",
                if up {
                    "listening (127.0.0.1:18736)"
                } else {
                    "DOWN — run `spanreed capture ensure`"
                }
            );
            println!("log:     {}", log.display());
            if up {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Some(other) => {
            eprintln!("unknown capture subcommand: {other}");
            eprintln!(
                "usage: spanreed capture serve [--watchdog] [--grok-cli-bind A] [--xai-api-bind B]\n\
                 \t spanreed capture ensure [--dry-run]\n\
                 \t spanreed capture status"
            );
            ExitCode::FAILURE
        }
    }
}

fn cmd_grok_proxy(args: &[String]) -> ExitCode {
    let bind = args
        .iter()
        .position(|a| a == "--bind")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str);
    let upstream = args
        .iter()
        .position(|a| a == "--upstream")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str);
    match grok_proxy::run(bind, upstream) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("grok-proxy error: {e}");
            ExitCode::FAILURE
        }
    }
}
