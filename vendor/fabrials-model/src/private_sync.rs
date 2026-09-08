//! Opt-in private observations. No provider credentials or request bodies.
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum PrivateEvent {
    Quota { at_ms: i64, output: crate::ProviderOutput },
    History { sample: crate::HistorySample },
    Request { record: crate::UsageRecord },
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateObservation {
    pub source: String,
    pub event: PrivateEvent,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivatePush {
    pub device: String,
    pub observations: Vec<PrivateObservation>,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateStoredObservation {
    pub cursor: i64,
    pub device: String,
    pub observation: PrivateObservation,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivatePage {
    pub observations: Vec<PrivateStoredObservation>,
    pub next_cursor: i64,
    pub has_more: bool,
}

/// Descending presentation page; independent from the ascending replication cursor.
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateRecentPage {
    pub observations: Vec<PrivateStoredObservation>,
    pub before: Option<i64>,
    pub has_more: bool,
}
