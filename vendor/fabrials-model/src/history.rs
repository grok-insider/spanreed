//! Shared quota history observations, independent of persistence.
use serde::{Deserialize, Serialize};
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistorySample {
    pub ts_ms: i64,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub label: String,
    pub used: f64,
    pub limit: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_start_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_window_secs: Option<i64>,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
}

impl HistorySample {
    pub fn key(&self) -> (String, String) {
        (self.provider.clone(), self.label.clone())
    }
}

