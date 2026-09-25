use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub detection: bool,
    pub oauth: bool,
    pub api_key: bool,
    pub quota: bool,
    pub balance: bool,
    pub reset_inventory: bool,
    pub models: bool,
    pub inference: bool,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDescriptor {
    pub id: String,
    pub name: String,
    pub capabilities: Capabilities,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharingConsent {
    pub share_metrics: bool,
    pub sync_history: bool,
}
