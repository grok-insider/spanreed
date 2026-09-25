//! Provider rate cards that depend on more than the model name (provider,
//! time of day). The rates are data in `pricing-overlays.json`.

use fabrials_types::HopRecord;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProviderRate {
    provider: String,
    model: String,
    input_cost_per_token: f64,
    #[serde(default)]
    cache_read_input_token_cost: f64,
    output_cost_per_token: f64,
    #[serde(default)]
    peak: Option<PeakWindow>,
}

#[derive(Debug, Clone, Deserialize)]
struct PeakWindow {
    multiplier: f64,
    #[serde(default)]
    weekdays_only: bool,
    /// Half-open UTC hour ranges `[start, end)`.
    hours_utc: Vec<(i64, i64)>,
}

impl PeakWindow {
    fn applies(&self, ts_ms: i64) -> bool {
        let days = ts_ms.div_euclid(86_400_000);
        let weekday = (days + 3).rem_euclid(7);
        let hour = ts_ms.rem_euclid(86_400_000) / 3_600_000;
        (!self.weekdays_only || weekday < 5)
            && self
                .hours_utc
                .iter()
                .any(|(start, end)| (*start..*end).contains(&hour))
    }
}

impl ProviderRate {
    pub(crate) fn cost(&self, record: &HopRecord) -> Option<f64> {
        if record.provider.as_deref() != Some(self.provider.as_str())
            || record.model.as_deref() != Some(self.model.as_str())
        {
            return None;
        }
        let multiplier = match &self.peak {
            Some(peak) if peak.applies(record.ts_ms) => peak.multiplier,
            _ => 1.0,
        };
        let cached = record.cached_input_tokens.min(record.input_tokens);
        let uncached = record.input_tokens - cached;
        Some(
            multiplier
                * (uncached as f64 * self.input_cost_per_token
                    + cached as f64 * self.cache_read_input_token_cost
                    + record.output_tokens as f64 * self.output_cost_per_token),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_cost_usd(record: &HopRecord) -> Option<f64> {
        crate::pricing::table().provider_rate_cost(record)
    }

    fn record(ts_ms: i64) -> HopRecord {
        HopRecord {
            provider: Some("opencode-go".into()),
            model: Some("deepseek-v4.1-flash".into()),
            ts_ms,
            input_tokens: 1_000_000,
            cached_input_tokens: 400_000,
            output_tokens: 100_000,
            ..Default::default()
        }
    }

    #[test]
    fn utc_peak_boundaries_and_weekends_use_the_record_timestamp() {
        let monday = 1_789_344_000_000;
        for day in 0..7 {
            for hour in 0..24 {
                let peak = day < 5 && ((1..4).contains(&hour) || (6..10).contains(&hour));
                let expected = 0.1512 * if peak { 2.0 } else { 1.0 };
                for millis in [0, 3_599_999] {
                    let ts = monday + day * 86_400_000 + hour * 3_600_000 + millis;
                    assert!((list_cost_usd(&record(ts)).unwrap() - expected).abs() < 1e-12);
                }
            }
        }
    }

    #[test]
    fn cached_input_is_clamped_and_provider_and_model_are_exact() {
        let mut rec = record(1_789_409_220_431);
        rec.cached_input_tokens = u64::MAX;
        assert!((list_cost_usd(&rec).unwrap() - 0.063).abs() < 1e-12);
        rec.provider = Some("deepseek".into());
        assert!(list_cost_usd(&rec).is_none());
        rec.provider = Some("opencode-go".into());
        rec.model = Some("deepseek-v4.1-flash-unknown".into());
        assert!(list_cost_usd(&rec).is_none());
    }
}
