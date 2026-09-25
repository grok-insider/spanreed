use fabrials_model::UsageRecord;

pub(crate) fn list_cost_usd(record: &UsageRecord) -> Option<f64> {
    if record.provider.as_deref() != Some("opencode-go")
        || record.model.as_deref() != Some("deepseek-v4.1-flash")
    {
        return None;
    }
    let days = record.ts_ms.div_euclid(86_400_000);
    let weekday = (days + 3).rem_euclid(7);
    let hour = record.ts_ms.rem_euclid(86_400_000) / 3_600_000;
    let peak = weekday < 5 && ((1..4).contains(&hour) || (6..10).contains(&hour));
    let multiplier = if peak { 2.0 } else { 1.0 };
    let cached = record.cached_input_tokens.min(record.input_tokens);
    let uncached = record.input_tokens - cached;
    Some(
        multiplier
            * (uncached as f64 * 0.15e-6
                + cached as f64 * 0.003e-6
                + record.output_tokens as f64 * 0.60e-6),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(ts_ms: i64) -> UsageRecord {
        UsageRecord {
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
