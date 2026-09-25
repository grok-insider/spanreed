//! Command-line front end for an embedding program.
//!
//! A host binary exposes these as subcommands, e.g. `spanreed agent serve`:
//!
//! - `serve`  — run the host in the foreground.
//! - `open`   — mint a pairing nonce from a running host and print its URL.
//! - `status` — report whether a host is running and whether it is paired.
//! - `doctor` — check the qualified CLI, the state directory, and the port.
//! - `stop`   — ask a running host to shut down.
//! - `repair` — rotate origin and pairings. Explicit and destructive to the
//!   existing bookmark, which is why a busy port never reaches it.
//! - `workspace add|list|remove` — manage enrolments from the terminal.
//!
//! Visiting an HTTP URL can never start a stopped host: only this command
//! can, and only through the owner-only control channel.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::cli_matrix::{self, CliQualification, MIN_QUALIFIED_CLI_LABEL};
use crate::control::{self, ControlRequest, ControlResponse};
use crate::host::{self, HostConfig, HostIdentity};
use crate::instance::{InstanceError, ensure_private_directory};
use crate::origin::ALLOWED_WEB_ORIGINS;
use crate::server::DEFAULT_AGENT_PROGRAM;
use crate::state;
use crate::state_dir::{MigrationOutcome, StateDirs, migrate_legacy_state, resolve_state_dirs};
use crate::workspace;

/// Environment variable naming the Grok Build CLI executable.
pub const AGENT_PROGRAM_ENV: &str = "FABRIALS_AGENT_PROGRAM";

/// Legacy name for [`AGENT_PROGRAM_ENV`], still honoured.
pub const LEGACY_AGENT_PROGRAM_ENV: &str = "GROK_BRIDGE_AGENT";

/// How the embedding program presents the commands.
#[derive(Debug, Clone)]
pub struct CliDefaults {
    /// Embedding program, reported by `/healthz` and used in messages.
    pub identity: HostIdentity,
    /// How users invoke these commands, e.g. `spanreed agent`.
    pub command: String,
    /// Agent executable. `None` reads [`AGENT_PROGRAM_ENV`], then
    /// [`LEGACY_AGENT_PROGRAM_ENV`], then uses `grok`.
    pub agent_program: Option<String>,
    /// Hosted document origins allowed to call the loopback API.
    pub allowed_origins: Vec<String>,
}

impl CliDefaults {
    /// Defaults for `identity`, invoked as `command`.
    #[must_use]
    pub fn new(identity: HostIdentity, command: impl Into<String>) -> Self {
        Self {
            identity,
            command: command.into(),
            agent_program: None,
            allowed_origins: ALLOWED_WEB_ORIGINS
                .iter()
                .map(|origin| (*origin).to_owned())
                .collect(),
        }
    }

    fn agent_program(&self) -> String {
        self.agent_program
            .clone()
            .or_else(|| {
                [AGENT_PROGRAM_ENV, LEGACY_AGENT_PROGRAM_ENV]
                    .into_iter()
                    .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
            })
            .unwrap_or_else(|| DEFAULT_AGENT_PROGRAM.to_owned())
    }
}

/// Run one command on a new Tokio runtime. `args` excludes the program and
/// any parent subcommand, e.g. `["workspace", "add", "."]`.
///
/// Must not be called from inside a Tokio runtime; use [`run_async`] there.
#[must_use]
pub fn run(args: &[String], defaults: CliDefaults) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!(
                "{}: could not start the async runtime: {error}",
                defaults.identity.name
            );
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run_async(args, &defaults))
}

/// Run one command on the current Tokio runtime.
pub async fn run_async(args: &[String], defaults: &CliDefaults) -> ExitCode {
    let command = args.first().map_or("help", String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    let cli = Cli { defaults };
    let outcome = match command {
        "serve" => cli.serve().await,
        "open" => cli.open().await,
        "status" => cli.status().await,
        "doctor" => cli.doctor().await,
        "stop" => cli.stop().await,
        "repair" => cli.repair().await,
        "workspace" => cli.workspace(rest),
        "help" | "--help" | "-h" => {
            cli.print_help();
            Ok(())
        }
        other => Err(format!(
            "unknown command `{other}`. Try `{} help`.",
            defaults.command
        )),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{}: {message}", defaults.identity.name);
            ExitCode::FAILURE
        }
    }
}

