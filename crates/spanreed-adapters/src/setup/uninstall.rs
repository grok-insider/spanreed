//! `spanreed setup uninstall`: disable services and unwire clients; with
//! `purge_all` also remove usage data and the installed binary.

use super::apply::{Line, Outcome, Reporter, disabled};
use super::{
    install_bin_path, service, share_schedule, state, tray_autostart, wire_grok, wire_opencode,
};

pub use spanreed_app::app::setup::UninstallOptions;
const USAGE_FILES: [&str; 5] = [
    "runtime.sqlite3",
    "runtime.sqlite3-wal",
    "runtime.sqlite3-shm",
    "grok-usage.jsonl",
    "usage-history.jsonl",
];

pub fn uninstall(options: UninstallOptions, out: &mut dyn FnMut(Line)) -> Outcome {
    let dry_run = options.dry_run;
    let mut report = Reporter::new(out);
    let mut state = state::load().unwrap_or_default();
    report.step("Capture:  ", service::disable(dry_run), "");
    state.service = Some(disabled(service::kind_label()));
    report.step("Tray:     ", tray_autostart::disable(dry_run), "");
    state.tray = Some(disabled(tray_autostart::kind_label()));
    report.step("Share:    ", share_schedule::disable(dry_run), "");
    state.share_schedule = Some(disabled(share_schedule::kind_label()));
    report.step("Grok:     ", wire_grok::unwire(dry_run, &mut state), "");
    report.step("OpenCode: ", wire_opencode::unwire(dry_run, &mut state), "");
    if options.purge_all {
        purge_usage(&mut report, dry_run);
        purge_binary(&mut report, dry_run);
        state.install_path = None;
    }
    if !dry_run {
        let _ = state::save(&state);
    }
    report.outcome
}

/// Remove import sources too, so a later install cannot restore purged usage.
fn purge_usage(report: &mut Reporter<'_>, dry_run: bool) {
    for name in USAGE_FILES {
        let ledger = crate::product::data_dir().join(name);
        if dry_run {
            report.ok(format!("  Usage:    would remove {}", ledger.display()));
        } else if ledger.exists()
            && let Err(error) = std::fs::remove_file(&ledger)
        {
            report.fail(format!("  Usage:    remove usage data: {error}"));
        }
    }
}

fn purge_binary(report: &mut Reporter<'_>, dry_run: bool) {
    let bin = install_bin_path();
    if dry_run {
        report.ok(format!("  CLI:      would remove {}", bin.display()));
    } else if bin.exists() {
        match std::fs::remove_file(&bin) {
            Ok(()) => report.ok(format!("  CLI:      removed {}", bin.display())),
            Err(error) => report.fail(format!("  CLI:      remove binary: {error}")),
        }
    }
}
