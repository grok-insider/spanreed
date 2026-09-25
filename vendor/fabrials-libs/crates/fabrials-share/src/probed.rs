//! Plan economics from a probed provider output (the Spanreed CLI lines).
//!
//! Primary goal: estimate **API list-price $ if rate-limit pools run at 100%**
//! for a week (and month ≈ week × 30/7). Multi-model mixes are valued as
//! Σ tokens_i × price_i by the host's cost engine; a single model price is
//! never assumed.

use fabrials_types::{
    MetricKind, MetricLine, ModelEconomics, ProgressFormat, ProviderEconomics, ProviderOutput,
};

use crate::economics::{scale_to_full_pool, scale_tokens_to_full, MONTH_WEEK_FACTOR};

/// Minimum Weekly % increase between two samples before a span is trusted.
pub const MIN_PCT_DELTA: f64 = 3.0;
/// Minimum Weekly % for one-shot density (tokens/pct_now) from the origin.
pub const MIN_PCT_ONESHOT: f64 = 5.0;
/// A first sample at or below this % is treated as the week's origin.
pub const NEAR_ORIGIN_PCT: f64 = 5.0;

#[derive(Debug, Clone, PartialEq)]
pub struct WeekProjection {
    pub tokens: u64,
    pub cost_usd: f64,
    pub low_confidence: bool,
}

/// Tokens and cost per pool % from the origin, only when `weekly_pct` is high
/// enough. Callers use it only when observation started near 0%.
pub fn density_oneshot(tokens: u64, cost_usd: f64, weekly_pct: f64) -> Option<(f64, f64)> {
    if weekly_pct < MIN_PCT_ONESHOT {
        return None;
    }
    Some((tokens as f64 / weekly_pct, cost_usd / weekly_pct))
}

/// Scale observed $ across a pool span `[pct_lo, pct_hi]` up to 100%.
pub fn scale_span_to_full(obs: f64, pct_lo: f64, pct_hi: f64) -> Option<f64> {
    if !obs.is_finite() || obs < 0.0 {
        return None;
    }
    let d = pct_hi - pct_lo;
    if !d.is_finite() || d < MIN_PCT_DELTA - f64::EPSILON {
        return None;
    }
    Some(obs * (100.0 / d))
}

/// True when the earliest sample this week is close enough to 0% to one-shot.
pub fn origin_is_near_zero(first_weekly_pct: f64) -> bool {
    first_weekly_pct.is_finite() && first_weekly_pct <= NEAR_ORIGIN_PCT
}

