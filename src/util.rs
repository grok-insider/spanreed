//! Small formatting helpers shared by providers.

use std::sync::OnceLock;
use time::format_description::well_known::Rfc3339;
use time::{Date, Month, OffsetDateTime, UtcOffset};

/// Current unix time in milliseconds.
pub fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

static LOCAL_OFFSET: OnceLock<UtcOffset> = OnceLock::new();

/// Capture the local UTC offset once, while the process is still
/// single-threaded (call from `main` before spawning any threads). `time`
/// refuses to read the local offset once other threads exist, so this is the
/// only reliable point to determine it; daily cost buckets use it afterward.
pub fn init_local_offset() {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let _ = LOCAL_OFFSET.set(offset);
}

/// The captured local offset (UTC if never initialized or unavailable).
pub fn local_offset() -> UtcOffset {
    *LOCAL_OFFSET.get().unwrap_or(&UtcOffset::UTC)
}

/// Format the time remaining until an RFC3339 `resets_at` as a short relative
/// string: `2d 4h`, `3h 12m`, `5m`, or `<1m`. Returns `None` when the value is
/// absent, unparseable, or already in the past.
pub fn reset_in(resets_at: &str) -> Option<String> {
    let reset = OffsetDateTime::parse(resets_at.trim(), &Rfc3339).ok()?;
    let reset_ms = (reset.unix_timestamp_nanos() / 1_000_000) as i64;
    let secs = (reset_ms - now_ms()) / 1000;
    if secs < 0 {
        return None;
    }
    Some(format_duration_secs(secs))
}

fn format_duration_secs(secs: i64) -> String {
    let total_minutes = secs / 60;
    let total_hours = total_minutes / 60;
    let days = total_hours / 24;
    let hours = total_hours % 24;
    let minutes = total_minutes % 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if total_hours > 0 {
        format!("{total_hours}h {minutes}m")
    } else if total_minutes > 0 {
        format!("{total_minutes}m")
    } else {
        "<1m".to_string()
    }
}

/// Format a unix-ms instant as `YYYY-MM-DD` in the captured local timezone.
pub fn local_date_ymd(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let dt = OffsetDateTime::from_unix_timestamp(secs)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .to_offset(local_offset());
    let d = dt.date();
    format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
}

/// Parse an RFC3339 instant and format as local `YYYY-MM-DD`.
pub fn iso_local_date(iso: &str) -> Option<String> {
    let dt = OffsetDateTime::parse(iso.trim(), &Rfc3339).ok()?;
    let ms = (dt.unix_timestamp_nanos() / 1_000_000) as i64;
    Some(local_date_ymd(ms))
}

/// Format a future plan-period ISO as `YYYY-MM-DD · in 2d 4h` (relative omitted
/// when past/unparseable). When `estimated`, append ` · est.`.
pub fn format_plan_renew_value(iso: &str, estimated: bool) -> String {
    let date = iso_local_date(iso).unwrap_or_else(|| iso.trim().to_string());
    let mut s = date;
    if let Some(rel) = reset_in(iso) {
        s.push_str(&format!(" · in {rel}"));
    }
    if estimated {
        s.push_str(" · est.");
    }
    s
}

/// Format a past plan-period ISO as `YYYY-MM-DD` (optional ` · est.`).
pub fn format_plan_last_value(iso: &str, estimated: bool) -> String {
    let date = iso_local_date(iso).unwrap_or_else(|| iso.trim().to_string());
    if estimated {
        format!("{date} · est.")
    } else {
        date
    }
}

/// Advance by one calendar month, clamping day-of-month to the target month's
/// length (Jan 31 → Feb 28/29). Preserves time-of-day and offset.
pub fn add_calendar_month(dt: OffsetDateTime) -> OffsetDateTime {
    shift_calendar_months(dt, 1)
}

/// Go back one calendar month (clamping day-of-month).
pub fn sub_calendar_month(dt: OffsetDateTime) -> OffsetDateTime {
    shift_calendar_months(dt, -1)
}

