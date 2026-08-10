//! Build anonymous plan economics for community share.
//!
//! Primary goal: estimate **API list-price $ if rate-limit pools run at 100%**
//! for a week (and month ≈ week × 30/7). Multi-model mixes are valued as
//! Σ tokens_i × price_i (already done by local cost engines); we never pick
//! a single Sol/Terra/Luna price on the server/web.

use crate::forecast::{density_oneshot, project_week_to_full};
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
    if !pool_pct.is_finite() || pool_pct < MIN_PCT_SCALE || pool_pct > 100.0 {
        return None;
    }
    Some(api_usd_obs * (100.0 / pool_pct))
}

pub fn scale_tokens_to_full(tokens_obs: u64, pool_pct: f64) -> Option<u64> {
    if pool_pct < MIN_PCT_SCALE || pool_pct > 100.0 {
        return None;
    }
    Some(((tokens_obs as f64) * (100.0 / pool_pct)).round() as u64)
}

/// Parse "$34.09 · 51M tokens" / "51M tokens · ~$34" / "~794M · ~$569.32".
pub fn parse_usd_and_tokens(value: &str) -> (Option<f64>, Option<u64>) {
    let mut usd = None;
    let mut tokens = None;
    // $1.23 or ~$1.23
    for cap in value.split(|c: char| c == '·' || c == '|') {
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
            kind,
            label,
            used,
            ..
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
            kind,
            label,
            used,
            ..
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
            if label.to_ascii_lowercase().contains(&label_sub.to_ascii_lowercase()) {
                return Some(value.clone());
            }
        }
    }
    None
}

/// Build economics from a probed provider output (and optional model breakdown).
pub fn from_output(o: &ProviderOutput, by_model: Vec<ModelEconomics>) -> Option<ProviderEconomics> {
    let pool = weekly_pct(&o.lines);
    let since = text_value(&o.lines, "since weekly")
        .or_else(|| text_value(&o.lines, "since week"));
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

    // Prefer multi-model sum when present.
    let model_usd: Option<f64> = if by_model.is_empty() {
        None
    } else {
        Some(by_model.iter().map(|m| m.api_usd_list).sum())
    };
    let model_tok: Option<u64> = if by_model.is_empty() {
        None
    } else {
        Some(by_model.iter().map(|m| m.tokens).sum())
    };

    let api_usd_obs = model_usd.or(usd_since).or(usd_30d);
    let tokens_obs = model_tok.or(tok_since).or(tok_30d);
    let observed_window = if model_usd.is_some() || usd_since.is_some() {
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

    if full_week_api.is_none() {
        if let (Some(usd), Some(pct)) = (api_usd_obs, pool_pct) {
            // Prefer density oneshot when pct high enough
            if let (Some(tok), Some((tp, cp))) =
                (tokens_obs, density_oneshot(tokens_obs.unwrap_or(0), usd, pct))
            {
                let proj = project_week_to_full(tok, usd, pct, tp, cp, pct < 5.0);
                full_week_api = Some(proj.cost_usd);
                full_week_tok = Some(proj.tokens);
                method = Some("density_oneshot_to_100".into());
            } else if let Some(v) = scale_to_full_pool(usd, pct) {
                full_week_api = Some(v);
                full_week_tok = tokens_obs.and_then(|t| scale_tokens_to_full(t, pct));
                method = Some("scale_by_pool_pct".into());
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
    v.sort_by(|a, b| b.tokens.cmp(&a.tokens));
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

    #[test]
    fn multi_model_not_single_price() {
        // Sol-heavy mix must not equal Luna-only pricing of same tokens.
        let mix = vec![
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
