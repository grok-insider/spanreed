//! Plan economics from hops + quota. No HTTP.

use std::collections::{HashMap, HashSet};

use fabrials_metrics::list_cost_usd;
use fabrials_model::{
    MetricKind, MetricLine, ModelEconomics, ProviderEconomics, ProviderOutput, ShareSnapshot,
    UsageRecord,
};

use crate::snapshot_from_outputs;
use crate::ProviderShareExtras;

/// Minimum pool % for a safe scale-to-100%.
pub const MIN_PCT_SCALE: f64 = 1.0;
/// Month convention: full_week × (30/7).
pub const MONTH_WEEK_FACTOR: f64 = 30.0 / 7.0;
const MAX_MODELS: usize = 8;
const DAY_MS: i64 = 86_400_000;
const WINDOW_DAYS: i64 = 31;

/// Quota + time window used to scale hop totals to a full pool.
#[derive(Debug, Clone, Default)]
pub struct HopsWindow {
    pub pool_pct: Option<f64>,
    pub pool_label: Option<String>,
    pub week_start_ms: Option<i64>,
    pub now_ms: i64,
    /// Hosted/synced ledger is complete for this owner — scale even if the
    /// first hop is not at week origin (unlike a half-machine CLI capture).
    pub complete_ledger: bool,
}

/// Scale observed API $ to 100% of pool. None if pct is unsafe.
pub fn scale_to_full_pool(api_usd_obs: f64, pool_pct: f64) -> Option<f64> {
    if !api_usd_obs.is_finite() || api_usd_obs < 0.0 {
        return None;
    }
    if !pool_pct.is_finite() || !(MIN_PCT_SCALE..=100.0).contains(&pool_pct) {
        return None;
    }
    Some(api_usd_obs * (100.0 / pool_pct))
}

pub fn scale_tokens_to_full(tokens_obs: u64, pool_pct: f64) -> Option<u64> {
    if !(MIN_PCT_SCALE..=100.0).contains(&pool_pct) {
        return None;
    }
    Some(((tokens_obs as f64) * (100.0 / pool_pct)).round() as u64)
}

fn dedup_ok(records: &[UsageRecord]) -> Vec<&UsageRecord> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for r in records {
        if r.is_failed() {
            continue;
        }
        if let Some(id) = r.request_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            if !seen.insert(id.to_string()) {
                continue;
            }
        }
        out.push(r);
    }
    out
}

fn sum_window(recs: &[&UsageRecord], from_ms: i64, to_ms: i64) -> (u64, f64, Vec<ModelEconomics>) {
    let mut tokens: u64 = 0;
    let mut usd: f64 = 0.0;
    let mut by: HashMap<String, (u64, f64)> = HashMap::new();
    for r in recs {
        if r.ts_ms < from_ms || r.ts_ms > to_ms {
            continue;
        }
        let tok = r.tokens_for_total();
        let cost = list_cost_usd(r).unwrap_or(0.0);
        tokens = tokens.saturating_add(tok);
        usd += cost;
        let name = r
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown")
            .to_string();
        let e = by.entry(name).or_default();
        e.0 = e.0.saturating_add(tok);
        e.1 += cost;
    }
    let mut models: Vec<ModelEconomics> = by
        .into_iter()
        .map(|(model, (tokens, api_usd_list))| ModelEconomics {
            model,
            tokens,
            api_usd_list,
        })
        .collect();
    models.sort_by(|a, b| b.tokens.cmp(&a.tokens).then(a.model.cmp(&b.model)));
    models.truncate(MAX_MODELS);
    (tokens, usd, models)
}

