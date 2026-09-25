//! spanreed: Linux-native AI coding subscription usage tracker.
//!
//! This package is the `spanreed` command line: `cli/` parses arguments and
//! prints; the work is done by `spanreed_app::app`. The tray lives in
//! `spanreed-tray` (feature `tray`).

mod cli;

use spanreed_app::app;
use spanreed_domain::util;

pub use cli::run_cli;
