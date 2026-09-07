//! Shared autosteer pick: skip exhausted, urgent reset, plan rank, finish the week.
//!
//! Grok callers pass [`grok_burn_rank`] (smaller pool first). Cursor callers
//! pass [`cursor_plan_rank`] (higher tier first). [`pick_autosteer`] always
//! prefers the higher numeric rank.

pub const URGENT_RESET_HOURS: f64 = 12.0;
pub const DEFAULT_EXHAUSTED_PCT: f64 = 100.0;
/// Max value of [`grok_plan_rank`] (SuperGrok Heavy). Used to invert burn order.
pub const GROK_PLAN_RANK_MAX: u8 = 5;

pub trait Steerable {
    fn used_pct(&self) -> Option<f64>;
    fn plan_slug(&self) -> Option<&str>;
    fn resets_at(&self) -> Option<&str>;
}

impl Steerable for crate::Account {
    fn used_pct(&self) -> Option<f64> {
        self.used_pct
    }
    fn plan_slug(&self) -> Option<&str> {
        self.plan_slug.as_deref()
    }
    fn resets_at(&self) -> Option<&str> {
        self.resets_at.as_deref()
    }
}

pub fn grok_plan_rank(slug: &str) -> u8 {
    match slug {
        "heavy" => 5,
        "plus" => 4,
        "normal" => 3,
        "lite" => 2,
        "premium-plus" => 1,
        "premium" => 0,
        _ => 0,
    }
}

/// Invert [`grok_plan_rank`]: burn smaller SuperGrok / X pools first; Heavy is reserve.
pub fn grok_burn_rank(slug: &str) -> u8 {
    GROK_PLAN_RANK_MAX.saturating_sub(grok_plan_rank(slug))
}

pub fn cursor_plan_rank(slug: &str) -> u8 {
    match slug {
        "business" | "enterprise" | "ultra" => 4,
        "pro" | "pro-plus" => 3,
        "plus" => 2,
        "hobby" | "free" => 1,
        _ => 0,
    }
}

pub fn hours_to_reset(iso: Option<&str>, now_ms: i64) -> Option<f64> {
    let ms = parse_rfc3339_ms(iso?)?;
    Some(((ms - now_ms) as f64 / 3_600_000.0).max(0.0))
}

/// Compact duration until reset: `now`, `40m`, `18h`, `4d 2h`.
pub fn format_reset_in(hours: f64) -> String {
    let h = hours.max(0.0);
    if h < 1.0 / 60.0 {
        return "now".into();
    }
    if h < 1.0 {
        let m = (h * 60.0).round().max(1.0) as u32;
        return format!("{m}m");
    }
    if h < 48.0 {
        return format!("{:.0}h", h);
    }
    let days = (h / 24.0).floor() as u32;
    let rem = ((h - f64::from(days) * 24.0).round() as i32).max(0) as u32;
    if rem == 0 {
        format!("{days}d")
    } else {
        format!("{days}d {rem}h")
    }
}

/// Skip exhausted; prefer reset &lt; 12h, then higher rank, then higher used %.
/// Cursor / grok-bot use this (higher plan tier first).
pub fn pick_autosteer<'a, T: Steerable>(
    accounts: &'a [T],
    exhausted_pct: f64,
    now_ms: i64,
    rank: impl Fn(&T) -> u8,
) -> Option<&'a T> {
    pick_by_score(accounts, exhausted_pct, |a| {
        plan_first_score(
            a.used_pct().unwrap_or(0.0),
            hours_to_reset(a.resets_at(), now_ms),
            rank(a),
        )
    })
}

/// Grok: soonest reset first, then smaller plan (`rank`), then higher used %.
pub fn pick_deadline_autosteer<'a, T: Steerable>(
    accounts: &'a [T],
    exhausted_pct: f64,
    now_ms: i64,
    rank: impl Fn(&T) -> u8,
) -> Option<&'a T> {
    pick_by_score(accounts, exhausted_pct, |a| {
        deadline_first_score(
            a.used_pct().unwrap_or(0.0),
            hours_to_reset(a.resets_at(), now_ms),
            rank(a),
            None,
        )
    })
}

