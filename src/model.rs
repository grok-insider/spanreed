use serde::{Deserialize, Serialize};

/// Semantic role of a metric line for presentation filters (probe flags).
///
/// Providers set this when building lines. New providers that use the helpers
/// below participate in default/`--cost`/`--plan`/… views without CLI changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum MetricKind {
    /// Rate-limit / pool progress (default `probe`).
    Quota,
    /// Plan renews, PAYG toggle/cap, plan-level credits.
    Plan,
    /// Last 30 Days, since weekly, cost coverage, on-demand spend.
    Cost,
    Models,
    Cache,
    Trend,
    /// Always shown.
    Error,
    /// Provider-specific; only with `--all`.
    #[default]
    Other,
}

/// How a progress bar's numbers are rendered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProgressFormat {
    /// `used`/`limit` are a 0-100 percentage (limit must be 100).
    Percent,
    /// `used`/`limit` are dollar amounts.
    Dollars,
    /// `used`/`limit` are a raw count with a unit suffix (e.g. "reqs").
    Count { suffix: String },
}

/// One point in a bar chart (e.g. a single day of cost).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BarChartPoint {
    pub label: String,
    pub value: f64,
    #[serde(rename = "valueLabel", skip_serializing_if = "Option::is_none")]
    pub value_label: Option<String>,
}

/// A single line of output for a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MetricLine {
    Text {
        #[serde(default)]
        kind: MetricKind,
        label: String,
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
    },
    Progress {
        #[serde(default)]
        kind: MetricKind,
        label: String,
        used: f64,
        limit: f64,
        format: ProgressFormat,
        #[serde(rename = "resetsAt", skip_serializing_if = "Option::is_none")]
        resets_at: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<String>,
    },
    Badge {
        #[serde(default)]
        kind: MetricKind,
        label: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
    },
    #[serde(rename = "barChart")]
    BarChart {
        #[serde(default)]
        kind: MetricKind,
        label: String,
        points: Vec<BarChartPoint>,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<String>,
    },
}

impl MetricLine {
    pub fn kind(&self) -> MetricKind {
        match self {
            MetricLine::Text { kind, .. }
            | MetricLine::Progress { kind, .. }
            | MetricLine::Badge { kind, .. }
            | MetricLine::BarChart { kind, .. } => *kind,
        }
    }

    pub fn text(kind: MetricKind, label: impl Into<String>, value: impl Into<String>) -> Self {
        MetricLine::Text {
            kind,
            label: label.into(),
            value: value.into(),
            color: None,
            subtitle: None,
        }
    }

    pub fn badge(kind: MetricKind, label: impl Into<String>, text: impl Into<String>) -> Self {
        MetricLine::Badge {
            kind,
            label: label.into(),
            text: text.into(),
            color: None,
            subtitle: None,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        MetricLine::Badge {
            kind: MetricKind::Error,
            label: "Error".into(),
            text: text.into(),
            color: Some("#ef4444".into()),
            subtitle: None,
        }
    }

    /// Rate-limit / pool percentage (always [`MetricKind::Quota`]).
    pub fn percent(label: impl Into<String>, used: f64, resets_at: Option<String>) -> Self {
        MetricLine::Progress {
            kind: MetricKind::Quota,
            label: label.into(),
            used: used.clamp(0.0, 100.0),
            limit: 100.0,
            format: ProgressFormat::Percent,
            resets_at,
            color: None,
        }
    }

    /// Dollar progress; `kind` is usually [`MetricKind::Quota`] or [`MetricKind::Cost`].
    pub fn dollars(
        kind: MetricKind,
        label: impl Into<String>,
        used: f64,
        limit: f64,
        resets_at: Option<String>,
    ) -> Self {
        MetricLine::Progress {
            kind,
            label: label.into(),
            used,
            limit,
            format: ProgressFormat::Dollars,
            resets_at,
            color: None,
        }
    }

    /// Usage trend sparkline (always [`MetricKind::Trend`]).
    pub fn bar_chart(
        label: impl Into<String>,
        points: Vec<BarChartPoint>,
        note: Option<String>,
    ) -> Self {
        MetricLine::BarChart {
            kind: MetricKind::Trend,
            label: label.into(),
            points,
            note,
            color: None,
        }
    }

    /// True when this line is an error badge.
    pub fn is_error(&self) -> bool {
        self.kind() == MetricKind::Error
            || matches!(self, MetricLine::Badge { label, .. } if label == "Error")
    }
}

/// Which detail blocks `probe` should print (default = quotas only).
#[derive(Debug, Clone, Copy, Default)]
pub struct ProbeView {
    pub cost: bool,
    pub models: bool,
    pub cache: bool,
    pub trend: bool,
    pub plan: bool,
    pub all: bool,
}

impl ProbeView {
    pub fn show(&self, kind: MetricKind) -> bool {
        if self.all {
            return true;
        }
        match kind {
            MetricKind::Quota | MetricKind::Error => true,
            MetricKind::Plan => self.plan,
            MetricKind::Cost => self.cost,
            MetricKind::Models => self.models,
            MetricKind::Cache => self.cache,
            MetricKind::Trend => self.trend,
            MetricKind::Other => false,
        }
    }
}

/// The result of probing a single provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOutput {
    pub provider_id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub lines: Vec<MetricLine>,
}

