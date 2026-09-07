//! Public list-price USD for a [`fabrials_model::UsageRecord`].
//!
//! xAI long-context rule: when prompt tokens ≥ 200k, **all** token types in
//! the request use the higher rate (not progressive Anthropic-style tiers).

use fabrials_model::UsageRecord;

use crate::pricing;

/// Public API list-price USD for this record, or None if the model is unknown.
pub fn list_cost_usd(record: &UsageRecord) -> Option<f64> {
    list_cost_usd_with(record, pricing::table())
}

pub fn list_cost_usd_with(record: &UsageRecord, table: &pricing::PricingMap) -> Option<f64> {
    if record.is_failed() {
        return None;
    }
    if let Some(usd) = media_cost(record, table) {
        return Some(usd);
    }
    let model = record
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let p = table.find(model)?;
    let cached = record.cached_input_tokens.min(record.input_tokens);
    let uncached = record.input_tokens.saturating_sub(cached);
    const TIER: u64 = 200_000;
    let long = record.input_tokens >= TIER;
    let (rin, rcache, rout) = if long {
        (
            p.input_above_200k.unwrap_or(p.input),
            p.cache_read_above_200k.unwrap_or(p.cache_read),
            p.output_above_200k.unwrap_or(p.output),
        )
    } else {
        (p.input, p.cache_read, p.output)
    };
    Some(uncached as f64 * rin + cached as f64 * rcache + record.output_tokens as f64 * rout)
}

fn voice_billing_key(record: &UsageRecord) -> Option<&'static str> {
    match record.kind.as_deref().map(str::trim).unwrap_or("") {
        "tts" => Some("tts"),
        "realtime" => Some("realtime"),
        "stt" => match record.model.as_deref().map(str::trim).unwrap_or("") {
            "stt-batch" => Some("stt-batch"),
            _ => Some("stt"),
        },
        "" => match record.model.as_deref().map(str::trim).unwrap_or("") {
            "tts" | "grok-tts" => Some("tts"),
            "stt" | "grok-stt" => Some("stt"),
            "stt-batch" => Some("stt-batch"),
            "realtime" | "grok-realtime" => Some("realtime"),
            _ => None,
        },
        _ => None,
    }
}

fn media_cost(record: &UsageRecord, table: &pricing::PricingMap) -> Option<f64> {
    match record.kind.as_deref().map(str::trim).unwrap_or("") {
        "image" => {
            let model = record.model.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
            let p = table.get(model)?;
            let n = record.quantity.unwrap_or(1).max(1);
            return Some(n as f64 * p.cost_per_image?);
        }
        "video" => {
            let model = record.model.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
            let p = table.get(model)?;
            let ms = match record.unit.as_deref() {
                Some("video_ms") => record.quantity.unwrap_or(0),
                _ => record.duration_ms.unwrap_or(0),
            };
            return Some((ms as f64 / 1000.0) * p.cost_per_second?);
        }
        _ => {}
    }
    voice_cost(record, table)
}