fn pick_by_score<'a, T: Steerable>(
    accounts: &'a [T],
    exhausted_pct: f64,
    score: impl Fn(&T) -> f64,
) -> Option<&'a T> {
    let mut best: Option<(f64, &'a T)> = None;
    for a in accounts {
        let used = a.used_pct().unwrap_or(0.0);
        if used >= exhausted_pct {
            continue;
        }
        let s = score(a);
        match &best {
            Some((best_s, _)) if *best_s >= s => {}
            _ => best = Some((s, a)),
        }
    }
    best.map(|(_, a)| a)
}

pub fn plan_first_score(used: f64, hours: Option<f64>, rank: u8) -> f64 {
    let urgent = hours.map(|h| h < URGENT_RESET_HOURS).unwrap_or(false);
    let reset_term = hours.map(|h| 1.0 / h.max(0.01)).unwrap_or(0.0);
    (if urgent { 1_000_000.0 } else { 0.0 })
        + 10_000.0 * f64::from(rank)
        + 100.0 * used
        + reset_term
}

/// `pct_per_hour`: observed pool-points/hour while this account was active.
/// At-risk (cannot finish before reset) is scored as due now.
/// Missing `hours` is due now so unknown resets are not deferred.
pub fn deadline_first_score(
    used: f64,
    hours: Option<f64>,
    rank: u8,
    pct_per_hour: Option<f64>,
) -> f64 {
    let remaining = (100.0 - used).max(0.0);
    let at_risk = match (hours, pct_per_hour) {
        (Some(h), Some(rate)) if rate > 1e-6 => remaining / rate > h,
        _ => false,
    };
    let h = if at_risk { 0.0 } else { hours.unwrap_or(0.0) };
    1e12 / h.max(0.01) + 10_000.0 * f64::from(rank) + 100.0 * used
}

pub fn parse_rfc3339_ms(iso: &str) -> Option<i64> {
    let s = iso.trim();
    let (date, time_and_off) = s.split_once('T')?;
    let mut dp = date.split('-');
    let y: i32 = dp.next()?.parse().ok()?;
    let mo: u32 = dp.next()?.parse().ok()?;
    let d: u32 = dp.next()?.parse().ok()?;
    let rest = time_and_off.trim_end_matches('Z');
    let time = rest
        .split_once('+')
        .or_else(|| {
            rest.rfind('-').and_then(|i| {
                if i > 2 {
                    Some((&rest[..i], &rest[i + 1..]))
                } else {
                    None
                }
            })
        })
        .map(|(t, _)| t)
        .unwrap_or(rest);
    let time = time.split('.').next()?;
    let mut tp = time.split(':');
    let h: u32 = tp.next()?.parse().ok()?;
    let mi: u32 = tp.next()?.parse().ok()?;
    let se: u32 = tp.next()?.parse().ok()?;
    civil_unix_ms(y, mo, d, h, mi, se)
}

