//! Spanreed application crate: the `app` facade (use cases shared by the
//! CLI, the tray, the desktop host and the local HTTP API), the ports those
//! use cases call, probe orchestration and `AppContext`. It performs no I/O
//! itself: `spanreed-adapters` implements the ports and a composition root
//! (the `spanreed` binary, the desktop host) builds the context.

pub use spanreed_domain::{model, output, provider_icons, tray_format, usage_stats, util};

pub mod app;
pub mod context;
pub mod ports;
pub mod probe;

#[cfg(feature = "contracts")]
pub mod desktop_contracts;
