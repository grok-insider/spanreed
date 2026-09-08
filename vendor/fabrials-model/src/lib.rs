//! Shared DTO types for Fabrials (Spanreed and ai-relay).
//!
//! No I/O. Callers own persistence, HTTP, and pricing.

pub mod private_sync;
pub mod history;
pub use history::HistorySample;
pub mod metric;
pub mod share;
pub mod usage;

pub use metric::{
    BarChartPoint, MetricKind, MetricLine, ProbeView, ProgressFormat, ProviderOutput,
};
pub use share::{
    ModelEconomics, ProviderEconomics, ResetEvent, ShareLine, ShareProvider, ShareSnapshot,
    ShareSource,
};
pub use usage::{UsageRecord, UNIT_AUDIO_MS, UNIT_CHARS, UNIT_IMAGES, UNIT_TOKENS, UNIT_VIDEO_MS};

#[cfg(feature = "contracts")]
pub mod contracts;
