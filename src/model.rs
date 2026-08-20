//! Probe/output DTOs. Owned by [`spanreed-model`]; re-exported so `crate::model::…` stays stable.

pub use spanreed_model::{
    BarChartPoint, MetricKind, MetricLine, ProbeView, ProgressFormat, ProviderOutput,
};
