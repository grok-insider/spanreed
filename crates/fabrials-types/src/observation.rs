use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountScope {
    pub environment: String,
    pub owner: String,
    pub provider: String,
    pub account: String,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Availability {
    Available,
    Unsupported,
    Unavailable,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Freshness {
    Fresh,
    Stale,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation<T> {
    pub availability: Availability,
    pub freshness: Freshness,
    pub observed_at_ms: Option<i64>,
    pub source: String,
    pub value: Option<T>,
    pub error: Option<String>,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetCredit {
    pub valid_from_ms: Option<i64>,
    pub expires_at_ms: Option<i64>,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetInventory {
    pub available: u32,
    pub credits: Vec<ResetCredit>,
    pub details_complete: bool,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditBalance {
    pub remaining_usd: Option<f64>,
    pub subscription_remaining_usd: Option<f64>,
    pub subscription_limit_usd: Option<f64>,
    pub purchased_remaining_usd: Option<f64>,
    pub paid_access: Option<bool>,
}

impl CreditBalance {
    pub fn subscription_used_percent(&self) -> Option<f64> {
        let remaining = self.subscription_remaining_usd?;
        let limit = self.subscription_limit_usd?;
        if !remaining.is_finite() || !limit.is_finite() || remaining < 0.0 || limit <= 0.0 {
            return None;
        }
        Some(((1.0 - remaining / limit) * 100.0).clamp(0.0, 100.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn purchased_balance_cannot_be_used_as_subscription_numerator() {
        let mut balance = CreditBalance {
            remaining_usd: Some(100.0),
            subscription_limit_usd: Some(22.0),
            ..Default::default()
        };
        assert_eq!(balance.subscription_used_percent(), None);
        balance.subscription_remaining_usd = Some(11.0);
        assert_eq!(balance.subscription_used_percent(), Some(50.0));
        balance.subscription_limit_usd = Some(f64::NAN);
        assert_eq!(balance.subscription_used_percent(), None);
    }
}