/// Help text for `command`.
#[must_use]
pub fn help_text(command: &str) -> String {
    format!(
        "{command} — local host for the Grok Build CLI

USAGE:
  {command} <command>

COMMANDS:
  serve     Run the host in the foreground
  open      Mint a pairing nonce and print the URL to open
  status    Report host and pairing state
  doctor    Check the qualified CLI, state directory, and port
  stop      Ask a running host to shut down
  repair    Rotate the origin and every pairing (invalidates the bookmark)

  workspace add <path>     Enrol a directory the agent may work in
  workspace list           List enrolled workspaces
  workspace remove <id>    Forget an enrolment

The host drives the Grok Build CLI you already installed, with your own
configuration and your own authority. It is a control surface, not a sandbox."
    )
}

struct Cli<'a> {
    defaults: &'a CliDefaults,
}

impl Cli<'_> {
    fn name(&self) -> &'static str {
        self.defaults.identity.name
    }

    fn command(&self) -> &str {
        &self.defaults.command
    }

    fn print_help(&self) {
        println!("{}", help_text(self.command()));
    }

    /// Resolve the state directory and adopt grok-bridge state when it is the
    /// platform default and still uninitialised.
    fn state_directory(&self) -> Result<PathBuf, String> {
        let StateDirs {
            state_dir,
            legacy_state_dir,
        } = resolve_state_dirs().map_err(|error| error.to_string())?;
        if let Some(legacy) = legacy_state_dir {
            self.migrate(&state_dir, &legacy)?;
        }
        Ok(state_dir)
    }

    fn migrate(&self, state_dir: &Path, legacy: &Path) -> Result<(), String> {
        match migrate_legacy_state(state_dir, legacy).map_err(|error| error.to_string())? {
            MigrationOutcome::Migrated { port, .. } => {
                eprintln!(
                    "{}: adopted grok-bridge state from {} (same origin, port {port})",
                    self.name(),
                    legacy.display()
                );
            }
            MigrationOutcome::AlreadyInitialised | MigrationOutcome::NoLegacyState => {}
        }
        Ok(())
    }

    async fn serve(&self) -> Result<(), String> {
        let directory = self.state_directory()?;
        let program = self.defaults.agent_program();
        if let Err(message) = probe_agent(&program) {
            eprintln!(
                "{}: {message} The host keeps retrying in the background.",
                self.name()
            );
        }
        let mut config = HostConfig::new(directory, self.defaults.identity);
        config.agent_program = program;
        config.allowed_origins = self.defaults.allowed_origins.clone();

        let running = host::start(config).await.map_err(|error| match error {
            host::Error::Bind { .. } => format!(
                "{error}. Retry, or run `{} repair` to rotate them.",
                self.command()
            ),
            other => other.to_string(),
        })?;
        let origin = running.origin().clone();
        println!("{} listening on {origin}", self.name());
        println!(
            "also reachable at http://127.0.0.1:{} (use when *.localhost resolves to IPv6 only)",
            origin.port()
        );
        if running.ipv6_bound() {
            println!("IPv6 loopback [::1]:{} bound", origin.port());
        } else {
            eprintln!(
                "{}: could not bind [::1]:{}; hostname URLs that resolve only to IPv6 may fail — use http://127.0.0.1:{}",
                self.name(),
                origin.port(),
                origin.port()
            );
        }
        println!(
            "run `{} open` in this account to pair a browser",
            self.command()
        );
        running.wait().await.map_err(|error| error.to_string())
    }

    async fn open(&self) -> Result<(), String> {
        let directory = self.state_directory()?;
        match control::call(&directory, &ControlRequest::MintNonce).await {
            Ok(ControlResponse::Paired { url, .. }) => {
                println!("{url}");
                // desktop.grok.me needs local-network permission; 127.0.0.1
                // pairs against the embedded SPA without DNS / LNA issues.
                if let Some(loopback) = loopback_pair_url(&url) {
                    println!("{loopback}");
                }
                println!(
                    "\nOpen either URL once to pair this browser, then bookmark the address \
                     without the fragment. Prefer the 127.0.0.1 link if desktop.grok.me cannot \
                     reach the host."
                );
                Ok(())
            }
            Ok(ControlResponse::Error { code }) => Err(format!("host refused to pair: {code}")),
            Ok(other) => Err(format!("unexpected response: {other:?}")),
            Err(error) => Err(format!(
                "{error}. Start one with `{} serve` — visiting the URL cannot \
                 start a stopped host.",
                self.command()
            )),
        }
    }

    async fn status(&self) -> Result<(), String> {
        let directory = self.state_directory()?;
        match control::call(&directory, &ControlRequest::Status).await {
            Ok(ControlResponse::Status {
                origin,
                paired,
                controlled,
            }) => {
                println!("running   {origin}");
                println!("paired    {}", if paired { "yes" } else { "no" });
                println!("in use    {}", if controlled { "yes" } else { "no" });
                Ok(())
            }
            Ok(other) => Err(format!("unexpected response: {other:?}")),
            Err(_) => {
                println!("running   no");
                Ok(())
            }
        }
    }

    async fn doctor(&self) -> Result<(), String> {
        let directory = self.state_directory()?;
        println!(
            "host       {} {}",
            self.name(),
            self.defaults.identity.version
        );
        println!("state dir  {}", directory.display());
        if let Err(error) = ensure_private_directory(&directory) {
            println!("state dir  unusable: {error}");
            if matches!(error, InstanceError::ForeignOwner) {
                #[cfg(unix)]
                println!(
                    "           Fix: chmod 700 '{}'  (state must be owner-only)",
                    directory.display()
                );
                #[cfg(windows)]
                println!(
                    "           Fix: remove '{}' and re-run so the host can recreate \
                     an owner-only DACL",
                    directory.display()
                );
            }
        }

        match state::load_or_create(&directory) {
            Ok(identity) => {
                println!("install id {}", identity.install_id);
                println!("port       {}", identity.port);
                match identity.origin() {
                    Ok(origin) => println!("origin     {origin}"),
                    Err(error) => println!("origin     unusable: {error}"),
                }
            }
            Err(error) => println!("identity   unusable: {error}"),
        }

        // Support diagnostics against the qualified matrix, not a security
        // boundary.
        match cli_matrix::qualify_program(&self.defaults.agent_program()) {
            CliQualification::Known {
                version,
                meets_minimum,
            } if meets_minimum => {
                println!("grok cli   {version} (qualified, min {MIN_QUALIFIED_CLI_LABEL})");
            }
            CliQualification::Known { version, .. } => {
                println!(
                    "grok cli   {version} — below qualified minimum {MIN_QUALIFIED_CLI_LABEL}"
                );
                println!(
                    "           Upgrade Grok Build for history integrity, session load, \
                     and review features the host expects."
                );
            }
            CliQualification::Unavailable { reason } => {
                println!("grok cli   unavailable: {reason}");
                println!(
                    "           The host drives the CLI you install and authenticate yourself."
                );
            }
        }

        match control::call(&directory, &ControlRequest::Status).await {
            Ok(_) => println!("host       running"),
            Err(_) => println!("host       not running"),
        }
        Ok(())
    }

    async fn stop(&self) -> Result<(), String> {
        let directory = self.state_directory()?;
        if control::call(&directory, &ControlRequest::Stop)
            .await
            .is_ok()
        {
            println!("stop requested");
        } else {
            println!("no running host");
        }
        Ok(())
    }

    async fn repair(&self) -> Result<(), String> {
        let directory = self.state_directory()?;
        if control::call(&directory, &ControlRequest::Status)
            .await
            .is_ok()
        {
            return Err(
                "a host is still running. Stop it first: repair rotates the origin and \
                 invalidates every pairing."
                    .to_owned(),
            );
        }
        ensure_private_directory(&directory).map_err(|error| error.to_string())?;
        let identity = state::rotate(&directory).map_err(|error| error.to_string())?;
        let origin = identity.origin().map_err(|error| error.to_string())?;
        println!("origin rotated to {origin}");
        println!("every previous pairing and bookmark is now invalid");
        Ok(())
    }

    /// Enrolment from the terminal. The browser never reaches here: it can
    /// only ask the host to open its own picker, and afterwards refers to the
    /// result by an opaque id.
    fn workspace(&self, args: &[String]) -> Result<(), String> {
        let directory = self.state_directory()?;
        ensure_private_directory(&directory).map_err(|error| error.to_string())?;
        let mut index = workspace::load(&directory).map_err(|error| error.to_string())?;

        let action = args.first().map_or("list", String::as_str);
        match action {
            "add" => {
                let raw = args
                    .get(1)
                    .ok_or_else(|| format!("usage: {} workspace add <path>", self.command()))?;
                let entry = index
                    .enrol(Path::new(raw), crate::now_ms())
                    .map_err(|error| error.to_string())?;
                workspace::persist(&directory, &index).map_err(|error| error.to_string())?;
                println!("{}  {}", entry.id, entry.canonical_path.display());
                println!(
                    "\nThe agent will run here with your own authority. The host is a \
                     control surface, not a sandbox."
                );
                Ok(())
            }
            "remove" => {
                let id = args
                    .get(1)
                    .ok_or_else(|| format!("usage: {} workspace remove <id>", self.command()))?;
                index.remove(id).map_err(|error| error.to_string())?;
                workspace::persist(&directory, &index).map_err(|error| error.to_string())?;
                println!("removed {id}");
                Ok(())
            }
            "list" => {
                if index.is_empty() {
                    println!(
                        "no workspaces enrolled — try `{} workspace add <path>`",
                        self.command()
                    );
                    return Ok(());
                }
                for entry in index.entries() {
                    // A swapped directory is visible before a session uses it.
                    let status = match index.resolve(&entry.id) {
                        Ok(_) => "ok",
                        Err(workspace::WorkspaceError::IdentityChanged) => "changed",
                        Err(_) => "missing",
                    };
                    println!(
                        "{:<8} {:<8} {}",
                        entry.id,
                        status,
                        entry.canonical_path.display()
                    );
                }
                Ok(())
            }
            other => Err(format!(
                "unknown workspace action `{other}`. Try add, list, or remove."
            )),
        }
    }
}

