//! Probe/output DTOs. Owned by [`fabrials-model`]; re-exported so `crate::model::…` stays stable.

pub use fabrials_model::{
    BarChartPoint, MetricKind, MetricLine, ProbeView, ProgressFormat, ProviderOutput,
};
