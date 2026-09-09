//! Probe orchestration: run providers concurrently.

use std::thread;

use crate::model::ProviderOutput;
use crate::providers;

/// Probe every provider that is detected on this machine, in parallel.
pub fn probe_detected() -> Vec<ProviderOutput> {
    probe_filtered(|p| p.detect())
}

/// Probe all providers regardless of detection (used by `probe <id> --force`).
pub fn probe_all() -> Vec<ProviderOutput> {
    probe_filtered(|_| true)
}

/// Probe a single provider by id (forced).
pub fn probe_one(id: &str) -> Option<ProviderOutput> {
    let mut out = providers::by_id(id)
        .map(|p| p.probe())
        .or_else(|| {
            crate::drivers::grok::probe_accounts()
                .into_iter()
                .find(|o| o.provider_id == id)
        })
        .or_else(|| {
            crate::drivers::codex::probe_accounts()
                .into_iter()
                .find(|output| output.provider_id == id)
        })
        .or_else(|| crate::addons::host::probe_extra_one(id));
    if let Some(o) = &mut out {
        enrich_local_usage(o);
        crate::pool_baseline::note_from_output(o);
        crate::epoch::note_jumps_from_outputs(std::slice::from_ref(o));
        crate::sync::after_probe(std::slice::from_ref(o));
    }
    out
}

fn probe_filtered<F>(filter: F) -> Vec<ProviderOutput>
where
    F: Fn(&dyn providers::Provider) -> bool,
{
    let selected: Vec<_> = providers::all()
        .into_iter()
        .filter(|p| filter(p.as_ref()))
        .collect();

    // Each provider runs on its own thread; provider probes are blocking I/O.
    let handles: Vec<_> = selected
        .into_iter()
        .map(|p| thread::spawn(move || p.probe()))
        .collect();

    let mut outs: Vec<_> = handles.into_iter().filter_map(|h| h.join().ok()).collect();
    outs.extend(crate::drivers::grok::probe_accounts());
    outs.extend(crate::drivers::codex::probe_accounts());
    outs.extend(crate::addons::extra_detected_outputs());
    for o in &mut outs {
        enrich_local_usage(o);
        crate::pool_baseline::note_from_output(o);
    }
    crate::epoch::note_jumps_from_outputs(&outs);
    crate::sync::after_probe(&outs);
    outs
}

fn enrich_local_usage(output: &mut ProviderOutput) {
    if output
        .lines
        .iter()
        .any(|line| line.kind() == crate::model::MetricKind::Cost)
    {
        return;
    }
    let source = match output.provider_id.as_str() {
        "codex" => crate::cost::Source::Codex,
        "claude" => crate::cost::Source::Claude,
        _ => return,
    };
    output.lines.extend(crate::cost::cost_lines(source, None));
}