/// Check that the agent executable runs. The web surface never influences
/// which program this is.
fn probe_agent(program: &str) -> Result<(), String> {
    let probe = std::process::Command::new(program)
        .arg("--version")
        .output()
        .map_err(|_| {
            format!("`{program}` was not found. Install and authenticate the Grok Build CLI first.")
        })?;
    if probe.status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{program} --version` failed; the CLI is not usable."
        ))
    }
}

/// The `http://127.0.0.1:<port>/#pair=…&p=<port>` form of a hosted pair URL.
fn loopback_pair_url(url: &str) -> Option<String> {
    let (document, fragment) = url.split_once('#')?;
    if document.starts_with("http://127.0.0.1:") {
        return None;
    }
    let port = fragment
        .split('&')
        .find_map(|part| part.strip_prefix("p="))?;
    let pair = fragment
        .split('&')
        .find_map(|part| part.strip_prefix("pair="))?;
    Some(format!("http://127.0.0.1:{port}/#pair={pair}&p={port}"))
}

#[cfg(test)]
mod tests {
    use super::{CliDefaults, help_text, loopback_pair_url, run};
    use crate::HostIdentity;

    #[test]
    fn help_names_the_embedding_command() {
        let text = help_text("spanreed agent");
        assert!(text.starts_with("spanreed agent — local host"));
        assert!(text.contains("  spanreed agent <command>"));
    }

    #[test]
    fn help_and_unknown_commands_exit_accordingly() {
        let defaults = CliDefaults::new(HostIdentity::new("test-host", "0.0.0"), "test-host agent");
        assert_eq!(
            run(&["help".to_owned()], defaults.clone()),
            std::process::ExitCode::SUCCESS
        );
        assert_eq!(
            run(&["nope".to_owned()], defaults),
            std::process::ExitCode::FAILURE
        );
    }

    #[test]
    fn the_hosted_pair_url_has_a_loopback_twin() {
        assert_eq!(
            loopback_pair_url("https://desktop.grok.me/#pair=abc&p=24601").as_deref(),
            Some("http://127.0.0.1:24601/#pair=abc&p=24601")
        );
        assert_eq!(
            loopback_pair_url("http://127.0.0.1:24601/#pair=abc&p=24601"),
            None
        );
        assert_eq!(loopback_pair_url("https://desktop.grok.me/"), None);
    }
}
