//! spanreed: Linux-native AI coding subscription usage tracker.
//!
//! Subcommands:
//!   spanreed list                 List known providers and detection state.
//!   spanreed probe [id] [flags]   Probe providers (default: quotas only).
//!   spanreed waybar               Emit Waybar custom-module JSON (one shot).
//!   spanreed json                 Emit raw JSON of all detected providers.
//!   spanreed serve [--interval S] Run the local HTTP API on 127.0.0.1:6736.
//!   spanreed capture serve        Fabric :18736 (/v1 grok, /xai api.x.ai).
//!   spanreed capture serve --watchdog  Restart capture if it exits.
//!   spanreed capture ensure       Start capture (with watchdog) if ports down.
//!   spanreed agent <command>      Local host for desktop.grok.me (was grok-bridge).
//!   spanreed grok-proxy [--bind]  Alias for `capture serve --grok-cli-bind`.
//!   spanreed setup [...]          Install CLI, optional capture service, wire Grok/OpenCode.
//!   spanreed auth copilot [...]   Opt-in link a GitHub token for Copilot.
//!   spanreed auth logout copilot  Forget the stored Copilot credential.
//!   spanreed update-pricing [out] Fetch + filter the upstream price table.
//!   spanreed self-update […]     Check/install latest GitHub Release binary.
//!   spanreed tray […]            System tray (feature `tray`: Spanreed icon).

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
mod cli;
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
pub mod model;
mod output;
pub mod panel;
mod pool_baseline;
mod ports;
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
mod provider_icons;
mod share;
mod share_economics;
mod share_session;
mod share_state;
pub mod sync;
mod sync_store;
mod tray_card;
mod tray_format;
pub mod usage;
mod usage_stats;
mod util;

#[cfg(feature = "tray")]
mod tray;

pub use cli::run_cli;

pub mod notifications;

pub mod desktop_runtime;

#[cfg(feature = "contracts")]
pub mod desktop_contracts;
