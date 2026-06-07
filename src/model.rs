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
