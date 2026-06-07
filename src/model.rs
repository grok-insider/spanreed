use serde::{Deserialize, Serialize};

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
        label: String,
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
    },
    Progress {
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
        label: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
    },
    #[serde(rename = "barChart")]
    BarChart {
        label: String,
        points: Vec<BarChartPoint>,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<String>,
    },
}

impl MetricLine {
    pub fn text(label: impl Into<String>, value: impl Into<String>) -> Self {
        MetricLine::Text {
            label: label.into(),
            value: value.into(),
            color: None,
            subtitle: None,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        MetricLine::Badge {
            label: "Error".into(),
            text: text.into(),
            color: Some("#ef4444".into()),
            subtitle: None,
        }
    }

    pub fn percent(label: impl Into<String>, used: f64, resets_at: Option<String>) -> Self {
        MetricLine::Progress {
            label: label.into(),
            used: used.clamp(0.0, 100.0),
            limit: 100.0,
            format: ProgressFormat::Percent,
            resets_at,
            color: None,
        }
    }

    pub fn dollars(
        label: impl Into<String>,
        used: f64,
        limit: f64,
        resets_at: Option<String>,
    ) -> Self {
        MetricLine::Progress {
            label: label.into(),
            used,
            limit,
            format: ProgressFormat::Dollars,
            resets_at,
            color: None,
        }
    }

    pub fn bar_chart(
        label: impl Into<String>,
        points: Vec<BarChartPoint>,
        note: Option<String>,
    ) -> Self {
        MetricLine::BarChart {
            label: label.into(),
            points,
            note,
            color: None,
        }
    }

    /// True when this line is the synthesized "Error" badge.
    pub fn is_error(&self) -> bool {
        matches!(self, MetricLine::Badge { label, .. } if label == "Error")
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
        if let MetricLine::Progress { used, limit, .. } = MetricLine::percent("S", 142.0, None) {
            assert_eq!(used, 100.0);
            assert_eq!(limit, 100.0);
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
        let ok = ProviderOutput::new("x", "X", vec![MetricLine::text("a", "b")]);
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
        assert_eq!(j["points"][0]["valueLabel"], "$1.50");
        assert_eq!(j["note"], "note");
    }

    #[test]
    fn dollars_count_formats_carry_through() {
        let d = MetricLine::dollars("On-demand", 1.0, 10.0, None);
        let j = serde_json::to_value(&d).unwrap();
        assert_eq!(j["format"]["kind"], "dollars");
    }
}