fn voice_cost(record: &UsageRecord, table: &pricing::PricingMap) -> Option<f64> {
    let key = voice_billing_key(record)?;
    let p = table.get(key)?;
    if let Some(per_char) = p.cost_per_character {
        let chars = match record.unit.as_deref() {
            Some("chars") => record.quantity.unwrap_or(0),
            _ => record.input_tokens,
        };
        return Some(chars as f64 * per_char);
    }
    if let Some(per_sec) = p.cost_per_second {
        let ms = match record.unit.as_deref() {
            Some("audio_ms") | Some("video_ms") => record.quantity.unwrap_or(0),
            _ => record.duration_ms.unwrap_or(0),
        };
        return Some((ms as f64 / 1000.0) * per_sec);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabrials_model::UsageRecord;

    #[test]
    fn grok_list_price_splits_cached_input() {
        let rec = UsageRecord {
            model: Some("grok-4.5".into()),
            input_tokens: 100,
            output_tokens: 20,
            cached_input_tokens: 40,
            total_tokens: 120,
            ..UsageRecord::default()
        };
        let list = list_cost_usd(&rec).expect("priced");
        let expected = 60.0 * 2e-6 + 40.0 * 3e-7 + 20.0 * 6e-6;
        assert!(
            (list - expected).abs() < 1e-12,
            "list={list} expected={expected}"
        );
    }

    #[test]
    fn grok_long_context_all_or_nothing() {
        let rec = UsageRecord {
            model: Some("grok-4.5".into()),
            input_tokens: 200_000,
            output_tokens: 1_000,
            cached_input_tokens: 100_000,
            total_tokens: 201_000,
            ..UsageRecord::default()
        };
        let expected = 100_000.0 * 4e-6 + 100_000.0 * 6e-7 + 1_000.0 * 1.2e-5;
        let got = list_cost_usd(&rec).unwrap();
        assert!(
            (got - expected).abs() < 1e-9,
            "got={got} expected={expected}"
        );
    }

    #[test]
    fn grok_46_and_build_use_official_list_price() {
        fn close(got: Option<f64>, expected: f64) {
            let got = got.expect("priced");
            assert!(
                (got - expected).abs() < 1e-12,
                "got={got} expected={expected}"
            );
        }
        // 100k stays under the 200k all-or-nothing long-context tier.
        let inn = UsageRecord {
            model: Some("grok-4.6".into()),
            input_tokens: 100_000,
            output_tokens: 0,
            cached_input_tokens: 0,
            ..UsageRecord::default()
        };
        close(list_cost_usd(&inn), 100_000.0 * 2e-6);
        let build = UsageRecord {
            model: Some("grok-4.6-build".into()),
            input_tokens: 100_000,
            ..UsageRecord::default()
        };
        close(list_cost_usd(&build), 100_000.0 * 2e-6);
        let cached = UsageRecord {
            model: Some("grok-4.6".into()),
            input_tokens: 100_000,
            cached_input_tokens: 100_000,
            ..UsageRecord::default()
        };
        close(list_cost_usd(&cached), 100_000.0 * 5e-7);
        let four_five_cached = UsageRecord {
            model: Some("grok-4.5".into()),
            input_tokens: 100_000,
            cached_input_tokens: 100_000,
            ..UsageRecord::default()
        };
        close(list_cost_usd(&four_five_cached), 100_000.0 * 3e-7);
    }

    #[test]
    fn unknown_model_is_none() {
        let rec = UsageRecord {
            model: Some("not-a-real-model-xyz".into()),
            input_tokens: 10,
            ..UsageRecord::default()
        };
        assert!(list_cost_usd(&rec).is_none());
    }

    fn close(got: Option<f64>, expected: f64) {
        let got = got.expect("priced");
        assert!(
            (got - expected).abs() < 1e-12,
            "got={got} expected={expected}"
        );
    }

    #[test]
    fn tts_million_chars_is_fifteen_dollars() {
        let rec = UsageRecord {
            kind: Some("tts".into()),
            model: Some("tts".into()),
            unit: Some("chars".into()),
            quantity: Some(1_000_000),
            ..UsageRecord::default()
        };
        close(list_cost_usd(&rec), 15.0);
    }

    #[test]
    fn legacy_tts_without_unit_uses_input_tokens_as_chars() {
        let rec = UsageRecord {
            kind: Some("tts".into()),
            model: Some("tts".into()),
            input_tokens: 1_000_000,
            ..UsageRecord::default()
        };
        close(list_cost_usd(&rec), 15.0);
    }

    #[test]
    fn failed_hop_is_not_priced() {
        let rec = UsageRecord {
            kind: Some("tts".into()),
            model: Some("tts".into()),
            unit: Some("chars".into()),
            quantity: Some(1_000_000),
            status: Some(502),
            ..UsageRecord::default()
        };
        assert!(list_cost_usd(&rec).is_none());
    }

    #[test]
    fn stt_stream_hour_is_twenty_cents() {
        let rec = UsageRecord {
            kind: Some("stt".into()),
            model: Some("stt".into()),
            unit: Some("audio_ms".into()),
            quantity: Some(3_600_000),
            duration_ms: Some(50),
            ..UsageRecord::default()
        };
        close(list_cost_usd(&rec), 0.20);
    }

    #[test]
    fn stt_batch_hour_is_ten_cents() {
        let rec = UsageRecord {
            kind: Some("stt".into()),
            model: Some("stt-batch".into()),
            unit: Some("audio_ms".into()),
            quantity: Some(3_600_000),
            duration_ms: Some(12_000),
            ..UsageRecord::default()
        };
        close(list_cost_usd(&rec), 0.10);
    }

    #[test]
    fn imagine_quality_1k_is_five_cents() {
        let rec = UsageRecord {
            kind: Some("image".into()),
            model: Some("grok-imagine-image-quality".into()),
            unit: Some("images".into()),
            quantity: Some(1),
            ..UsageRecord::default()
        };
        close(list_cost_usd(&rec), 0.05);
    }

    #[test]
    fn imagine_video_1s_720p_is_list_price() {
        let rec = UsageRecord {
            kind: Some("video".into()),
            model: Some("grok-imagine-video-1.5".into()),
            unit: Some("video_ms".into()),
            quantity: Some(1_000),
            ..UsageRecord::default()
        };
        close(list_cost_usd(&rec), 0.14);
    }

    #[test]
    fn realtime_minute_is_five_cents() {
        let rec = UsageRecord {
            kind: Some("realtime".into()),
            model: Some("realtime".into()),
            unit: Some("audio_ms".into()),
            quantity: Some(60_000),
            ..UsageRecord::default()
        };
        close(list_cost_usd(&rec), 0.05);
    }
}
