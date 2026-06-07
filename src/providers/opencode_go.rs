//! OpenCode Go provider.
//!
//! Tracks locally-observed OpenCode Go assistant spend from the OpenCode SQLite
//! history at `~/.local/share/opencode/opencode.db`, against the published Go
//! plan limits (5h $12, weekly $30, monthly $60).

use crate::creds;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "opencode-go";
const NAME: &str = "OpenCode Go";

const LIMIT_5H: f64 = 12.0;
const LIMIT_WEEKLY: f64 = 30.0;
const LIMIT_MONTHLY: f64 = 60.0;

const WINDOW_5H_MS: i64 = 5 * 60 * 60 * 1000;
const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

pub struct OpenCodeGo;

fn db_path() -> std::path::PathBuf {
    creds::data_home().join("opencode").join("opencode.db")
}

fn auth_path() -> std::path::PathBuf {
    creds::data_home().join("opencode").join("auth.json")
}

/// True if auth.json has an `opencode-go` entry with a non-empty key.
fn auth_has_go() -> bool {
    let Some(value) = creds::read_json(&auth_path()) else {
        return false;
    };
    value
        .get("opencode-go")
        .and_then(|e| e.get("key"))
        .and_then(|k| k.as_str())
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}

/// Pull (createdMs, cost) rows for opencode-go assistant messages.
fn load_rows() -> Option<Vec<(i64, f64)>> {
    let sql = "
        SELECT
          CAST(COALESCE(json_extract(data, '$.time.created'), time_created) AS INTEGER) AS createdMs,
          CAST(json_extract(data, '$.cost') AS REAL) AS cost
        FROM message
        WHERE json_valid(data)
          AND json_extract(data, '$.providerID') = 'opencode-go'
          AND json_extract(data, '$.role') = 'assistant'
          AND json_type(data, '$.cost') IN ('integer', 'real')";
    creds::sqlite_query_rows_i64_f64(&db_path(), sql)
}

/// Sum cost across rows whose timestamp falls in [start_ms, end_ms).
fn sum_in_window(rows: &[(i64, f64)], start_ms: i64, end_ms: i64) -> f64 {
    rows.iter()
        .filter(|(ts, _)| *ts >= start_ms && *ts < end_ms)
        .map(|(_, cost)| *cost)
        .sum()
}

/// Start of the current UTC week (Monday 00:00) in ms.
fn utc_week_start_ms(now_ms: i64) -> i64 {
    let now = time::OffsetDateTime::from_unix_timestamp(now_ms / 1000).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let weekday_from_monday = now.weekday().number_days_from_monday() as i64;
    let midnight = now
        .replace_time(time::Time::MIDNIGHT)
        .unix_timestamp();
    (midnight - weekday_from_monday * 86_400) * 1000
}

impl Provider for OpenCodeGo {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        if auth_has_go() {
            return true;
        }
        // Or: any opencode-go history already exists.
        load_rows().map(|r| !r.is_empty()).unwrap_or(false)
    }

    fn probe(&self) -> ProviderOutput {
        let rows = match load_rows() {
            Some(r) => r,
            None => {
                // Visible but no data, per upstream failure behavior.
                if auth_has_go() {
                    return ProviderOutput::new(
                        ID,
                        NAME,
                        vec![MetricLine::Badge {
                            label: "Status".into(),
                            text: "No usage data".into(),
                            color: Some("#a3a3a3".into()),
                            subtitle: None,
                        }],
                    );
                }
                return ProviderOutput::error(ID, NAME, "OpenCode history not found");
            }
        };

        let now = util::now_ms();
        let mut lines = Vec::new();

        // 5h rolling
        let used_5h = sum_in_window(&rows, now - WINDOW_5H_MS, now);
        lines.push(MetricLine::dollars(
            "5h",
            used_5h.min(LIMIT_5H),
            LIMIT_5H,
            util::ms_to_iso(now + WINDOW_5H_MS),
        ));

        // Weekly (UTC Mon..Mon)
        let week_start = utc_week_start_ms(now);
        let used_week = sum_in_window(&rows, week_start, week_start + WEEK_MS);
        lines.push(MetricLine::dollars(
            "Weekly",
            used_week.min(LIMIT_WEEKLY),
            LIMIT_WEEKLY,
            util::ms_to_iso(week_start + WEEK_MS),
        ));

        // Monthly: anchor to earliest observed usage; fall back to calendar.
        let earliest = rows.iter().map(|(ts, _)| *ts).min().unwrap_or(now);
        let (m_start, m_end) = monthly_window(now, earliest);
        let used_month = sum_in_window(&rows, m_start, m_end);
        lines.push(MetricLine::dollars(
            "Monthly",
            used_month.min(LIMIT_MONTHLY),
            LIMIT_MONTHLY,
            util::ms_to_iso(m_end),
        ));

        ProviderOutput::new(ID, NAME, lines)
    }
}

/// Compute the current subscription-style monthly window anchored on the day-of-
/// month of the earliest usage. Returns (start_ms, end_ms).
fn monthly_window(now_ms: i64, anchor_ms: i64) -> (i64, i64) {
    let anchor = time::OffsetDateTime::from_unix_timestamp(anchor_ms / 1000)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let now = time::OffsetDateTime::from_unix_timestamp(now_ms / 1000)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);

    let anchor_day = anchor.day();
    // Find the most recent occurrence of `anchor_day` at or before `now`.
    let mut year = now.year();
    let mut month = now.month();
    let day = clamp_day(year, month, anchor_day);
    let mut start = make_utc(year, month, day);
    if start > now_ms {
        // step back one month
        (year, month) = prev_month(year, month);
        let day = clamp_day(year, month, anchor_day);
        start = make_utc(year, month, day);
    }
    let (ny, nm) = next_month(year, month);
    let nd = clamp_day(ny, nm, anchor_day);
    let end = make_utc(ny, nm, nd);
    (start, end)
}

fn make_utc(year: i32, month: time::Month, day: u8) -> i64 {
    let date = time::Date::from_calendar_date(year, month, day)
        .unwrap_or(time::Date::from_calendar_date(year, month, 1).unwrap());
    date.with_time(time::Time::MIDNIGHT).assume_utc().unix_timestamp() * 1000
}

fn clamp_day(year: i32, month: time::Month, day: u8) -> u8 {
    let last = month.length(year);
    day.min(last)
}

fn prev_month(year: i32, month: time::Month) -> (i32, time::Month) {
    if month == time::Month::January {
        (year - 1, time::Month::December)
    } else {
        (year, month.previous())
    }
}

fn next_month(year: i32, month: time::Month) -> (i32, time::Month) {
    if month == time::Month::December {
        (year + 1, time::Month::January)
    } else {
        (year, month.next())
    }
}
