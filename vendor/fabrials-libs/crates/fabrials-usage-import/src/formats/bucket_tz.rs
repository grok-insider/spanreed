// Adapted from Tokscale 0d621cae343af9b057fb4a38c84d089524ac4378.
// Copyright (c) 2025 Junho Yeo. MIT; see TOKSCALE-LICENSE.
//! The timezone a scan buckets usage into. Day keys follow `chrono::Local`,
//! read afresh on every scan.

use std::fmt::Display;

use chrono::TimeZone;

/// The zone a scan buckets its day keys into.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum BucketTimezone {
    /// Day keys follow `chrono::Local`, re-read every scan.
    #[default]
    Local,
}

impl BucketTimezone {
    /// The `YYYY-MM-DD` day key this instant falls in.
    pub fn day_key(&self, timestamp_ms: i64) -> String {
        match self {
            Self::Local => format_day_key(timestamp_ms, &chrono::Local),
        }
    }
}

/// Format an instant as a `YYYY-MM-DD` day key in `timezone`.
///
/// Returns an empty string for an instant the zone cannot represent, matching
/// what the pre-pinning `timestamp_to_date` did. Mapping an *instant* into a
/// zone is unambiguous for every real zone — the ambiguity in `chrono` runs the
/// other way, local wall-clock to instant — so the non-`Single` arm is a
/// defensive floor, not a live path.
pub(crate) fn format_day_key<Tz>(timestamp_ms: i64, timezone: &Tz) -> String
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    match timezone.timestamp_millis_opt(timestamp_ms) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d").to_string(),
        _ => String::new(),
    }
}
