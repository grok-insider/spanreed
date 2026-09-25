//! Community share snapshot (POST `/v1/usage/snapshots`).

use serde::{Deserialize, Serialize};

/// Opt-in aggregated usage snapshot. Never includes tokens or raw logs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShareSnapshot {
    pub schema_version: u32,
    pub captured_at: String,
    pub source: ShareSource,
    pub providers: Vec<ShareProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShareSource {
    pub app: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShareProvider {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub lines: Vec<ShareLine>,
    /// Structured plan economics (100% pool API $). Schema v2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub economics: Option<ProviderEconomics>,
    /// Early pool resets observed on this install (does not change at 100% week).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resets: Vec<ResetEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShareLine {
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelEconomics {
    pub model: String,
    pub tokens: u64,
    pub api_usd_list: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderEconomics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_pct_at_start: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial_observation: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_obs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_usd_obs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_window: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_usd_30d: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_30d: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_week_api_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_week_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_month_api_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_pool_method: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_model: Vec<ModelEconomics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResetEvent {
    pub ts_ms: i64,
    pub provider: String,
    /// Always `"early"` locally; API may relabel broadcast vs user.
    pub kind: String,
    pub prev_end: String,
    pub new_end: String,
    pub used_before: f64,
    pub used_after: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrip_v2() {
        let snap = ShareSnapshot {
            schema_version: 2,
            captured_at: "2026-08-21T00:00:00Z".into(),
            source: ShareSource {
                app: "spanreed".into(),
                version: "0.1.0".into(),
            },
            providers: vec![ShareProvider {
                id: "grok".into(),
                plan: Some("SuperGrok Heavy".into()),
                lines: vec![ShareLine {
                    kind: "percent".into(),
                    label: "Weekly".into(),
                    used: Some(22.0),
                    limit: Some(100.0),
                    resets_at: None,
                    value: None,
                }],
                economics: Some(ProviderEconomics {
                    pool_label: Some("Weekly".into()),
                    pool_pct: Some(22.0),
                    pool_pct_at_start: None,
                    partial_observation: false,
                    tokens_obs: Some(1000),
                    api_usd_obs: Some(1.5),
                    observed_window: None,
                    api_usd_30d: None,
                    tokens_30d: None,
                    full_week_api_usd: None,
                    full_week_tokens: None,
                    full_month_api_usd: None,
                    full_pool_method: None,
                    by_model: vec![],
                }),
                resets: vec![],
            }],
        };
        let j = serde_json::to_string(&snap).unwrap();
        let back: ShareSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back, snap);
        assert!(!j.contains("access_token"));
    }
}