/// Project the week to a 100% pool using a density.
pub fn project_week_to_full(
    tokens_now: u64,
    cost_now: f64,
    weekly_pct: f64,
    tokens_per_pct: f64,
    cost_per_pct: f64,
    low_confidence: bool,
) -> WeekProjection {
    let remaining = (100.0 - weekly_pct).max(0.0);
    let add_tok = (tokens_per_pct * remaining).max(0.0).round() as u64;
    let add_cost = (cost_per_pct * remaining).max(0.0);
    WeekProjection {
        tokens: tokens_now.saturating_add(add_tok),
        cost_usd: cost_now + add_cost,
        low_confidence,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FullWeekEst {
    pub usd: Option<f64>,
    pub tokens: Option<u64>,
    pub method: &'static str,
    pub partial: bool,
}

/// Full-week estimate at a 100% pool. `pct_start` is the first-seen pool %
/// this week. Two machines sharing one pool are not detectable here; the
/// estimate is marked `partial` when the window did not start at the origin.
pub fn estimate_full_week(
    obs_usd: f64,
    obs_tokens: Option<u64>,
    pct_now: f64,
    pct_start: Option<f64>,
) -> FullWeekEst {
    let lo = pct_start.unwrap_or(pct_now);
    let from_origin = origin_is_near_zero(lo);
    if let Some(lo) = pct_start.filter(|p| *p + MIN_PCT_DELTA <= pct_now) {
        if let Some(usd) = scale_span_to_full(obs_usd, lo, pct_now) {
            let d = (pct_now - lo).max(MIN_PCT_DELTA);
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

/// Parse "$34.09 · 51M tokens" / "51M tokens · ~$34" / "~794M · ~$569.32".
pub fn parse_usd_and_tokens(value: &str) -> (Option<f64>, Option<u64>) {
    let mut usd = None;
    let mut tokens = None;
    for cap in value.split(['·', '|']) {
        let t = cap.trim();
        let rest = t.strip_prefix('~').unwrap_or(t);
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

fn quota(line: &MetricLine) -> Option<(&String, f64, &ProgressFormat)> {
    match line {
        MetricLine::Progress {
            kind: MetricKind::Quota,
            label,
            used,
            format,
            ..
        } => Some((label, *used, format)),
        _ => None,
    }
}

/// The Weekly quota line, else the first percent quota line.
pub fn weekly_pct(lines: &[MetricLine]) -> Option<(String, f64)> {
    lines
        .iter()
        .filter_map(quota)
        .find(|(label, _, _)| label.eq_ignore_ascii_case("Weekly"))
        .or_else(|| {
            lines
                .iter()
                .filter_map(quota)
                .find(|(_, _, format)| matches!(format, ProgressFormat::Percent))
        })
        .map(|(label, used, _)| (label.clone(), used))
}

/// The value of the first text line whose label contains `label_sub`
/// (case-insensitive).
pub fn text_value(lines: &[MetricLine], label_sub: &str) -> Option<String> {
    let needle = label_sub.to_ascii_lowercase();
    lines.iter().find_map(|line| match line {
        MetricLine::Text { label, value, .. } if label.to_ascii_lowercase().contains(&needle) => {
            Some(value.clone())
        }
        _ => None,
    })
}

/// Economics from a probed provider output and an optional model breakdown.
///
/// `first_pct` answers the first-seen weekly pool % for this output's week
/// (hosts keep those samples); it is only consulted when the observed $ is
/// aligned with the weekly reset.
pub fn from_output(
    o: &ProviderOutput,
    by_model: Vec<ModelEconomics>,
    first_pct: &dyn Fn(&ProviderOutput) -> Option<f64>,
) -> Option<ProviderEconomics> {
    let pool = weekly_pct(&o.lines);
    let since = text_value(&o.lines, "since weekly").or_else(|| text_value(&o.lines, "since week"));
    let last30 = text_value(&o.lines, "last 30");
    let expected_week = text_value(&o.lines, "expected this week");
    let expected_month = text_value(&o.lines, "expected this month");
    let parsed = |value: Option<String>| {
        value
            .as_deref()
            .map(parse_usd_and_tokens)
            .unwrap_or((None, None))
    };
    let (usd_30d, tok_30d) = parsed(last30);
    let (usd_since, tok_since) = parsed(since);
    let (usd_exp_w, tok_exp_w) = parsed(expected_week);
    let (usd_exp_m, _) = parsed(expected_month);

    // Weekly pool % only aligns with *since weekly reset* $ and tokens. A
    // last-30d mix must not be scaled by the weekly % (a mid-week start or 30
    // days of use both explode at 100%).
    let api_usd_obs = usd_since.or(usd_30d);
    let tokens_obs = tok_since.or(tok_30d);
    let observed_window = if usd_since.is_some() {
        Some("since_weekly_reset".to_string())
    } else if usd_30d.is_some() {
        Some("last_30d".to_string())
    } else {
        None
    };
    let (pool_label, pool_pct) = match &pool {
        Some((label, pct)) => (Some(label.clone()), Some(*pct)),
        None => (None, None),
    };

    let mut full_week_api = usd_exp_w;
    let mut full_week_tok = tok_exp_w;
    let mut method = usd_exp_w.map(|_| "expected_this_week".to_string());
    let mut pool_pct_at_start = None;
    let mut partial_observation = false;
    if full_week_api.is_none() && observed_window.as_deref() == Some("since_weekly_reset") {
        if let (Some(usd), Some(pct)) = (api_usd_obs, pool_pct) {
            let first = first_pct(o);
            pool_pct_at_start = first;
            let est = estimate_full_week(usd, tokens_obs, pct, first);
            full_week_api = est.usd;
            full_week_tok = est.tokens;
            method = Some(est.method.into());
            partial_observation = est.partial;
        }
    }

    let full_month_api = usd_exp_m.or_else(|| full_week_api.map(|w| w * MONTH_WEEK_FACTOR));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn no_baseline(_: &ProviderOutput) -> Option<f64> {
        None
    }

    #[test]
    fn parse_last30() {
        let (u, t) = parse_usd_and_tokens("$34.09 · 51M tokens");
        assert!((u.unwrap() - 34.09).abs() < 0.01);
        assert_eq!(t, Some(51_000_000));
        let (u, t) = parse_usd_and_tokens("~794M · ~$569.32");
        assert!((u.unwrap() - 569.32).abs() < 0.01);
        assert_eq!(t, Some(794_000_000));
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
        // Two machines on one pool: $400 at 80% from 0% looks like $500/week, not $1000.
        let dual = estimate_full_week(400.0, None, 80.0, Some(0.0));
        assert!((dual.usd.unwrap() - 500.0).abs() < 0.01);
    }

    fn grok_output(pct: f64) -> ProviderOutput {
        ProviderOutput::new(
            "grok",
            "Grok",
            vec![
                MetricLine::percent("Weekly", pct, None),
                MetricLine::text(
                    MetricKind::Cost,
                    "Since weekly reset",
                    "$20.00 · 10M tokens",
                ),
            ],
        )
    }

    #[test]
    fn mid_week_start_does_not_oneshot() {
        let e = from_output(&grok_output(81.0), vec![], &no_baseline).unwrap();
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
    fn the_host_baseline_scales_the_observed_span() {
        let asked = std::cell::Cell::new(0);
        let baseline = |o: &ProviderOutput| {
            asked.set(asked.get() + 1);
            assert_eq!(o.provider_id, "grok");
            Some(1.0)
        };
        let e = from_output(&grok_output(21.0), vec![], &baseline).unwrap();
        assert_eq!(asked.get(), 1);
        assert_eq!(e.pool_pct_at_start, Some(1.0));
        assert!((e.full_week_api_usd.unwrap() - 100.0).abs() < 1e-9);
        assert_eq!(e.full_week_tokens, Some(50_000_000));
        assert_eq!(e.full_pool_method.as_deref(), Some("scale_by_pool_span"));
        assert!(!e.partial_observation);
        assert!((e.full_month_api_usd.unwrap() - 100.0 * MONTH_WEEK_FACTOR).abs() < 1e-9);
    }

    #[test]
    fn multi_model_not_single_price() {
        // A Sol-heavy mix must not equal Luna-only pricing of the same tokens.
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
        assert!(sum > 10.0);
    }

    #[test]
    fn project_week_extrapolates_to_full_pool() {
        let w = project_week_to_full(50_000_000, 100.0, 50.0, 1_000_000.0, 2.0, false);
        assert_eq!(w.tokens, 100_000_000);
        assert!((w.cost_usd - 200.0).abs() < 0.01);
        assert!(!w.low_confidence);
    }

    #[test]
    fn oneshot_requires_min_pct() {
        assert!(density_oneshot(1_000_000, 10.0, 2.0).is_none());
        let (t, c) = density_oneshot(10_000_000, 50.0, 10.0).unwrap();
        assert!((t - 1_000_000.0).abs() < 1.0);
        assert!((c - 5.0).abs() < 0.01);
    }

    #[test]
    fn scale_span_uses_delta_not_origin() {
        // $20 observed while the pool moved 70% → 80% ⇒ $200 @ 100%, not $25.
        let v = scale_span_to_full(20.0, 70.0, 80.0).unwrap();
        assert!((v - 200.0).abs() < 1e-9);
        assert!(scale_span_to_full(20.0, 78.0, 80.0).is_none());
    }
}
