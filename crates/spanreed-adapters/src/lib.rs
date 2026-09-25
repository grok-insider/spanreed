//! Spanreed adapters: every implementation of the `spanreed-app` ports —
//! provider drivers, account and secret stores, ledgers and SQLite stores,
//! pricing tables, the capture relay, OS service managers and notifiers,
//! HTTP clients for Fabrials, the embedded agent host and the local API.
//! [`services::standard`] assembles them for a composition root.

pub use spanreed_app::{app, context, ports, probe};
pub use spanreed_domain::{model, provider_icons, tray_format, usage_stats, util};

mod output;

pub mod account_keys;
pub mod account_login;
pub mod accounts;
mod activity;
mod addons;
mod api;
mod capture;
mod capture_log;
mod capture_watchdog;
mod client_id;
pub mod codex_session_move;
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
#[cfg(test)]
mod lifecycle_tests;
mod local_control;
pub mod local_relay;
mod local_tokens;
mod machine_name;
pub mod migration;
pub mod panel;
mod pool_baseline;
mod pricing;
pub mod privacy;
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
pub mod services;
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
