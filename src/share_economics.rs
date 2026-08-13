//! Build plan economics for community share.
//!
//! Primary goal: estimate **API list-price $ if rate-limit pools run at 100%**
//! for a week (and month ≈ week × 30/7). Multi-model mixes are valued as
//! Σ tokens_i × price_i (already done by local cost engines); we never pick
//! a single Sol/Terra/Luna price on the server/web.

use crate::forecast::{
    density_oneshot, origin_is_near_zero, project_week_to_full, scale_span_to_full,
};
use crate::model::{MetricKind, MetricLine, ProviderOutput};

/// Minimum pool % for safe scale-to-100% (matches forecast oneshot spirit).
pub const MIN_PCT_SCALE: f64 = 1.0;
/// Month convention: full_week × (30/7).
pub const MONTH_WEEK_FACTOR: f64 = 30.0 / 7.0;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ModelEconomics {
    pub model: String,
    pub tokens: u64,
    pub api_usd_list: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ProviderEconomics {
    /// e.g. "Weekly"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_pct: Option<f64>,
    /// Weekly % when this install first saw the provider this week.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_pct_at_start: Option<f64>,
    /// True when local $ likely miss other hosts (or start was mid-pool).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial_observation: bool,
    /// Observed tokens in the aligned window (e.g. since weekly reset).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_obs: Option<u64>,
    /// Observed API list $ in that window (multi-model sum when available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_usd_obs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_window: Option<String>,
    /// Last-30d observed API $ (not a limit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_usd_30d: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_30d: Option<u64>,
    /// Estimated API $ if the primary pool is used at 100% for one week.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_week_api_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_week_tokens: Option<u64>,
    /// full_week × (30/7) convention.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_month_api_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_pool_method: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_model: Vec<ModelEconomics>,
}

/// Scale observed API $ to 100% of pool. Returns None if pct unsafe.
pub fn scale_to_full_pool(api_usd_obs: f64, pool_pct: f64) -> Option<f64> {
    if !api_usd_obs.is_finite() || api_usd_obs < 0.0 {
        return None;
    }
    if !pool_pct.is_finite() || !(MIN_PCT_SCALE..=100.0).contains(&pool_pct) {
        return None;
    }
    Some(api_usd_obs * (100.0 / pool_pct))
}

/// Pure at 100% week estimate. `pct_start` is first-seen pool % this week.
///
/// Dual-PC / half-ledger is not detectable here; callers may set
/// `partial_observation` when density is implausible.
pub fn estimate_full_week(
    obs_usd: f64,
    obs_tokens: Option<u64>,
    pct_now: f64,
    pct_start: Option<f64>,
) -> FullWeekEst {
    let lo = pct_start.unwrap_or(pct_now);
    let from_origin = origin_is_near_zero(lo);
    if let Some(lo) = pct_start.filter(|p| *p + crate::forecast::MIN_PCT_DELTA <= pct_now) {
        if let Some(usd) = scale_span_to_full(obs_usd, lo, pct_now) {
            let d = (pct_now - lo).max(crate::forecast::MIN_PCT_DELTA);
            return FullWeekEst {
                usd: Some(usd),
                tokens: obs_tokens.map(|t| ((t as f64) * (100.0 / d)).round() as u64),
                method: "scale_by_pool_span",
                partial: !from_origin,
            };
        }
    }
    if from_origin {
        if let (Some(tok), Some((tp, cp))) = (
            obs_tokens,
            density_oneshot(obs_tokens.unwrap_or(0), obs_usd, pct_now),
        ) {
            let proj = project_week_to_full(tok, obs_usd, pct_now, tp, cp, pct_now < 5.0);
            return FullWeekEst {
                usd: Some(proj.cost_usd),
                tokens: Some(proj.tokens),
                method: "density_oneshot_to_100",
                partial: false,
            };
        }
        if let Some(v) = scale_to_full_pool(obs_usd, pct_now) {
            return FullWeekEst {
                usd: Some(v),
                tokens: obs_tokens.and_then(|t| scale_tokens_to_full(t, pct_now)),
                method: "scale_by_pool_pct",
                partial: false,
            };
        }
    }
    FullWeekEst {
        usd: None,
        tokens: None,
        method: "incomplete_window_no_scale",
        partial: true,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FullWeekEst {
    pub usd: Option<f64>,
    pub tokens: Option<u64>,
    pub method: &'static str,
    pub partial: bool,
}

pub fn scale_tokens_to_full(tokens_obs: u64, pool_pct: f64) -> Option<u64> {
    if !(MIN_PCT_SCALE..=100.0).contains(&pool_pct) {
        return None;
    }
    Some(((tokens_obs as f64) * (100.0 / pool_pct)).round() as u64)
}

/// Parse "$34.09 · 51M tokens" / "51M tokens · ~$34" / "~794M · ~$569.32".
pub fn parse_usd_and_tokens(value: &str) -> (Option<f64>, Option<u64>) {
    let mut usd = None;
    let mut tokens = None;
    // $1.23 or ~$1.23
    for cap in value.split(['·', '|']) {
        let t = cap.trim();
        if let Some(rest) = t.strip_prefix('~').or(Some(t)) {
            if let Some(s) = rest.strip_prefix('$') {
                let n: f64 = s
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect::<String>()
                    .parse()
                    .unwrap_or(f64::NAN);
                if n.is_finite() {
                    usd = Some(n);
                }
            }
        }
        // 51M tokens or ~794M
        if let Some(idx) = t.to_ascii_lowercase().find('m') {
            let num: String = t[..idx]
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(m) = num.parse::<f64>() {
                if m.is_finite() && m >= 0.0 {
                    tokens = Some((m * 1_000_000.0).round() as u64);
                }
            }
        }
    }
    (usd, tokens)
}

fn weekly_pct(lines: &[MetricLine]) -> Option<(String, f64)> {
    for l in lines {
        if let MetricLine::Progress {
            kind, label, used, ..
        } = l
        {
            if *kind == MetricKind::Quota
                && (label.eq_ignore_ascii_case("Weekly")
                    || label.eq_ignore_ascii_case("5h")
                    || label.eq_ignore_ascii_case("Session"))
            {
                // Prefer Weekly over session/5h when both exist — first Weekly wins later.
                if label.eq_ignore_ascii_case("Weekly") {
                    return Some((label.clone(), *used));
                }
            }
        }
    }
    for l in lines {
        if let MetricLine::Progress {
            kind, label, used, ..
        } = l
        {
            if *kind == MetricKind::Quota && label.eq_ignore_ascii_case("Weekly") {
                return Some((label.clone(), *used));
            }
        }
    }
    // fallback first quota percent
    for l in lines {
        if let MetricLine::Progress {
            kind,
            label,
            used,
            format: crate::model::ProgressFormat::Percent,
            ..
        } = l
        {
            if *kind == MetricKind::Quota {
                return Some((label.clone(), *used));
            }
        }
    }
    None
}

fn text_value(lines: &[MetricLine], label_sub: &str) -> Option<String> {
    for l in lines {
        if let MetricLine::Text { label, value, .. } = l {
            if label
                .to_ascii_lowercase()
                .contains(&label_sub.to_ascii_lowercase())
            {
                return Some(value.clone());
            }
        }
    }
    None
}

/// Build economics from a probed provider output (and optional model breakdown).
pub fn from_output(o: &ProviderOutput, by_model: Vec<ModelEconomics>) -> Option<ProviderEconomics> {
    let pool = weekly_pct(&o.lines);
    let since = text_value(&o.lines, "since weekly").or_else(|| text_value(&o.lines, "since week"));
    let last30 = text_value(&o.lines, "last 30");
    let expected_week = text_value(&o.lines, "expected this week");
    let expected_month = text_value(&o.lines, "expected this month");

    let (usd_30d, tok_30d) = last30
        .as_deref()
        .map(parse_usd_and_tokens)
        .unwrap_or((None, None));
    let (usd_since, tok_since) = since
        .as_deref()
        .map(parse_usd_and_tokens)
        .unwrap_or((None, None));
    let (usd_exp_w, tok_exp_w) = expected_week
        .as_deref()
        .map(parse_usd_and_tokens)
        .unwrap_or((None, None));
    let (usd_exp_m, _) = expected_month
        .as_deref()
        .map(parse_usd_and_tokens)
        .unwrap_or((None, None));

    // Weekly pool % only aligns with *since weekly reset* $ / tokens.
    // Last-30d model mix must not be scaled by Weekly % (mid-week start or
    // 30d ≫ one week both explode at 100%).
    let api_usd_obs = usd_since.or(usd_30d);
    let tokens_obs = tok_since.or(tok_30d);
    let observed_window = if usd_since.is_some() {
        Some("since_weekly_reset".into())
    } else if usd_30d.is_some() {
        Some("last_30d".into())
    } else {
        None
    };

    let (pool_label, pool_pct) = match &pool {
        Some((l, p)) => (Some(l.clone()), Some(*p)),
        None => (None, None),
    };

    // Full week: prefer CLI "Expected this week", else scale by pool %.
    let mut full_week_api = usd_exp_w;
    let mut full_week_tok = tok_exp_w;
    let mut method = if usd_exp_w.is_some() {
        Some("expected_this_week".into())
    } else {
        None
    };

    let mut pool_pct_at_start = None;
    let mut partial_observation = false;
    if full_week_api.is_none() {
        if let (Some(usd), Some(pct)) = (api_usd_obs, pool_pct) {
            let weekly_aligned = observed_window.as_deref() == Some("since_weekly_reset");
            if weekly_aligned {
                let week_id = crate::pool_baseline::week_and_pct(o).map(|(w, _)| w);
                let first_pct = week_id
                    .as_deref()
                    .and_then(|w| crate::pool_baseline::baseline_pct(&o.provider_id, w))
                    .or_else(|| crate::forecast::earliest_weekly_pct(&o.provider_id));
                pool_pct_at_start = first_pct;
                let est = estimate_full_week(usd, tokens_obs, pct, first_pct);
                full_week_api = est.usd;
                full_week_tok = est.tokens;
                method = Some(est.method.into());
                partial_observation = est.partial;
            }
        }
    }

    let full_month_api = usd_exp_m.or_else(|| full_week_api.map(|w| w * MONTH_WEEK_FACTOR));

    // Need at least something useful
    if full_week_api.is_none() && api_usd_obs.is_none() && usd_30d.is_none() {
        return None;
    }

    Some(ProviderEconomics {
        pool_label,
        pool_pct,
        pool_pct_at_start,
        partial_observation,
        tokens_obs,
        api_usd_obs,
        observed_window,
        api_usd_30d: usd_30d,
        tokens_30d: tok_30d,
        full_week_api_usd: full_week_api,
        full_week_tokens: full_week_tok,
        full_month_api_usd: full_month_api,
        full_pool_method: method,
        by_model,
    })
}

/// Model breakdown for Grok (ledger) or Codex/Claude (log cost).
pub fn model_breakdown_for(provider_id: &str) -> Vec<ModelEconomics> {
    match provider_id {
        "grok" => grok_models(),
        "codex" => log_cost_models(crate::cost::Source::Codex),
        "claude" => log_cost_models(crate::cost::Source::Claude),
        _ => Vec::new(),
    }
}

fn grok_models() -> Vec<ModelEconomics> {
    let now = crate::util::now_ms();
    let recs = crate::grok_ledger::read_window(now);
    let mut map: std::collections::HashMap<String, (u64, f64)> = std::collections::HashMap::new();
    for r in recs {
        let tok = r.tokens_for_total();
        let cost = r.list_cost_usd().unwrap_or(0.0);
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

fn log_cost_models(source: crate::cost::Source) -> Vec<ModelEconomics> {
    let Some(sum) = crate::cost::estimate(source) else {
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
    fn parse_last30() {
        let (u, t) = parse_usd_and_tokens("$34.09 · 51M tokens");
        assert!((u.unwrap() - 34.09).abs() < 0.01);
        assert_eq!(t, Some(51_000_000));
    }

    /// $300 plan, true capacity ~$1000/week at 100% pool, ~$4286/month (×30/7).
    #[test]
    fn fixture_300_plan_week_estimates() {
        struct Case {
            start: Option<f64>,
            now: f64,
            obs: f64,
            want: Option<f64>,
            method: &'static str,
        }
        let cases = [
            Case {
                start: Some(4.0),
                now: 40.0,
                obs: 400.0,
                want: Some(400.0 * 100.0 / 36.0),
                method: "scale_by_pool_span",
            },
            Case {
                start: Some(10.0),
                now: 40.0,
                obs: 300.0,
                want: Some(1000.0),
                method: "scale_by_pool_span",
            },
            Case {
                start: Some(5.0),
                now: 20.0,
                obs: 150.0,
                want: Some(1000.0),
                method: "scale_by_pool_span",
            },
            Case {
                start: Some(10.0),
                now: 11.0,
                obs: 20.0,
                want: None,
                method: "incomplete_window_no_scale",
            },
            Case {
                start: Some(70.0),
                now: 81.0,
                obs: 20.0,
                want: Some(20.0 * 100.0 / 11.0),
                method: "scale_by_pool_span",
            },
            Case {
                start: None,
                now: 81.0,
                obs: 20.0,
                want: None,
                method: "incomplete_window_no_scale",
            },
            Case {
                start: Some(0.0),
                now: 80.0,
                obs: 400.0,
                want: Some(500.0),
                method: "scale_by_pool_span",
            },
        ];
        for c in cases {
            let e = estimate_full_week(c.obs, Some(10_000_000), c.now, c.start);
            assert_eq!(e.method, c.method, "now={} start={:?}", c.now, c.start);
            match (e.usd, c.want) {
                (Some(g), Some(w)) => assert!((g - w).abs() < 0.5, "got {g} want {w}"),
                (None, None) => {}
                other => panic!("usd mismatch {other:?}"),
            }
        }
        // Dual-PC: $400 at 80% from 0% looks like $500/week, not $1000.
        let dual = estimate_full_week(400.0, None, 80.0, Some(0.0));
        assert!((dual.usd.unwrap() - 500.0).abs() < 0.01);
        // Never scale 30d $ by weekly % (caller must not pass 30d as obs).
    }

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
