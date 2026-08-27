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
    let out = providers::by_id(id).map(|p| p.probe());
    if let Some(o) = &out {
        crate::pool_baseline::note_from_output(o);
        crate::epoch::note_jumps_from_outputs(std::slice::from_ref(o));
        crate::sync::after_probe();
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

    let outs: Vec<_> = handles.into_iter().filter_map(|h| h.join().ok()).collect();
    for o in &outs {
        crate::pool_baseline::note_from_output(o);
    }
    crate::epoch::note_jumps_from_outputs(&outs);
    crate::sync::after_probe();
    outs
}
