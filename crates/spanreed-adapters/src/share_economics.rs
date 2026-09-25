//! Plan economics for community share. The estimation math lives in
//! `fabrials_share::probed`; this module supplies Spanreed's pool baseline and
//! its per-model breakdown (ledger and local logs).
//!
//! Primary goal: estimate **API list-price $ if rate-limit pools run at 100%**
//! for a week (and month ≈ week × 30/7). Multi-model mixes are valued as
//! Σ tokens_i × price_i (already done by local cost engines); we never pick
//! a single Sol/Terra/Luna price on the server/web.

use crate::model::ProviderOutput;

pub use fabrials_types::{ModelEconomics, ProviderEconomics};

/// Plan economics for one probed output; the pool baseline is Spanreed's
/// first-seen weekly % for that week, else the earliest forecast sample.
pub fn from_output(o: &ProviderOutput, by_model: Vec<ModelEconomics>) -> Option<ProviderEconomics> {
    fabrials_share::probed::from_output(o, by_model, &first_weekly_pct)
}

fn first_weekly_pct(o: &ProviderOutput) -> Option<f64> {
    crate::pool_baseline::week_and_pct(o)
        .and_then(|(week, _)| crate::pool_baseline::baseline_pct(&o.provider_id, &week))
        .or_else(|| crate::forecast::earliest_weekly_pct(&o.provider_id))
}

/// Model breakdown for Grok (ledger) or Codex/Claude (log cost).
pub fn model_breakdown_for(
    provider_id: &str,
    pricing: &crate::pricing::PricingMap,
) -> Vec<ModelEconomics> {
    match provider_id {
        "grok" => grok_models(pricing),
        "codex" => log_cost_models(crate::cost::Source::Codex, pricing),
        "claude" => log_cost_models(crate::cost::Source::Claude, pricing),
        _ => Vec::new(),
    }
}

fn grok_models(pricing: &crate::pricing::PricingMap) -> Vec<ModelEconomics> {
    let now = crate::util::now_ms();
    let recs = crate::grok_ledger::read_window(now);
    let mut map: std::collections::HashMap<String, (u64, f64)> = std::collections::HashMap::new();
    for r in recs {
        let tok = r.tokens_for_total();
        let cost = crate::pricing::hop_cost_usd(&r, pricing).unwrap_or(0.0);
        let name = r
            .model
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".into());
        let e = map.entry(name).or_default();
        e.0 = e.0.saturating_add(tok);
        e.1 += cost;
    }
    let mut v: Vec<_> = map
        .into_iter()
        .map(|(model, (tokens, api_usd_list))| ModelEconomics {
            model,
            tokens,
            api_usd_list,
        })
        .collect();
    v.sort_by_key(|b| std::cmp::Reverse(b.tokens));
    v.truncate(8);
    v
}

fn log_cost_models(
    source: crate::cost::Source,
    pricing: &crate::pricing::PricingMap,
) -> Vec<ModelEconomics> {
    let Some(sum) = crate::cost::estimate(source, pricing) else {
        return Vec::new();
    };
    sum.by_model
        .into_iter()
        .take(8)
        .map(|m| ModelEconomics {
            model: m.model,
            tokens: m.tokens,
            api_usd_list: m.cost,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MetricLine;

    #[test]
    fn mid_week_start_does_not_oneshot() {
        let o = ProviderOutput::new(
            "grok",
            "Grok",
            vec![
                MetricLine::percent("Weekly", 81.0, None),
                MetricLine::text(
                    crate::model::MetricKind::Cost,
                    "Since weekly reset",
                    "$20.00 · 10M tokens",
                ),
            ],
        );
        let e = from_output(&o, vec![]).unwrap();
        assert_eq!(e.pool_pct, Some(81.0));
        assert!((e.api_usd_obs.unwrap() - 20.0).abs() < 0.01);
        // No first sample near 0% → do not claim $20/0.81 ≈ $25 as full week.
        assert!(e.full_week_api_usd.is_none());
        assert_eq!(
            e.full_pool_method.as_deref(),
            Some("incomplete_window_no_scale")
        );
    }

    #[test]
    fn multi_model_not_single_price() {
        // Sol-heavy mix must not equal Luna-only pricing of same tokens.
        let mix = [
            ModelEconomics {
                model: "luna".into(),
                tokens: 10_000_000,
                api_usd_list: 1.0,
            },
            ModelEconomics {
                model: "sol".into(),
                tokens: 2_000_000,
                api_usd_list: 20.0,
            },
        ];
        let sum: f64 = mix.iter().map(|m| m.api_usd_list).sum();
        assert!((sum - 21.0).abs() < 1e-9);
        // Fake single-model Luna would be ~$1.2 for 12M at cheap rate — far from 21
        assert!(sum > 10.0);
    }
}
