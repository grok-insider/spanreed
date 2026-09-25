//! Applying a setup plan: install, ledger, capture service, tray, share
//! schedule and client wiring. The CLI builds the plan (prompts or `--yes`)
//! and prints each reported line as it arrives.

use super::{
    detect, install, service, share_schedule, state, tray_autostart, wire_grok, wire_opencode,
};

pub use spanreed_app::app::setup::Line;
pub use spanreed_app::app::setup::Outcome;
pub use spanreed_app::app::setup::SetupOptions;
pub use spanreed_app::app::setup::SetupPlan;
pub(super) struct Reporter<'a> {
    out: &'a mut dyn FnMut(Line),
    pub(super) outcome: Outcome,
}

impl<'a> Reporter<'a> {
    pub(super) fn new(out: &'a mut dyn FnMut(Line)) -> Self {
        Self {
            out,
            outcome: Outcome::default(),
        }
    }

    pub(super) fn ok(&mut self, text: String) {
        (self.out)(Line {
            text,
            stderr: false,
        });
    }

    pub(super) fn warn(&mut self, text: String) {
        (self.out)(Line { text, stderr: true });
    }

    pub(super) fn fail(&mut self, text: String) {
        self.warn(text);
        self.outcome.errors += 1;
    }

    /// `label` padded like the other rows; `Err` counts as an error.
    pub(super) fn step(&mut self, label: &str, result: Result<String, String>, error_prefix: &str) {
        match result {
            Ok(message) => self.ok(format!("  {label}{message}")),
            Err(error) => self.fail(format!("  {label}{error_prefix}{error}")),
        }
    }
}

/// Apply `plan`, reporting one line per step.
pub fn apply(plan: &SetupPlan, options: SetupOptions, out: &mut dyn FnMut(Line)) -> Outcome {
    let detection = detect::scan();
    let mut report = Reporter::new(out);
    let mut state = state::load().unwrap_or_default();
    if plan.install {
        install_step(&mut report, &mut state, options);
    }
    if plan.ledger {
        let ledger = install::ensure_ledger(options.dry_run).map(|p| p.display().to_string());
        report.step("Ledger:   ", ledger, "error: ");
    }
    capture_step(&mut report, &mut state, plan, options.dry_run);
    tray_step(&mut report, &mut state, plan.tray, options.dry_run);
    if !options.dry_run {
        let client = crate::client_id::ensure().map(|id| {
            format!(
                "{}… (install device id)",
                id.chars().take(8).collect::<String>()
            )
        });
        report.step("Client:   ", client, "error: ");
    }
    if !share_step(
        &mut report,
        &mut state,
        plan.share_schedule,
        options.dry_run,
    ) {
        report.outcome.aborted = true;
        return report.outcome;
    }
    if plan.wire_grok {
        report.step(
            "Grok:     ",
            wire_grok::wire(options.dry_run, &mut state),
            "error: ",
        );
    }
    if plan.wire_opencode {
        let wired = wire_opencode::wire(options.dry_run, &mut state, &detection);
        report.step("OpenCode: ", wired, "error: ");
    }
    if !options.dry_run
        && let Err(error) = state::save(&state)
    {
        report.warn(format!("  warning: could not save setup state: {error}"));
    }
    report.outcome
}

fn install_step(report: &mut Reporter<'_>, state: &mut state::SetupState, options: SetupOptions) {
    match install::install_cli(options.dry_run, options.from_current_exe) {
        Ok(installed) => {
            report.ok(format!("  CLI:      {}", installed.path.display()));
            state.install_path = Some(installed.path.display().to_string());
            report.outcome.path_changed = installed.path_updated;
        }
        Err(error) => report.fail(format!("  CLI:      error: {error}")),
    }
}

fn capture_step(
    report: &mut Reporter<'_>,
    state: &mut state::SetupState,
    plan: &SetupPlan,
    dry_run: bool,
) {
    if plan.service {
        match service::enable(dry_run) {
            Ok(message) => {
                report.ok(format!("  Capture:  {message}"));
                state.service = Some(enabled(service::kind_label()));
            }
            Err(error) => report.fail(format!("  Capture:  error: {error}")),
        }
    } else if plan.wire_grok || plan.wire_opencode {
        // Wiring without a live proxy silently breaks clients — ensure process at least.
        match service::ensure(dry_run) {
            Ok(message) => report.ok(format!(
                "  Capture:  {message} (no autostart; use --service for login)"
            )),
            Err(error) => {
                report.warn(format!("  Capture:  warning: {error}"));
                report.warn("            clients may fail until: spanreed capture ensure".into());
            }
        }
    } else {
        report.ok("  Capture:  not enabled (run `spanreed capture ensure` or `--service`)".into());
    }
}

fn tray_step(report: &mut Reporter<'_>, state: &mut state::SetupState, tray: bool, dry_run: bool) {
    if !tray {
        report.ok("  Tray:     not enabled (run `spanreed tray` or re-run setup)".into());
        return;
    }
    match tray_autostart::resolve_bin().and_then(|bin| tray_autostart::enable(&bin, dry_run)) {
        Ok(message) => {
            report.ok(format!("  Tray:     {message}"));
            state.tray = Some(enabled(tray_autostart::kind_label()));
        }
        Err(error) => report.fail(format!("  Tray:     error: {error}")),
    }
}

/// Returns `false` when sharing consent could not be saved (setup stops).
fn share_step(
    report: &mut Reporter<'_>,
    state: &mut state::SetupState,
    schedule: bool,
    dry_run: bool,
) -> bool {
    if !schedule {
        report.ok("  Share:    schedule not enabled (daily auto-share off)".into());
        return true;
    }
    if !dry_run {
        let mut consent = crate::privacy::load();
        consent.share_metrics = true;
        if let Err(error) = crate::privacy::save(&consent) {
            report.fail(format!("Could not save sharing consent: {error}"));
            return false;
        }
    }
    match share_schedule::enable(dry_run) {
        Ok(message) => {
            report.ok(format!("  Share:    {message}"));
            state.share_schedule = Some(enabled(share_schedule::kind_label()));
        }
        Err(error) => report.fail(format!("  Share:    error: {error}")),
    }
    true
}

pub(super) fn enabled(kind: &str) -> state::ServiceState {
    state::ServiceState {
        enabled: true,
        kind: kind.into(),
    }
}

pub(super) fn disabled(kind: &str) -> state::ServiceState {
    state::ServiceState {
        enabled: false,
        kind: kind.into(),
    }
}
