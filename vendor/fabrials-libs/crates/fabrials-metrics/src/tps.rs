//! Output tokens per second for one chat hop, matching OpenCode's session footer.
//!
//! The clock is the hop wall time (`duration_ms`), which already excludes client
//! tool calls. `output_tokens` is the provider total and already includes
//! reasoning, so `reasoning_tokens` is not added again.

use fabrials_model::UsageRecord;

/// Pooled output tokens and model-active milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutputRate {
    pub tokens: u64,
    pub duration_ms: u64,
}

impl OutputRate {
    pub fn observe(&mut self, record: &UsageRecord) {
        let Some(duration_ms) = chat_duration_ms(record) else {
            return;
        };
        self.tokens = self.tokens.saturating_add(record.output_tokens);
        self.duration_ms = self.duration_ms.saturating_add(duration_ms);
    }

    pub fn tps(&self) -> Option<f64> {
        pooled_output_tps(self.tokens, self.duration_ms)
    }
}

/// Tokens per second for one qualifying chat hop.
pub fn output_tps(record: &UsageRecord) -> Option<f64> {
    let duration_ms = chat_duration_ms(record)?;
    pooled_output_tps(record.output_tokens, duration_ms)
}

pub fn pooled_output_tps(tokens: u64, duration_ms: u64) -> Option<f64> {
    if tokens == 0 || duration_ms == 0 {
        return None;
    }
    Some(tokens as f64 / (duration_ms as f64 / 1000.0))
}

fn chat_duration_ms(record: &UsageRecord) -> Option<u64> {
    if record.is_failed() || record.kind.as_deref() != Some("chat") || record.output_tokens == 0 {
        return None;
    }
    record.duration_ms.filter(|duration| *duration > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(output: u64, duration_ms: u64, reasoning: u64) -> UsageRecord {
        UsageRecord {
            kind: Some("chat".into()),
            status: Some(200),
            output_tokens: output,
            reasoning_tokens: reasoning,
            duration_ms: Some(duration_ms),
            ..UsageRecord::default()
        }
    }

    #[test]
    fn one_second_of_generation_is_the_output_count() {
        assert_eq!(output_tps(&chat(1_000, 4_000, 80)), Some(250.0));
    }

    #[test]
    fn missing_clock_failure_and_non_chat_are_blank() {
        let mut bare = chat(10, 1_000, 0);
        bare.duration_ms = None;
        assert_eq!(output_tps(&bare), None);
        bare.duration_ms = Some(0);
        assert_eq!(output_tps(&bare), None);
        bare.duration_ms = Some(1_000);
        bare.status = Some(500);
        assert_eq!(output_tps(&bare), None);
        bare.status = Some(200);
        bare.kind = Some("stt".into());
        assert_eq!(output_tps(&bare), None);
        bare.kind = Some("chat".into());
        bare.output_tokens = 0;
        assert_eq!(output_tps(&bare), None);
    }

    #[test]
    fn pooled_rate_weights_by_tokens_not_by_hops() {
        let mut rate = OutputRate::default();
        rate.observe(&chat(100, 1_000, 7));
        rate.observe(&chat(100, 4_000, 9));
        assert_eq!(rate.tokens, 200);
        assert_eq!(rate.tps(), Some(40.0));
        rate.observe(&chat(1_000, 1_000, 0));
        rate.observe(&UsageRecord {
            kind: Some("chat".into()),
            status: Some(500),
            output_tokens: 9_000,
            duration_ms: Some(1_000),
            ..UsageRecord::default()
        });
        assert_eq!(rate.tps(), Some(200.0));
    }
}
