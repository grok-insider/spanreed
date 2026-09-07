//! Map probe outputs → community share API snapshot (no secrets).
//!
//! Callers POST the JSON. This crate does not do HTTP.

mod economics;

pub use economics::{
    economics_from_hops, scale_to_full_pool, scale_tokens_to_full, snapshot_from_hops, HopsWindow,
    MIN_PCT_SCALE, MONTH_WEEK_FACTOR,
};

use fabrials_model::{
    MetricKind, MetricLine, ProgressFormat, ProviderEconomics, ProviderOutput, ResetEvent,
    ShareLine, ShareProvider, ShareSnapshot, ShareSource,
};

pub fn line_to_share(line: &MetricLine) -> Option<ShareLine> {
    match line {
        MetricLine::Progress {
            label,
            used,
            limit,
            format,
            resets_at,
            ..
        } => {
            let kind = match format {
                ProgressFormat::Percent => "percent",
                ProgressFormat::Dollars => "dollars",
                ProgressFormat::Count { .. } => "count",
            };
            Some(ShareLine {
                kind: kind.into(),
                label: label.clone(),
                used: Some(*used),
                limit: Some(*limit),
                resets_at: resets_at.clone(),
                value: None,
            })
        }
        MetricLine::Text { label, value, .. } => Some(ShareLine {
            kind: "text".into(),
            label: label.clone(),
            used: None,
            limit: None,
            resets_at: None,
            value: Some(value.clone()),
        }),
        MetricLine::Badge { label, text, .. } => Some(ShareLine {
            kind: "badge".into(),
            label: label.clone(),
            used: None,
            limit: None,
            resets_at: None,
            value: Some(text.clone()),
        }),
        MetricLine::BarChart { .. } => None,
    }
}

pub struct ProviderShareExtras {
    pub economics: Option<ProviderEconomics>,
    pub resets: Vec<ResetEvent>,
}

/// Build a snapshot. `app` is `spanreed` or `ai-relay`.
pub fn snapshot_from_outputs(
    outputs: &[ProviderOutput],
    app: &str,
    version: &str,
    captured_at: &str,
    extras: &[ProviderShareExtras],
) -> ShareSnapshot {
    let mut providers = Vec::new();
    let mut any_econ = false;
    for (i, o) in outputs.iter().enumerate() {
        let lines: Vec<ShareLine> = o
            .lines
            .iter()
            .filter(|l| {
                matches!(
                    l.kind(),
                    MetricKind::Quota | MetricKind::Plan | MetricKind::Cost | MetricKind::Error
                )
            })
            .filter_map(line_to_share)
            .collect();
        if lines.is_empty() {
            continue;
        }
        let extra = extras.get(i);
        let economics = extra.and_then(|e| e.economics.clone());
        if economics.is_some() {
            any_econ = true;
        }
        let resets = extra.map(|e| e.resets.clone()).unwrap_or_default();
        providers.push(ShareProvider {
            id: o.provider_id.clone(),
            plan: o.plan.clone().filter(|p| !p.trim().is_empty()),
            lines,
            economics,
            resets,
        });
    }
    ShareSnapshot {
        schema_version: if any_econ { 2 } else { 1 },
        captured_at: captured_at.into(),
        source: ShareSource {
            app: app.into(),
            version: version.into(),
        },
        providers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_maps_quota_skips_charts_no_secrets() {
        let out = ProviderOutput::new(
            "grok",
            "Grok",
            vec![
                MetricLine::percent("Weekly", 22.0, None),
                MetricLine::bar_chart("Usage Trend", vec![], None),
            ],
        )
        .with_plan(Some("SuperGrok Heavy".into()));
        let snap = snapshot_from_outputs(&[out], "ai-relay", "0.1.0", "2026-08-21T00:00:00Z", &[]);
        assert_eq!(snap.source.app, "ai-relay");
        assert_eq!(snap.schema_version, 1);
        assert_eq!(snap.providers.len(), 1);
        assert_eq!(snap.providers[0].lines.len(), 1);
        assert_eq!(snap.providers[0].lines[0].kind, "percent");
        let j = serde_json::to_string(&snap).unwrap();
        for forbidden in [
            "access_token",
            "refresh_token",
            "authorization",
            "api_key",
            "cookie",
        ] {
            assert!(!j.contains(forbidden), "leaked {forbidden}");
        }
    }

    #[test]
    fn count_progress_stays_count_not_percent() {
        let line = MetricLine::Progress {
            kind: MetricKind::Quota,
            label: "Requests".into(),
            used: 3.0,
            limit: 10.0,
            format: ProgressFormat::Count {
                suffix: "reqs".into(),
            },
            resets_at: None,
            color: None,
        };
        let s = line_to_share(&line).unwrap();
        assert_eq!(s.kind, "count");
    }
}
