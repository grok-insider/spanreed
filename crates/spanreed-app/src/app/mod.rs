//! Application facade: the use cases shared by the CLI, the tray, the Tauri
//! desktop host and the local HTTP API. Entry points take `&AppContext`
//! where they need process state; front ends only parse input and present
//! results.

pub mod accounts;
pub mod addons;
pub mod agent;
pub mod capture;
pub mod clients;
pub mod fabrials;
pub mod local_api;
pub mod migration;
pub mod notifications;
pub mod profiles;
pub mod proxy;
pub mod routing;
pub mod setup;
pub mod sharing;
pub mod sync;
pub mod updates;
pub mod usage;
pub mod window;

pub use crate::context::AppContext;

pub const PRODUCT_NAME: &str = crate::product::APP_NAME;
