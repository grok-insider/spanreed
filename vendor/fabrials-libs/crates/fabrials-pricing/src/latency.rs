//! Time to first token across chat hops.
//!
//! Only a hop whose relay saw a streamed content event carries `ttft_ms`. A hop without one
//! is not fast, it is unmeasured, so it is left out of every statistic instead of counting
//! as zero. The clock is the hop's own (`HopRecord::ttft_ms` shares its origin with
//! `duration_ms`), so retries are included: this is the wait the client saw.

use fabrials_types::HopRecord;

/// Below this many samples a p95 is just the slowest hop, so none is reported.
pub const MIN_SAMPLES_FOR_P95: usize = 20;

/// The time to first token of one chat hop that has a trustworthy one: a successful chat hop
/// whose first content arrived no later than the hop ended.
pub fn chat_ttft_ms(record: &HopRecord) -> Option<u64> {
    if record.is_failed() || record.kind.as_deref() != Some("chat") {
        return None;
    }
    let (ttft, duration) = (record.ttft_ms?, record.duration_ms?);
    (ttft <= duration).then_some(ttft)
}

/// Time-to-first-token samples of one group of hops (a model, a key). The caller bounds how
/// many hops it feeds in; the accumulator keeps one `u64` per qualifying hop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TtftSamples {
    values: Vec<u64>,
}

impl TtftSamples {
    pub fn observe(&mut self, record: &HopRecord) {
        if let Some(ttft) = chat_ttft_ms(record) {
            self.values.push(ttft);
        }
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn p50(&self) -> Option<u64> {
        self.percentile(50)
    }

    /// `None` until there are [`MIN_SAMPLES_FOR_P95`] samples.
    pub fn p95(&self) -> Option<u64> {
        if self.values.len() < MIN_SAMPLES_FOR_P95 {
            return None;
        }
        self.percentile(95)
    }

    /// Nearest-rank percentile: the value at 1-based rank `ceil(p/100 * n)` of the sorted
    /// samples. It always returns a value that was observed.
    fn percentile(&self, percent: usize) -> Option<u64> {
        if self.values.is_empty() {
            return None;
        }
        let mut sorted = self.values.clone();
        sorted.sort_unstable();
        let rank = (percent * sorted.len()).div_ceil(100).max(1);
        sorted.get(rank - 1).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hop(ttft: Option<u64>, duration: Option<u64>, status: Option<u16>, kind: &str) -> HopRecord {
        HopRecord {
            ttft_ms: ttft,
            duration_ms: duration,
            status,
            kind: Some(kind.into()),
            ..HopRecord::default()
        }
    }

    fn chat(ttft: u64) -> HopRecord {
        hop(Some(ttft), Some(ttft + 1_000), Some(200), "chat")
    }

    fn samples(values: impl IntoIterator<Item = u64>) -> TtftSamples {
        let mut samples = TtftSamples::default();
        for value in values {
            samples.observe(&chat(value));
        }
        samples
    }

    #[test]
    fn nearest_rank_returns_an_observed_value() {
        assert_eq!(samples([]).p50(), None);
        assert_eq!(samples([700]).p50(), Some(700));
        // Rank ceil(0.5 * 4) = 2 of the sorted samples.
        assert_eq!(samples([400, 100, 300, 200]).p50(), Some(200));
        // Rank ceil(0.5 * 5) = 3.
        assert_eq!(samples([500, 100, 400, 200, 300]).p50(), Some(300));
    }

    #[test]
    fn p95_waits_for_enough_samples_and_then_ignores_the_order_they_came_in() {
        assert_eq!(samples(1..=19).p95(), None);
        assert_eq!(samples(1..=20).p95(), Some(19), "rank ceil(0.95 * 20) = 19");
        let shuffled = samples((1..=100).rev());
        assert_eq!(shuffled.p95(), Some(95));
        assert_eq!(shuffled.p50(), Some(50));
        assert_eq!(shuffled.len(), 100);
    }

    #[test]
    fn unmeasured_failed_and_non_chat_hops_are_left_out_not_counted_as_zero() {
        let mut samples = TtftSamples::default();
        for record in [
            hop(None, Some(900), Some(200), "chat"),
            hop(Some(100), None, Some(200), "chat"),
            hop(Some(100), Some(900), Some(499), "chat"),
            hop(Some(100), Some(900), Some(502), "chat"),
            hop(Some(100), Some(900), Some(200), "tts"),
            hop(Some(100), Some(900), Some(200), "stt"),
            // Cannot be ordered against the duration: not trusted.
            hop(Some(901), Some(900), Some(200), "chat"),
        ] {
            samples.observe(&record);
        }
        assert!(samples.is_empty());
        assert_eq!(samples.p50(), None);
        samples.observe(&hop(Some(900), Some(900), Some(200), "chat"));
        samples.observe(&hop(Some(0), Some(5), None, "chat"));
        assert_eq!(samples.len(), 2);
        assert_eq!(samples.p50(), Some(0));
    }

    #[test]
    fn a_hop_with_no_recorded_status_still_counts_when_it_is_not_failed() {
        assert_eq!(
            chat_ttft_ms(&hop(Some(40), Some(90), None, "chat")),
            Some(40)
        );
    }
}
