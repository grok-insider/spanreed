//! Shared data types and portable contracts for Fabrials hosts (Spanreed and ai-relay).
//!
//! No I/O, networking, storage, credentials or presentation. Callers own
//! persistence, HTTP and pricing.

pub mod consumption;
pub mod endpoints;
pub mod history;
pub mod hop;
pub mod metric;
pub mod migration;
pub mod observation;
pub mod private_sync;
pub mod provider;
pub mod recovery;
pub mod share;
pub mod usage;

pub use history::HistorySample;
pub use metric::{
    BarChartPoint, MetricKind, MetricLine, ProbeView, ProgressFormat, ProviderOutput,
};
pub use observation::*;
pub use provider::*;
pub use share::{
    ModelEconomics, ProviderEconomics, ResetEvent, ShareLine, ShareProvider, ShareSnapshot,
    ShareSource,
};
pub use usage::{HopRecord, UNIT_AUDIO_MS, UNIT_CHARS, UNIT_IMAGES, UNIT_TOKENS, UNIT_VIDEO_MS};

#[cfg(feature = "contracts")]
pub mod contracts;