/// Build economics from official usage hops + current pool %.
///
/// Uses list-price per hop (never SuperGrok ticks). Failed hops are skipped.
/// When the first hop is not near `week_start_ms`, does not scale to 100%.
pub fn economics_from_hops(records: &[UsageRecord], w: &HopsWindow) -> Option<ProviderEconomics> {
    let recs = dedup_ok(records);
    let now = if w.now_ms > 0 {
        w.now_ms
    } else {
        recs.iter().map(|r| r.ts_ms).max().unwrap_or(0)
    };
    let start_30 = now.saturating_sub(WINDOW_DAYS * DAY_MS);
    let (tok_30, usd_30, models_30) = sum_window(&recs, start_30, now.saturating_add(DAY_MS));

    let (tok_obs, usd_obs, models, observed_window, partial) = if let Some(week_start) = w.week_start_ms {
        let (t, u, m) = sum_window(&recs, week_start, now.saturating_add(DAY_MS));
        let first = recs
            .iter()
            .filter(|r| r.ts_ms >= week_start)
            .map(|r| r.ts_ms)
            .min();
        let late = !w.complete_ledger
            && first.is_some_and(|f| f > week_start.saturating_add(DAY_MS));
        if t > 0 || u > 0.0 {
            (t, u, m, Some("since_weekly_reset".into()), late)
        } else {
            (
                tok_30,
                usd_30,
                models_30,
                Some("last_30d".into()),
                true,
            )
        }
    } else {
        (tok_30, usd_30, models_30, Some("last_30d".into()), false)
    };

    let mut full_week_api = None;
    let mut full_week_tok = None;
    let mut method = None;
    let weekly_aligned = observed_window.as_deref() == Some("since_weekly_reset");
    if weekly_aligned && !partial {
        if let Some(pct) = w.pool_pct {
            full_week_api = scale_to_full_pool(usd_obs, pct);
            full_week_tok = scale_tokens_to_full(tok_obs, pct);
            if full_week_api.is_some() {
                method = Some("scale_by_pool_pct".into());
            }
        }
    } else if weekly_aligned && partial {
        method = Some("incomplete_window_no_scale".into());
    }

    let full_month = full_week_api.map(|v| v * MONTH_WEEK_FACTOR);

    if full_week_api.is_none() && usd_obs <= 0.0 && usd_30 <= 0.0 && w.pool_pct.is_none() {
        return None;
    }

    Some(ProviderEconomics {
        pool_label: w.pool_label.clone(),
        pool_pct: w.pool_pct,
        pool_pct_at_start: None,
        partial_observation: partial,
        tokens_obs: (tok_obs > 0).then_some(tok_obs),
        api_usd_obs: (usd_obs > 0.0).then_some(usd_obs),
        observed_window,
        api_usd_30d: (usd_30 > 0.0).then_some(usd_30),
        tokens_30d: (tok_30 > 0).then_some(tok_30),
        full_week_api_usd: full_week_api,
        full_week_tokens: full_week_tok,
        full_month_api_usd: full_month,
        full_pool_method: method,
        by_model: models,
    })
}

