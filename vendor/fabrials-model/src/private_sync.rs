//! Opt-in private observations. No provider credentials or request bodies.
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynchronizedAccount {
    #[serde(default)]
    pub linked_account_id: Option<String>,
    pub device: String,
    pub source: String,
    pub observed_at_ms: i64,
    pub output: crate::ProviderOutput,
    #[serde(default)]
    pub local_usage: Option<LocalUsageSnapshot>,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum PrivateEvent {
    Quota {
        at_ms: i64,
        output: crate::ProviderOutput,
    },
    History {
        sample: crate::HistorySample,
    },
    Request {
        record: crate::UsageRecord,
    },
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
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub source_identities: std::collections::BTreeMap<String, String>,
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

/// Revisioned local log summary, kept outside the v1 observation stream.
/// Logs may overlap hosted requests or contain multiple provider identities.
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalUsageSnapshot {
    pub device: String,
    pub source: String,
    pub observed_at_ms: i64,
    pub partial: bool,
    pub days: Vec<LocalUsageDay>,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalUsageDay {
    pub date: String,
    pub tokens: u64,
    pub estimated_usd: f64,
}

/// Full replacement of one device/client projection. Empty days clear old usage.
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalUsageSnapshotV2 {
    pub device: String,
    pub source: String,
    pub revision: u64,
    pub observed_at_ms: i64,
    pub partial: bool,
    pub days: Vec<LocalUsageDay>,
    #[serde(default)]
    pub period_totals: Option<LocalUsageAggregate>,
}

/// Unallocated session/account totals; never assigned to invented daily requests.
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalUsageAggregate {
    pub tokens: Option<u64>,
    pub known_usd: f64,
    pub partial: bool,
}
