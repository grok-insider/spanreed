//! Spanreed application crate: `AppContext` (the composition root), the
//! `app` facade shared by the CLI, the tray, the desktop host and the local
//! HTTP API, and the adapters behind it (providers, drivers, stores, capture,
//! setup, sharing). Pure types and logic come from `spanreed-domain`.

pub use spanreed_domain::{model, ports, provider_icons, tray_format, usage_stats, util};

mod output;

pub mod account_keys;
pub mod account_login;
pub mod accounts;
mod activity;
mod addons;
mod api;
pub mod app;
mod capture;
mod capture_log;
mod capture_watchdog;
mod client_id;
pub mod codex_session_move;
pub mod context;
mod cost;
mod creds;
pub mod desktop_open;
mod drivers;
mod epoch;
pub mod fabrials_login;
mod forecast;
mod grok_ledger;
mod history;
pub mod hosted_client_configuration;
mod http;
mod local_control;
pub mod local_relay;
mod local_tokens;
mod machine_name;
pub mod migration;
pub mod panel;
mod pool_baseline;
mod pricing;
pub mod privacy;
mod probe;
mod proc;
mod product;
mod profiles;
pub mod providers;
pub mod remote_workspace;
mod resets;
mod secret;
mod self_update;
mod setup;
pub use setup::review as client_configuration;
mod share;
mod share_economics;
mod share_session;
mod share_state;
pub mod sync;
mod sync_store;
mod tray_card;
pub mod usage;

pub mod notifications;

pub mod desktop_runtime;

#[cfg(feature = "contracts")]
pub mod desktop_contracts;
