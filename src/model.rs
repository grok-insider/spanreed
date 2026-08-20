//! Probe/output DTOs. Owned by [`spanreed-model`](https://github.com/grok-insider/spanreed-model);
//! this module re-exports so in-tree `crate::model::…` stays stable.

pub use spanreed_model::{
    BarChartPoint, MetricKind, MetricLine, ProbeView, ProgressFormat, ProviderOutput,
};
