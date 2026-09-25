//! Subscription probes: turning a provider's usage document into windows and
//! plan metadata. Claude and Kimi share the metadata extraction.

use fabrials_types::{MetricLine, ProviderOutput};
use serde_json::Value;

/// What a host shows for a subscription account between probes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProbeMetadata {
    /// Highest used share of any primary window, in percent.
    pub used_pct: Option<f64>,
    pub resets_at: Option<String>,
    pub plan_slug: Option<String>,
    pub plan_label: Option<String>,
}

/// A provider whose subscription sessions report usage windows.
pub trait SubscriptionProbe {
    fn provider_id(&self) -> &'static str;
    /// Whether a stored credential document is a subscription session rather
    /// than an API key.
    fn is_subscription(&self, document: &Value) -> bool;
    /// Windows and plan lines from the provider's usage response.
    fn usage_output(&self, usage: &Value, now_ms: i64) -> ProviderOutput;
    /// Metadata from a stored document carrying `usage_output`, `plan_slug`
    /// and `plan_label`.
    fn metadata(&self, document: &Value) -> ProbeMetadata {
        probe_metadata(document)
    }
}

/// The busiest primary window (review windows excluded) plus the stored plan.
pub fn probe_metadata(document: &Value) -> ProbeMetadata {
    let output = document
        .get("usage_output")
        .and_then(|value| serde_json::from_value::<ProviderOutput>(value.clone()).ok());
    let primary = output.as_ref().and_then(|output| {
        output
            .lines
            .iter()
            .filter_map(|line| match line {
                MetricLine::Progress {
                    label,
                    used,
                    resets_at,
                    ..
                } if !label.starts_with("Review ") => Some((*used, resets_at.clone())),
                _ => None,
            })
            .max_by(|left, right| left.0.total_cmp(&right.0))
    });
    ProbeMetadata {
        used_pct: primary.as_ref().map(|(used, _)| *used),
        resets_at: primary.and_then(|(_, resets_at)| resets_at),
        plan_slug: document_text(document, "plan_slug"),
        plan_label: document_text(document, "plan_label"),
    }
}

fn document_text(document: &Value, key: &str) -> Option<String> {
    document
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 120)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_busiest_primary_window_and_plan_are_reported() {
        let output = crate::kimi::usage_output(
            &json!({
                "usage": {"limit": "100", "remaining": "60", "resetTime": "2030-01-02T00:00:00Z"},
                "limits": [{"detail": {"limit": "100", "remaining": "30", "resetTime": "2030-01-01T05:00:00Z"}}]
            }),
            0,
        );
        let document = json!({
            "usage_output": output,
            "plan_slug": " pro ",
            "plan_label": "",
        });
        let metadata = crate::kimi::Subscription.metadata(&document);
        assert_eq!(metadata.plan_slug.as_deref(), Some("pro"));
        assert_eq!(metadata.plan_label, None);
        assert_eq!(metadata.used_pct, Some(70.0));
        assert_eq!(metadata.resets_at.as_deref(), Some("2030-01-01T05:00:00Z"));
        assert_eq!(metadata, crate::claude::Subscription.metadata(&document));
    }

    #[test]
    fn documents_without_output_have_no_window() {
        assert_eq!(probe_metadata(&json!({})), ProbeMetadata::default());
    }

    #[test]
    fn subscription_documents_are_recognized_per_provider() {
        assert!(
            crate::claude::Subscription.is_subscription(&json!({"access_token": "sk-ant-oat01-x"}))
        );
        assert!(!crate::claude::Subscription.is_subscription(&json!({"access_token": "gho_other"})));
        assert!(crate::kimi::Subscription.is_subscription(&json!({"access_token": "kimi-access"})));
        assert!(!crate::kimi::Subscription.is_subscription(&json!({"access_token": "  "})));
        assert!(!crate::kimi::Subscription.is_subscription(&json!({"api_key": "sk-kimi"})));
    }
}