/// One-provider snapshot (typically Grok) from quota + hops.
pub fn snapshot_from_hops(
    provider_id: &str,
    plan: Option<String>,
    pool_pct: Option<f64>,
    pool_label: Option<&str>,
    resets_at: Option<String>,
    records: &[UsageRecord],
    week_start_ms: Option<i64>,
    now_ms: i64,
    app: &str,
    version: &str,
    captured_at: &str,
) -> ShareSnapshot {
    let mut lines = Vec::new();
    if let Some(pct) = pool_pct {
        lines.push(MetricLine::percent(
            pool_label.unwrap_or("Weekly"),
            pct,
            resets_at,
        ));
    } else {
        lines.push(MetricLine::text(
            MetricKind::Quota,
            pool_label.unwrap_or("Weekly"),
            "unknown",
        ));
    }
    let out = ProviderOutput::new(provider_id, provider_id, lines).with_plan(plan);
    let window = HopsWindow {
        pool_pct,
        pool_label: pool_label.map(|s| s.to_string()),
        week_start_ms,
        now_ms,
        complete_ledger: true,
    };
    let economics = economics_from_hops(records, &window);
    snapshot_from_outputs(
        &[out],
        app,
        version,
        captured_at,
        &[ProviderShareExtras {
            economics,
            resets: vec![],
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(model: &str, inn: u64, out: u64, ts: i64, id: &str, status: Option<u16>) -> UsageRecord {
        UsageRecord {
            ts_ms: ts,
            model: Some(model.into()),
            input_tokens: inn,
            output_tokens: out,
            total_tokens: inn + out,
            request_id: Some(id.into()),
            status,
            ..UsageRecord::default()
        }
    }

    #[test]
    fn scale_rejects_zero_pct() {
        assert!(scale_to_full_pool(10.0, 0.0).is_none());
        assert!(scale_to_full_pool(10.0, 0.5).is_none());
    }

    #[test]
    fn scale_doubles_at_50_pct() {
        assert!((scale_to_full_pool(10.0, 50.0).unwrap() - 20.0).abs() < 1e-9);
    }

    #[test]
    fn hops_price_with_real_table_skips_failed_and_dups() {
        let now = 1_700_000_000_000;
        let week = now - 12 * 3_600_000;
        let ok = rec("grok-4.5", 1_000_000, 0, now - 1_000, "r1", Some(200));
        let dup = rec("grok-4.5", 1_000_000, 0, now - 900, "r1", Some(200));
        let fail = rec("grok-4.5", 9_000_000, 0, now - 800, "r2", Some(502));
        let priced = list_cost_usd(&ok).expect("grok-4.5 in price table");
        assert!(priced > 0.0, "list price must be positive, got {priced}");

        let e = economics_from_hops(
            &[ok.clone(), dup, fail],
            &HopsWindow {
                pool_pct: Some(50.0),
                pool_label: Some("Weekly".into()),
                week_start_ms: Some(week),
                now_ms: now,
                complete_ledger: false,
            },
        )
        .unwrap();
        assert_eq!(e.tokens_obs, Some(1_000_000));
        assert!((e.api_usd_obs.unwrap() - priced).abs() < 1e-12);
        assert!(!e.partial_observation);
        assert_eq!(e.full_pool_method.as_deref(), Some("scale_by_pool_pct"));
        assert!((e.full_week_api_usd.unwrap() - priced * 2.0).abs() < 1e-9);
        assert_eq!(e.by_model.len(), 1);
        assert_eq!(e.by_model[0].model, "grok-4.5");
        assert_eq!(e.by_model[0].tokens, 1_000_000);
    }

    #[test]
    fn late_first_hop_does_not_scale() {
        let now = 1_700_000_000_000;
        let week = now - 8 * DAY_MS;
        let hop = rec("grok-4.5", 100_000, 0, now - 1_000, "late", Some(200));
        let e = economics_from_hops(
            &[hop],
            &HopsWindow {
                pool_pct: Some(80.0),
                pool_label: Some("Weekly".into()),
                week_start_ms: Some(week),
                now_ms: now,
                complete_ledger: false,
            },
        )
        .unwrap();
        assert!(e.partial_observation);
        assert_eq!(
            e.full_pool_method.as_deref(),
            Some("incomplete_window_no_scale")
        );
        assert!(e.full_week_api_usd.is_none());
    }

    #[test]
    fn complete_ledger_scales_late_first_hop() {
        let now = 1_700_000_000_000;
        let week = now - 8 * DAY_MS;
        let hop = rec("grok-4.5", 1_000_000, 0, now - 1_000, "late", Some(200));
        let priced = list_cost_usd(&hop).unwrap();
        let e = economics_from_hops(
            &[hop],
            &HopsWindow {
                pool_pct: Some(50.0),
                pool_label: Some("Weekly".into()),
                week_start_ms: Some(week),
                now_ms: now,
                complete_ledger: true,
            },
        )
        .unwrap();
        assert!(!e.partial_observation);
        assert_eq!(e.full_pool_method.as_deref(), Some("scale_by_pool_pct"));
        assert!((e.full_week_api_usd.unwrap() - priced * 2.0).abs() < 1e-9);
        assert!((e.full_month_api_usd.unwrap() - priced * 2.0 * MONTH_WEEK_FACTOR).abs() < 1e-9);
    }

    #[test]
    fn snapshot_from_hops_is_v2_grok_no_secrets() {
        let now = 1_700_000_000_000;
        let hop = rec("grok-4.5", 10_000, 100, now, "a", Some(200));
        let snap = snapshot_from_hops(
            "grok",
            Some("SuperGrok".into()),
            Some(22.0),
            Some("Weekly"),
            None,
            &[hop],
            Some(now - DAY_MS),
            now,
            "ai-relay",
            "0.1.0",
            "2026-08-21T00:00:00Z",
        );
        assert_eq!(snap.source.app, "ai-relay");
        assert_eq!(snap.schema_version, 2);
        assert_eq!(snap.providers[0].id, "grok");
        assert_eq!(snap.providers[0].plan.as_deref(), Some("SuperGrok"));
        assert!(snap.providers[0].economics.is_some());
        let j = serde_json::to_string(&snap).unwrap();
        for bad in ["access_token", "refresh_token", "api_key", "cookie"] {
            assert!(!j.contains(bad), "leaked {bad}");
        }
    }
}