fn shift_calendar_months(dt: OffsetDateTime, delta: i32) -> OffsetDateTime {
    let d = dt.date();
    let mut year = d.year();
    let mut month = d.month() as i32 + delta;
    while month > 12 {
        month -= 12;
        year += 1;
    }
    while month < 1 {
        month += 12;
        year -= 1;
    }
    let month = Month::try_from(month as u8).expect("month 1-12");
    let day = d.day();
    let new_date = (1..=day)
        .rev()
        .find_map(|dom| Date::from_calendar_date(year, month, dom).ok())
        .unwrap_or(d);
    OffsetDateTime::new_in_offset(new_date, dt.time(), dt.offset())
}

/// Given a monthly billing anchor (first charge / subscription create) and
/// `now`, return `(last_period_start, next_period_end)` by walking calendar
/// months (Stripe-style fixed day-of-month with clamp).
pub fn monthly_cycle_bounds(
    anchor: OffsetDateTime,
    now: OffsetDateTime,
) -> (OffsetDateTime, OffsetDateTime) {
    if now < anchor {
        return (sub_calendar_month(anchor), anchor);
    }
    let mut last = anchor;
    let mut next = add_calendar_month(anchor);
    // Cap iterations so a pathological clock cannot hang.
    for _ in 0..2400 {
        if next > now {
            return (last, next);
        }
        last = next;
        next = add_calendar_month(next);
    }
    (last, next)
}

/// Format `OffsetDateTime` as RFC3339, or `None` on failure.
pub fn offset_dt_to_iso(dt: OffsetDateTime) -> Option<String> {
    dt.format(&Rfc3339).ok()
}

/// Parse RFC3339 (or ISO with assumed Z) into `OffsetDateTime`.
pub fn parse_iso_dt(iso: &str) -> Option<OffsetDateTime> {
    let s = iso.trim();
    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc3339) {
        return Some(dt);
    }
    if s.contains('T') && !s.ends_with('Z') && !s.contains('+') {
        let withz = format!("{s}Z");
        return OffsetDateTime::parse(&withz, &Rfc3339).ok();
    }
    None
}

/// Convert a JSON value that may be an ISO string, unix seconds, or unix ms
/// into an RFC3339 string (UTC).
pub fn to_iso(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            // Already RFC3339?
            if OffsetDateTime::parse(s, &Rfc3339).is_ok() {
                return Some(s.to_string());
            }
            // ISO-like missing tz -> assume UTC.
            if s.contains('T') && !s.ends_with('Z') && !s.contains('+') {
                let withz = format!("{s}Z");
                if OffsetDateTime::parse(&withz, &Rfc3339).is_ok() {
                    return Some(withz);
                }
            }
            // Numeric string?
            if let Ok(n) = s.parse::<f64>() {
                return ms_to_iso(numberish_to_ms(n));
            }
            None
        }
        serde_json::Value::Number(n) => {
            let f = n.as_f64()?;
            ms_to_iso(numberish_to_ms(f))
        }
        _ => None,
    }
}

/// Treat values below 1e10 as seconds, otherwise milliseconds.
fn numberish_to_ms(n: f64) -> i64 {
    if n.abs() < 1e10 {
        (n * 1000.0) as i64
    } else {
        n as i64
    }
}

