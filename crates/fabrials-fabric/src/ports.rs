//! Storage ports. The engine and its hosts program against these traits;
//! `fabrials-store-sqlite` implements them. None of them names a database.

use fabrials_types::consumption::{ConsumptionRecord, ImportCheckpoint, UsageFilter};
use fabrials_types::{HistorySample, HopRecord};
use serde_json::Value;

/// Durable, idempotent completed-hop ledger scoped by namespace.
pub trait HopStore {
    /// `false` when a record with the same identity was already stored.
    fn append(&mut self, namespace: &str, record: &HopRecord) -> Result<bool, String>;
    fn recent(&self, namespace: &str, limit: usize) -> Result<Vec<HopRecord>, String>;
    /// Every matching record in `[from_ms, to_ms]`, never silently truncated.
    fn read_window(
        &self,
        namespace: &str,
        provider: Option<&str>,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<HopRecord>, String>;
}

/// Quota history samples scoped by namespace.
pub trait HistoryStore {
    /// Number of samples written after duplicate and reset handling.
    fn append(&mut self, namespace: &str, samples: &[HistorySample]) -> Result<usize, String>;
    fn read(
        &self,
        namespace: &str,
        provider: Option<&str>,
        limit: usize,
    ) -> Result<Vec<HistorySample>, String>;
    /// Drop samples older than `cutoff_ms`, keeping the latest one per metric.
    fn prune(&mut self, namespace: &str, cutoff_ms: i64) -> Result<usize, String>;
}

/// Checkpoint of one imported usage source.
#[derive(Debug, Clone)]
pub struct StoredSource {
    pub parser_version: u32,
    pub fingerprint: String,
    pub generation: u64,
    pub checkpoint: ImportCheckpoint,
    pub last_success_ms: i64,
}

/// One atomic import of a usage source.
pub struct ImportBatch<'a> {
    pub source: &'a str,
    pub client: &'a str,
    pub parser_version: u32,
    pub fingerprint: &'a str,
    pub expected_generation: Option<u64>,
    pub replace: bool,
    pub checkpoint: &'a ImportCheckpoint,
    pub records: &'a [ConsumptionRecord],
    pub at_ms: i64,
}

/// Imported local and remote usage with per-source checkpoints.
pub trait UsageStore {
    fn source(&self, source: &str) -> Result<Option<StoredSource>, String>;
    /// Monotonic revision, bumped by every import that changed records.
    fn revision(&self) -> Result<u64, String>;
    /// Commit the records and checkpoint of one source; returns the new generation.
    fn import(&mut self, batch: ImportBatch<'_>) -> Result<u64, String>;
    fn records(&self, filter: &UsageFilter) -> Result<Vec<ConsumptionRecord>, String>;
}

/// Which credential rotation a journal entry guards.
pub struct Scope<'a> {
    pub environment: &'a str,
    pub owner: &'a str,
    pub provider: &'a str,
    pub alias: &'a str,
}

/// What an interrupted rotation left behind.
pub enum Recovery {
    Clean,
    Interrupted,
    Replacement(Value),
}

/// A host-owned source and durable journal held under one cooperating lease.
/// Implementations guard the account generation for the lifetime of this value.
pub trait RotationSource {
    fn load(&self) -> Result<Value, String>;
    fn recover(&self, current: &Value) -> Result<Recovery, String>;
    fn begin(&self, expected: &Value) -> Result<(), String>;
    fn rotated(&self, replacement: &Value) -> Result<(), String>;
    fn commit(&self, expected: &Value, replacement: &Value) -> Result<bool, String>;
    fn complete(&self) -> Result<(), String>;
}

/// Durable journal of one scoped credential rotation. An interrupted
/// redemption is never retried with the same credential generation.
pub trait CredentialJournal {
    fn recover(&self, current: &Value) -> Result<Recovery, String>;
    /// Commit this marker before sending a request that may rotate a grant.
    fn begin(&self, expected: &Value) -> Result<(), String>;
    /// Persist the complete replacement before attempting to update its source.
    fn rotated(&self, replacement: &Value) -> Result<(), String>;
    /// Call only after the source is updated or its generation has changed.
    fn complete(&self) -> Result<(), String>;
}

/// Durable delivery claims. Transport acknowledgement is separate from claiming.
pub trait DeliveryStore {
    type Claim;
    fn claim(
        &self,
        namespace: &str,
        key: &str,
        now: i64,
        expires: i64,
    ) -> Result<Option<Self::Claim>, String>;
    fn acknowledge(&self, claim: &Self::Claim) -> Result<(), String>;
}
