//! Installation, the capture user service, the tray autostart, the share
//! schedule and client wiring (`spanreed setup`).
use crate::context::AppContext;

/// What `spanreed setup` should do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SetupPlan {
    pub install: bool,
    pub ledger: bool,
    pub service: bool,
    pub tray: bool,
    pub share_schedule: bool,
    pub wire_grok: bool,
    pub wire_opencode: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SetupOptions {
    pub dry_run: bool,
    pub from_current_exe: bool,
}

/// One progress line; errors and warnings go to stderr.
pub struct Line {
    pub text: String,
    pub stderr: bool,
}

/// Result of a setup or uninstall run.
#[derive(Debug, Default)]
pub struct Outcome {
    pub errors: usize,
    pub path_changed: bool,
    /// Stopped before finishing (for example consent could not be saved).
    pub aborted: bool,
}

pub struct SetupStatus {
    pub install_path: String,
    pub binary_present: bool,
    pub current_exe: Option<std::path::PathBuf>,
    pub ledger: std::path::PathBuf,
    pub ledger_exists: bool,
    pub capture_service: String,
    pub tray_autostart: String,
    pub share_schedule: String,
    pub grok_detected: bool,
    pub grok_wiring: String,
    pub opencode_detected: bool,
    pub opencode_wiring: String,
    pub capture_up: bool,
    /// Some client is wired to the local capture proxy.
    pub clients_wired: bool,
}

/// Hints shown next to the interactive setup questions.
pub struct PromptHints {
    pub install_path: String,
    pub grok: String,
    pub opencode: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UninstallOptions {
    pub dry_run: bool,
    pub purge_all: bool,
}

/// Port: installation and OS service managers (systemd/launchd/Task
/// Scheduler units for capture, the tray and the share schedule).
pub trait Installer: Send + Sync {
    /// Apply `plan`, reporting one line per step.
    fn apply(&self, plan: &SetupPlan, options: SetupOptions, out: &mut dyn FnMut(Line)) -> Outcome;
    fn uninstall(&self, options: UninstallOptions, out: &mut dyn FnMut(Line)) -> Outcome;
    fn status(&self) -> SetupStatus;
    fn prompt_hints(&self) -> PromptHints;
    /// Whether the tray starts at login by default on this platform.
    fn tray_default(&self, capture_service_on: bool) -> bool;
    fn share_schedule_status(&self) -> String;
    fn enable_share_schedule(&self, dry_run: bool) -> Result<String, String>;
    fn disable_share_schedule(&self, dry_run: bool) -> Result<String, String>;
}

fn installer(ctx: &AppContext) -> &dyn Installer {
    ctx.services().installer.as_ref()
}

/// `--yes`: install and ledger; the capture service only with `--service`;
/// sharing and client rewiring need an explicit interactive choice.
pub fn non_interactive(ctx: &AppContext, service: bool) -> SetupPlan {
    SetupPlan {
        install: true,
        ledger: true,
        service,
        tray: tray_default(ctx, service),
        share_schedule: false,
        wire_grok: false,
        wire_opencode: false,
    }
}

pub fn apply(
    ctx: &AppContext,
    plan: &SetupPlan,
    options: SetupOptions,
    out: &mut dyn FnMut(Line),
) -> Outcome {
    installer(ctx).apply(plan, options, out)
}

pub fn uninstall(
    ctx: &AppContext,
    options: UninstallOptions,
    out: &mut dyn FnMut(Line),
) -> Outcome {
    installer(ctx).uninstall(options, out)
}

pub fn status(ctx: &AppContext) -> SetupStatus {
    installer(ctx).status()
}

pub fn prompt_hints(ctx: &AppContext) -> PromptHints {
    installer(ctx).prompt_hints()
}

pub fn tray_default(ctx: &AppContext, capture_service_on: bool) -> bool {
    installer(ctx).tray_default(capture_service_on)
}
