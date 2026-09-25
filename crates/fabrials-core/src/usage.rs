//! Consumption records are independent of account quotas and proxy accounting.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}

impl Tokens {
    pub fn validate(&self) -> Result<(), String> {
        if self.cache_read.saturating_add(self.cache_write) > self.input
            || self.reasoning > self.output
            || self.input.checked_add(self.output).is_none()
        {
            return Err("Invalid overlapping token counts".into());
        }
        Ok(())
    }

    pub fn total(&self) -> u64 {
        self.input.saturating_add(self.output)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
pub enum CostOrigin {
    ProviderReported,
    ClientReported,
    Estimated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
pub struct UsageCost {
    pub usd: f64,
    pub origin: CostOrigin,
    pub pricing_revision: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
pub enum UsageOrigin {
    LocalLog,
    RemoteReport,
    Proxy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
pub enum Granularity {
    Request,
    Session,
    AccountPeriod,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[cfg_attr(feature = "contracts", ts(rename = "ConsumptionRecord"))]
pub struct UsageRecord {
    /// Stable within client and origin. Never derived from token counts alone.
    pub id: String,
    pub client: String,
    pub provider: Option<String>,
    pub account: Option<String>,
    pub session: Option<String>,
    /// Local-only project identity; excluded from the default sync projection.
    pub project: Option<String>,
    pub model: Option<String>,
    pub service_tier: Option<String>,
    pub at_ms: i64,
    #[serde(default)]
    pub timestamp_inferred: bool,
    pub period_end_ms: Option<i64>,
    pub origin: UsageOrigin,
    pub granularity: Granularity,
    /// None means the source does not report tokens, not zero consumption.
    pub tokens: Option<Tokens>,
    pub cost: Option<UsageCost>,
    pub request_id: Option<String>,
}

impl UsageRecord {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || self.client.is_empty() || self.at_ms < 0 {
            return Err("Usage record requires an identity and timestamp".into());
        }
        if self.period_end_ms.is_some_and(|end| end < self.at_ms) {
            return Err("Usage period ends before it starts".into());
        }
        if let Some(tokens) = &self.tokens {
            tokens.validate()?;
        }
        if self
            .cost
            .as_ref()
            .is_some_and(|cost| !cost.usd.is_finite() || cost.usd < 0.0)
        {
            return Err("Invalid usage cost".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
pub enum ImportState {
    Ready,
    NoActivity,
    NoData,
    NeedsConnection,
    UnsupportedFormat,
    Partial,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
pub struct SourceStatus {
    pub source: String,
    pub client: String,
    pub state: ImportState,
    pub last_success_ms: Option<i64>,
    pub revision: u64,
    pub records: u64,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
pub struct UsageFilter {
    pub client: Option<String>,
    pub provider: Option<String>,
    pub account: Option<String>,
    pub model: Option<String>,
    pub session: Option<String>,
    pub project: Option<String>,
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportCheckpoint {
    pub offset: u64,
    pub parser_state: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedUsage {
    pub records: Vec<UsageRecord>,
    pub checkpoint: ImportCheckpoint,
    pub rejected_records: u64,
    pub recognized_records: u64,
}

pub trait UsageParser: Send + Sync {
    fn client(&self) -> &str;
    fn version(&self) -> u32;
    fn incremental(&self) -> bool;
    fn parse(
        &self,
        bytes: &[u8],
        source: &str,
        previous: &ImportCheckpoint,
    ) -> Result<ParsedUsage, String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_subsets_are_not_added_to_totals() {
        let tokens = Tokens {
            input: 100,
            output: 30,
            cache_read: 70,
            cache_write: 10,
            reasoning: 20,
        };
        assert!(tokens.validate().is_ok());
        assert_eq!(tokens.total(), 130);
        assert!(Tokens {
            reasoning: 31,
            ..tokens.clone()
        }
        .validate()
        .is_err());
        assert!(Tokens {
            cache_write: 31,
            ..tokens
        }
        .validate()
        .is_err());
    }
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsumptionTotal {
    pub key: String,
    pub tokens: u64,
    pub unknown_token_records: u64,
    pub known_usd: f64,
    pub partial: bool,
    pub records: u64,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumptionReport {
    pub revision: u64,
    pub sources: Vec<SourceStatus>,
    pub total: ConsumptionTotal,
    pub daily: Vec<ConsumptionTotal>,
    pub period_totals: Vec<ConsumptionTotal>,
    pub clients: Vec<ConsumptionTotal>,
    pub models: Vec<ConsumptionTotal>,
    pub sessions: Vec<ConsumptionTotal>,
    pub projects: Vec<ConsumptionTotal>,
    pub records: Vec<UsageRecord>,
    pub records_truncated: bool,
}