impl ProviderOutput {
    pub fn new(id: &str, name: &str, lines: Vec<MetricLine>) -> Self {
        ProviderOutput {
            provider_id: id.to_string(),
            display_name: name.to_string(),
            plan: None,
            lines,
        }
    }

    pub fn with_plan(mut self, plan: Option<String>) -> Self {
        self.plan = plan.filter(|p| !p.is_empty());
        self
    }

    pub fn error(id: &str, name: &str, msg: impl Into<String>) -> Self {
        ProviderOutput::new(id, name, vec![MetricLine::error(msg)])
    }

    pub fn has_error(&self) -> bool {
        self.lines.iter().any(MetricLine::is_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_clamps_to_0_100() {
        if let MetricLine::Progress {
            used, limit, kind, ..
        } = MetricLine::percent("S", 142.0, None)
        {
            assert_eq!(used, 100.0);
            assert_eq!(limit, 100.0);
            assert_eq!(kind, MetricKind::Quota);
        } else {
            panic!("expected progress");
        }
        if let MetricLine::Progress { used, .. } = MetricLine::percent("S", -5.0, None) {
            assert_eq!(used, 0.0);
        }
    }

    #[test]
    fn error_line_and_output_flagged() {
        let out = ProviderOutput::error("x", "X", "boom");
        assert!(out.has_error());
        assert_eq!(out.lines[0].kind(), MetricKind::Error);
        let ok = ProviderOutput::new(
            "x",
            "X",
            vec![MetricLine::text(MetricKind::Other, "a", "b")],
        );
        assert!(!ok.has_error());
    }

    #[test]
    fn with_plan_drops_empty() {
        let out = ProviderOutput::new("x", "X", vec![]).with_plan(Some(String::new()));
        assert!(out.plan.is_none());
        let out2 = ProviderOutput::new("x", "X", vec![]).with_plan(Some("Pro".into()));
        assert_eq!(out2.plan.as_deref(), Some("Pro"));
    }

    #[test]
    fn progress_serializes_camelcase_contract() {
        let line = MetricLine::percent("Session", 42.0, Some("2099-01-01T00:00:00Z".into()));
        let j = serde_json::to_value(&line).unwrap();
        assert_eq!(j["type"], "progress");
        assert_eq!(j["format"]["kind"], "percent");
        assert_eq!(j["kind"], "quota");
        assert_eq!(j["resetsAt"], "2099-01-01T00:00:00Z");
        assert!(j.get("resets_at").is_none());
    }

    #[test]
    fn bar_chart_serializes_with_value_label() {
        let line = MetricLine::bar_chart(
            "Usage Trend",
            vec![BarChartPoint {
                label: "2026-01-01".into(),
                value: 1.5,
                value_label: Some("$1.50".into()),
            }],
            Some("note".into()),
        );
        let j = serde_json::to_value(&line).unwrap();
        assert_eq!(j["type"], "barChart");
        assert_eq!(j["kind"], "trend");
        assert_eq!(j["points"][0]["valueLabel"], "$1.50");
        assert_eq!(j["note"], "note");
    }

    #[test]
    fn dollars_count_formats_carry_through() {
        let d = MetricLine::dollars(MetricKind::Cost, "On-demand", 1.0, 10.0, None);
        let j = serde_json::to_value(&d).unwrap();
        assert_eq!(j["format"]["kind"], "dollars");
        assert_eq!(j["kind"], "cost");
    }

    #[test]
    fn probe_view_default_is_quota_and_error_only() {
        let v = ProbeView::default();
        assert!(v.show(MetricKind::Quota));
        assert!(v.show(MetricKind::Error));
        assert!(!v.show(MetricKind::Plan));
        assert!(!v.show(MetricKind::Cost));
        assert!(!v.show(MetricKind::Models));
        assert!(!v.show(MetricKind::Cache));
        assert!(!v.show(MetricKind::Trend));
        assert!(!v.show(MetricKind::Other));
        let all = ProbeView {
            all: true,
            ..ProbeView::default()
        };
        assert!(all.show(MetricKind::Other));
        assert!(all.show(MetricKind::Plan));
    }

    #[test]
    fn payg_badge_is_plan_kind() {
        let b = MetricLine::badge(MetricKind::Plan, "Pay as you go", "Disabled");
        assert_eq!(b.kind(), MetricKind::Plan);
    }
}