fn leap(y: i32) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn civil_unix_ms(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Option<i64> {
    if !(1..=12).contains(&mo) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let md = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let dim = if mo == 2 && leap(y) {
        29
    } else {
        md[mo as usize]
    };
    if d < 1 || d > dim {
        return None;
    }
    let mut days: i64 = 0;
    if y >= 1970 {
        for yy in 1970..y {
            days += if leap(yy) { 366 } else { 365 };
        }
    } else {
        for yy in y..1970 {
            days -= if leap(yy) { 366 } else { 365 };
        }
    }
    for m in 1..mo {
        days += if m == 2 && leap(y) {
            29
        } else {
            md[m as usize]
        } as i64;
    }
    days += i64::from(d) - 1;
    Some(
        days * 86_400_000 + i64::from(h) * 3_600_000 + i64::from(mi) * 60_000 + i64::from(s) * 1000,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Account;

    fn acc(alias: &str, slug: &str, used: f64, resets_at: Option<&str>) -> Account {
        let mut a = Account::new("grok", alias).unwrap();
        a.plan_slug = Some(slug.into());
        a.used_pct = Some(used);
        a.resets_at = resets_at.map(|s| s.into());
        a
    }

    #[test]
    fn unix_epoch_ms_is_zero() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            hours_to_reset(Some("1970-01-01T02:00:00Z"), 0).unwrap(),
            2.0
        );
    }

    #[test]
    fn grok_burn_rank_inverts_quality() {
        assert_eq!(grok_burn_rank("heavy"), 0);
        assert_eq!(grok_burn_rank("plus"), 1);
        assert_eq!(grok_burn_rank("premium-plus"), 4);
        assert_eq!(grok_burn_rank("premium"), 5);
    }

    #[test]
    fn format_reset_in_compacts() {
        assert_eq!(format_reset_in(0.0), "now");
        assert_eq!(format_reset_in(0.5), "30m");
        assert_eq!(format_reset_in(18.0), "18h");
        assert_eq!(format_reset_in(47.0), "47h");
        assert_eq!(format_reset_in(4.0 * 24.0), "4d");
        assert_eq!(format_reset_in(4.0 * 24.0 + 2.0), "4d 2h");
    }

    #[test]
    fn autosteer_skips_exhausted_prefers_plan() {
        let now = parse_rfc3339_ms("2026-01-01T00:00:00Z").unwrap();
        let list = vec![
            acc("heavy", "heavy", 100.0, Some("2026-01-04T08:00:00Z")),
            acc("plus", "plus", 40.0, Some("2026-01-04T08:00:00Z")),
        ];
        let pick = pick_deadline_autosteer(&list, 100.0, now, |a| {
            grok_burn_rank(a.plan_slug.as_deref().unwrap())
        });
        assert_eq!(pick.unwrap().alias, "plus");
    }

    #[test]
    fn autosteer_burns_premium_plus_before_live_heavy() {
        let now = parse_rfc3339_ms("2026-01-01T00:00:00Z").unwrap();
        let list = vec![
            acc("heavy-2", "heavy", 3.0, Some("2026-01-08T00:00:00Z")),
            acc(
                "premium-plus-1",
                "premium-plus",
                0.0,
                Some("2026-01-08T00:00:00Z"),
            ),
            acc("heavy-1", "heavy", 100.0, Some("2026-01-08T00:00:00Z")),
        ];
        let pick = pick_deadline_autosteer(&list, 100.0, now, |a| {
            grok_burn_rank(a.plan_slug.as_deref().unwrap())
        });
        assert_eq!(pick.unwrap().alias, "premium-plus-1");
    }

    #[test]
    fn deadline_picks_sooner_reset_even_if_heavier_plan() {
        let now = parse_rfc3339_ms("2026-01-01T00:00:00Z").unwrap();
        let list = vec![
            acc(
                "premium-plus-1",
                "premium-plus",
                0.0,
                Some("2026-01-06T00:00:00Z"),
            ),
            acc("heavy-2", "heavy", 3.0, Some("2026-01-03T00:00:00Z")),
        ];
        let pick = pick_deadline_autosteer(&list, 100.0, now, |a| {
            grok_burn_rank(a.plan_slug.as_deref().unwrap())
        });
        assert_eq!(pick.unwrap().alias, "heavy-2");
    }

    #[test]
    fn autosteer_urgent_lite_beats_fresh_heavy() {
        let now = parse_rfc3339_ms("2026-01-01T00:00:00Z").unwrap();
        let list = vec![
            acc("heavy", "heavy", 20.0, Some("2026-01-06T00:00:00Z")),
            acc("lite", "lite", 90.0, Some("2026-01-01T02:00:00Z")),
        ];
        let pick = pick_deadline_autosteer(&list, 100.0, now, |a| {
            grok_burn_rank(a.plan_slug.as_deref().unwrap())
        });
        assert_eq!(pick.unwrap().alias, "lite");
    }

    #[test]
    fn at_risk_rate_beats_later_reset() {
        let used = 10.0;
        let hours = Some(2.0);
        let slow = deadline_first_score(used, hours, 0, Some(1.0));
        let fast_ok = deadline_first_score(used, Some(48.0), 4, Some(50.0));
        assert!(slow > fast_ok, "{slow} vs {fast_ok}");
    }

    #[test]
    fn autosteer_all_full_none() {
        let now = parse_rfc3339_ms("2026-01-01T00:00:00Z").unwrap();
        let list = vec![
            acc("a", "heavy", 100.0, Some("2026-01-01T10:00:00Z")),
            acc("b", "plus", 100.0, Some("2026-01-01T10:00:00Z")),
        ];
        assert!(pick_autosteer(&list, 100.0, now, |_| 1).is_none());
    }
}
