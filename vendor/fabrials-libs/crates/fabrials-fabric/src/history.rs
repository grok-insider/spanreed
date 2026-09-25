//! Quota history sample policy. Persistence lives behind [`crate::ports::HistoryStore`].
pub use fabrials_types::HistorySample;

/// Whether `curr` should be treated as a new rate-limit epoch vs `prev`.
pub fn is_reset_event(prev: &HistorySample, curr: &HistorySample) -> bool {
    matches!(
        (&prev.resets_at, &curr.resets_at),
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() && a != b
    )
}

/// Whether we should skip writing `curr` because nothing material changed.
pub fn is_duplicate(prev: &HistorySample, curr: &HistorySample) -> bool {
    if is_reset_event(prev, curr) {
        return false;
    }
    same_used(prev.used, curr.used) && prev.resets_at == curr.resets_at
}

fn same_used(a: f64, b: f64) -> bool {
    (a * 10.0).round() == (b * 10.0).round()
}

/// Apply reset event flag and decide write eligibility against last known sample.
pub fn prepare_sample(
    prev: Option<&HistorySample>,
    mut curr: HistorySample,
) -> Option<HistorySample> {
    if let Some(p) = prev {
        if is_duplicate(p, &curr) {
            return None;
        }
        if is_reset_event(p, &curr) {
            curr.event = Some("reset".into());
        }
    }
    Some(curr)
}