pub fn ms_to_iso(ms: i64) -> Option<String> {
    let secs = ms.div_euclid(1000);
    let nanos = (ms.rem_euclid(1000) * 1_000_000) as i128;
    let total = secs as i128 * 1_000_000_000 + nanos;
    OffsetDateTime::from_unix_timestamp_nanos(total)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

/// Title-case a plan label ("pro" -> "Pro", "team max" -> "Team Max").
pub fn plan_label(value: &str) -> String {
    let v = value.trim();
    if v.is_empty() {
        return String::new();
    }
    v.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Cents -> dollars (rounded to cents).
pub fn cents_to_dollars(cents: f64) -> f64 {
    (cents / 100.0 * 100.0).round() / 100.0
}

/// Format a token count compactly: 1234 -> "1.2K", 4_500_000 -> "4.5M".
pub fn fmt_tokens(n: u64) -> String {
    let n = n as f64;
    for (threshold, divisor, suffix) in [(1e9, 1e9, "B"), (1e6, 1e6, "M"), (1e3, 1e3, "K")] {
        if n >= threshold {
            let scaled = n / divisor;
            return if scaled >= 10.0 {
                format!("{}{}", scaled.round() as u64, suffix)
            } else {
                let s = format!("{scaled:.1}");
                format!("{}{}", s.trim_end_matches(".0"), suffix)
            };
        }
    }
    format!("{}", n.round() as u64)
}

/// Decode a JWT's payload (claims) without verifying the signature.
pub fn jwt_payload(token: &str) -> Option<serde_json::Value> {
    let payload_b64 = token.split('.').nth(1)?;
    let bytes = base64url_decode(payload_b64)?;
    serde_json::from_slice(&bytes).ok()
}

/// Decode a JWT's `exp` claim (unix seconds) without verifying the signature.
pub fn jwt_exp_ms(token: &str) -> Option<i64> {
    let exp = jwt_payload(token)?.get("exp")?.as_f64()?;
    Some((exp * 1000.0) as i64)
}

/// Minimal standard base64 decoder (handles `+/`, optional `=` padding).
/// Returns the decoded bytes as a UTF-8 string (lossy) for token use.
pub fn base64_decode_str(input: &str) -> Option<String> {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = b64_decode_with(input, ALPHA)?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Minimal base64url decoder (no padding required).
fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    b64_decode_with(input, ALPHA)
}

fn b64_decode_with(input: &str, alpha: &[u8; 64]) -> Option<Vec<u8>> {
    let mut lut = [255u8; 256];
    for (i, &c) in alpha.iter().enumerate() {
        lut[c as usize] = i as u8;
    }
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &b in input.as_bytes() {
        if b == b'=' {
            break;
        }
        let v = lut[b as usize];
        if v == 255 {
            return None;
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_iso_passes_through_rfc3339() {
        let v = serde_json::json!("2026-01-02T03:04:05Z");
        assert_eq!(to_iso(&v).as_deref(), Some("2026-01-02T03:04:05Z"));
    }

    #[test]
    fn to_iso_handles_unix_seconds_and_ms() {
        // 1_700_000_000 s == 2023-11-14T22:13:20Z
        let secs = to_iso(&serde_json::json!(1_700_000_000_i64)).unwrap();
        assert!(secs.starts_with("2023-11-14T22:13:20"));
        // same instant in ms
        let ms = to_iso(&serde_json::json!(1_700_000_000_000_i64)).unwrap();
        assert!(ms.starts_with("2023-11-14T22:13:20"));
    }

    #[test]
    fn to_iso_assumes_utc_when_tz_missing() {
        let v = serde_json::json!("2026-01-02T03:04:05");
        assert_eq!(to_iso(&v).as_deref(), Some("2026-01-02T03:04:05Z"));
    }

    #[test]
    fn plan_label_title_cases() {
        assert_eq!(plan_label("pro"), "Pro");
        assert_eq!(plan_label("team max"), "Team Max");
        assert_eq!(plan_label(""), "");
    }

    #[test]
    fn cents_to_dollars_rounds() {
        assert_eq!(cents_to_dollars(12345.0), 123.45);
        assert_eq!(cents_to_dollars(0.0), 0.0);
    }

    #[test]
    fn fmt_tokens_scales() {
        assert_eq!(fmt_tokens(500), "500");
        assert_eq!(fmt_tokens(1500), "1.5K");
        assert_eq!(fmt_tokens(4_500_000), "4.5M");
        assert_eq!(fmt_tokens(1_100_000_000), "1.1B");
        assert_eq!(fmt_tokens(12_000), "12K");
    }

    #[test]
    fn jwt_decodes_payload_and_exp() {
        // {"sub":"google-oauth2|user_abc","exp":1700000000} base64url, unsigned.
        let token = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJnb29nbGUtb2F1dGgyfHVzZXJfYWJjIiwiZXhwIjoxNzAwMDAwMDAwfQ.";
        let payload = jwt_payload(token).expect("payload");
        assert_eq!(
            payload.get("sub").unwrap().as_str().unwrap(),
            "google-oauth2|user_abc"
        );
        assert_eq!(jwt_exp_ms(token), Some(1_700_000_000_000));
    }

    #[test]
    fn base64_decode_str_roundtrips_simple() {
        // "hello" -> aGVsbG8=
        assert_eq!(base64_decode_str("aGVsbG8=").as_deref(), Some("hello"));
    }

    #[test]
    fn reset_in_formats_future_and_rejects_past() {
        assert_eq!(format_duration_secs(2 * 86400 + 4 * 3600), "2d 4h");
        assert_eq!(format_duration_secs(3 * 3600 + 12 * 60), "3h 12m");
        assert_eq!(format_duration_secs(5 * 60), "5m");
        assert_eq!(format_duration_secs(30), "<1m");

        // A timestamp ~2h in the future yields an "Xh" string.
        let future = OffsetDateTime::from_unix_timestamp((now_ms() / 1000) + 7200)
            .unwrap()
            .format(&Rfc3339)
            .unwrap();
        assert!(reset_in(&future).unwrap().contains('h'));

        // Past timestamps are not shown.
        assert!(reset_in("2000-01-01T00:00:00Z").is_none());
        assert!(reset_in("not-a-date").is_none());
    }

    #[test]
    fn local_date_ymd_formats() {
        // With UTC offset (default in tests), 1_700_000_000_000 ms == 2023-11-14.
        let d = local_date_ymd(1_700_000_000_000);
        // Allow for local offset shifting the date by a day either way.
        assert!(d.starts_with("2023-11-1"), "got {d}");
    }

    fn dt(iso: &str) -> OffsetDateTime {
        parse_iso_dt(iso).expect(iso)
    }

    #[test]
    fn add_calendar_month_clamps_day() {
        // Jan 31 → Feb 28 (non-leap)
        let feb = add_calendar_month(dt("2025-01-31T15:18:17Z"));
        assert_eq!(feb.date().to_string(), "2025-02-28");
        assert_eq!(feb.time().hour(), 15);
        // Jan 31 → Feb 29 (leap)
        let leap = add_calendar_month(dt("2024-01-31T12:00:00Z"));
        assert_eq!(leap.date().to_string(), "2024-02-29");
        // Jul 17 → Aug 17
        let aug = add_calendar_month(dt("2025-07-17T15:18:17Z"));
        assert_eq!(aug.date().to_string(), "2025-08-17");
        // Dec → next year
        let jan = add_calendar_month(dt("2025-12-17T00:00:00Z"));
        assert_eq!(jan.date().to_string(), "2026-01-17");
    }

    #[test]
    fn sub_calendar_month_clamps() {
        let jan = sub_calendar_month(dt("2025-03-31T10:00:00Z"));
        assert_eq!(jan.date().to_string(), "2025-02-28");
    }

    #[test]
    fn monthly_cycle_bounds_walks_forward() {
        let anchor = dt("2025-07-17T15:18:17Z");
        let now = dt("2026-07-10T12:00:00Z");
        let (last, next) = monthly_cycle_bounds(anchor, now);
        assert_eq!(last.date().to_string(), "2026-06-17");
        assert_eq!(next.date().to_string(), "2026-07-17");
    }

    #[test]
    fn monthly_cycle_bounds_on_exact_boundary() {
        // When now is exactly a period start, next should be one month later.
        let anchor = dt("2025-07-17T15:18:17Z");
        let now = dt("2026-06-17T15:18:17Z");
        let (last, next) = monthly_cycle_bounds(anchor, now);
        assert_eq!(last.date().to_string(), "2026-06-17");
        assert_eq!(next.date().to_string(), "2026-07-17");
    }

    #[test]
    fn format_plan_values_estimated() {
        let past = "2026-06-17T15:18:17Z";
        assert_eq!(format_plan_last_value(past, true), "2026-06-17 · est.");
        assert_eq!(format_plan_last_value(past, false), "2026-06-17");

        let future = OffsetDateTime::from_unix_timestamp((now_ms() / 1000) + 3 * 86400)
            .unwrap()
            .format(&Rfc3339)
            .unwrap();
        let renew = format_plan_renew_value(&future, true);
        assert!(renew.contains(" · in "), "{renew}");
        assert!(renew.ends_with(" · est."), "{renew}");
    }
}
