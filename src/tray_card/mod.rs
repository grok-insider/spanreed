#![cfg_attr(not(feature = "tray"), allow(dead_code))]
//! CodexBar-style tray card: quota pace, reset credits, and local cost.
//!
//! Pure data and HTML. The `tray` feature hosts this document in a borderless
//! window on macOS, Windows, and Linux. Colors are the Fabrials dark/light tokens.

use fabrials_types::{Availability, Freshness, ResetInventory};

use crate::cost::CostSummary;
use crate::model::{MetricLine, ProgressFormat, ProviderOutput};
use crate::util;

mod html;

#[cfg(test)]
use html::needs_attention;
pub use html::render;

const SESSION_MINUTES: i64 = 300;
const WEEKLY_MINUTES: i64 = 10_080;

#[derive(Debug, Clone, PartialEq)]
pub struct Pace {
    pub expected_used: f64,
    pub delta: f64,
    pub left_label: String,
    pub right_label: Option<String>,
    /// Marker on a remaining-percent bar. Absent when the window is on pace.
    pub marker_remaining: Option<f64>,
    /// Using the pool faster than the window expects.
    pub deficit: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Meter {
    pub title: String,
    pub reset_text: Option<String>,
    /// Milliseconds until this window resets.
    pub resets_in_ms: Option<i64>,
    /// Fill of the bar, 0–100. Quota meters show percent left.
    pub fill: f64,
    /// `fill` is percent remaining. Dollar and count meters store percent used.
    pub shows_left: bool,
    pub left: String,
    pub pace_left: Option<String>,
    pub pace_right: Option<String>,
    pub marker: Option<f64>,
    pub deficit: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub title: String,
    pub value: String,
    pub emphasis: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChartBar {
    pub label: String,
    pub value: f64,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrayCard {
    pub provider_id: String,
    pub name: String,
    pub account: String,
    pub plan: String,
    pub updated: String,
    pub error: Option<String>,
    pub meters: Vec<Meter>,
    pub reset_credits: Option<String>,
    pub reset_expiry: Option<String>,
    /// Codex-only spend action. True only for a fresh available credit.
    pub can_use_reset: bool,
    pub stats: Vec<Stat>,
    pub bars: Vec<ChartBar>,
    pub notes: Vec<String>,
    pub credits_left: Option<String>,
    pub credits_scale: Option<String>,
    pub credits_fill: Option<f64>,
    pub buy_url: Option<String>,
}

pub struct CardSources<'a> {
    pub output: &'a ProviderOutput,
    pub account: Option<&'a str>,
    pub cost: Option<&'a CostSummary>,
    pub now_ms: i64,
    pub updated_ms: i64,
}

pub fn pace(used: f64, resets_at_ms: i64, window_minutes: i64, now_ms: i64) -> Option<Pace> {
    if window_minutes <= 0 {
        return None;
    }
    let duration = window_minutes.saturating_mul(60_000);
    let until = resets_at_ms.saturating_sub(now_ms);
    if until <= 0 || until > duration {
        return None;
    }
    let elapsed = duration - until;
    let actual = used.clamp(0.0, 100.0);
    if elapsed == 0 && actual > 0.0 {
        return None;
    }
    let expected = (elapsed as f64 / duration as f64 * 100.0).clamp(0.0, 100.0);
    if expected < 3.0 && actual < 100.0 {
        return None;
    }
    let delta = actual - expected;
    let right_label = if actual >= 100.0 {
        Some("Runs out now".into())
    } else if elapsed > 0 && actual > 0.0 {
        let rate = actual / elapsed as f64;
        let candidate = (100.0 - actual) / rate;
        if candidate >= until as f64 {
            Some("Lasts until reset".into())
        } else {
            Some(format!("Runs out in {}", duration_text(candidate as i64)))
        }
    } else {
        Some("Lasts until reset".into())
    };
    let abs_delta = delta.abs();
    let left_label = if abs_delta <= 2.0 {
        "On pace".into()
    } else if delta < 0.0 {
        format!("{}% in reserve", abs_delta.round() as i64)
    } else {
        format!("{}% in deficit", abs_delta.round() as i64)
    };
    let on_track = abs_delta <= 2.0;
    Some(Pace {
        expected_used: expected,
        delta,
        left_label,
        right_label,
        marker_remaining: if on_track {
            None
        } else {
            Some((100.0 - expected).clamp(0.0, 100.0))
        },
        deficit: actual > expected,
    })
}

pub fn build(source: CardSources<'_>) -> TrayCard {
    let out = source.output;
    let error = out.lines.iter().find_map(|line| match line {
        MetricLine::Badge { text, .. } if line.is_error() => Some(text.clone()),
        _ => None,
    });
    let mut meters = Vec::new();
    let mut credits_text = None;
    let mut notes = Vec::new();
    for line in &out.lines {
        match line {
            MetricLine::Progress {
                label,
                used,
                limit,
                format,
                resets_at,
                ..
            } => {
                let progress = Progress {
                    label,
                    used: *used,
                    limit: *limit,
                    format,
                    resets_at: resets_at.as_deref(),
                };
                if let Some(meter) = meter(progress, source.now_ms) {
                    meters.push(meter);
                }
            }
            MetricLine::Text { label, value, .. } if label.eq_ignore_ascii_case("Credits") => {
                credits_text = Some(value.clone());
            }
            MetricLine::Text { label, value, .. }
                if label.eq_ignore_ascii_case("Plan")
                    || label.to_lowercase().starts_with("reset ") => {}
            MetricLine::BarChart {
                note: Some(note), ..
            } => notes.push(note.clone()),
            _ => {}
        }
    }
    let (reset_credits, reset_expiry) = reset_copy(out, source.now_ms);
    let can_use_reset = can_use_reset(out);
    let (stats, bars, top_model, partial) = cost_view(source.cost);
    if let Some(model) = top_model {
        notes.insert(0, format!("Top model: {model}"));
    }
    if partial {
        notes.push("Estimated from local logs for the selected account".into());
    }
    let (credits_left, credits_fill, credits_scale) = credits_meter(credits_text.as_deref());
    TrayCard {
        provider_id: out.provider_id.clone(),
        name: out.display_name.clone(),
        account: source.account.unwrap_or("").to_string(),
        plan: out.plan.clone().unwrap_or_default(),
        updated: updated_text(source.now_ms.saturating_sub(source.updated_ms)),
        error,
        meters,
        reset_credits,
        reset_expiry,
        can_use_reset,
        stats,
        bars,
        notes,
        credits_left,
        credits_scale,
        credits_fill,
        buy_url: buy_url(&out.provider_id),
    }
}

/// Build the tray document from the latest probe. Cost reads local logs only.
pub fn present(
    outputs: &[ProviderOutput],
    capture_up: bool,
    status: Option<&str>,
    now_ms: i64,
    pricing: &crate::pricing::PricingMap,
) -> String {
    let cards = outputs
        .iter()
        .map(|output| {
            let account = account_label(output);
            let cost = local_cost(output, pricing);
            build(CardSources {
                output,
                account: account.as_deref(),
                cost: cost.as_ref(),
                now_ms,
                updated_ms: now_ms,
            })
        })
        .collect::<Vec<_>>();
    render(&cards, capture_up, status)
}

fn account_label(output: &ProviderOutput) -> Option<String> {
    match output.provider_id.split('/').next().unwrap_or("") {
        "codex" => crate::providers::codex::account_label(),
        _ => None,
    }
}

fn local_cost(
    output: &ProviderOutput,
    pricing: &crate::pricing::PricingMap,
) -> Option<CostSummary> {
    let source = crate::cost::Source::from_provider(output.provider_id.split('/').next()?)?;
    crate::cost::estimate(source, pricing)
}

/// The fields of a `MetricLine::Progress`.
struct Progress<'a> {
    label: &'a str,
    used: f64,
    limit: f64,
    format: &'a ProgressFormat,
    resets_at: Option<&'a str>,
}

fn meter(progress: Progress<'_>, now_ms: i64) -> Option<Meter> {
    let Progress {
        label,
        used,
        limit,
        format,
        resets_at,
    } = progress;
    let resets_ms = resets_at.and_then(|iso| {
        util::parse_iso_dt(iso).map(|dt| (dt.unix_timestamp_nanos() / 1_000_000) as i64)
    });
    let resets_in_ms = resets_ms.filter(|ms| *ms > now_ms).map(|ms| ms - now_ms);
    let reset_text = resets_in_ms.map(|ms| format!("Resets in {}", duration_text(ms)));
    match format {
        ProgressFormat::Percent => {
            let used = used.clamp(0.0, 100.0);
            let pace = window_minutes(label)
                .and_then(|minutes| resets_ms.and_then(|ms| pace(used, ms, minutes, now_ms)));
            Some(Meter {
                title: label.to_string(),
                reset_text,
                resets_in_ms,
                fill: (100.0 - used).clamp(0.0, 100.0),
                shows_left: true,
                left: format!("{}% left", (100.0 - used).round() as i64),
                pace_left: pace.as_ref().map(|pace| pace.left_label.clone()),
                pace_right: pace.as_ref().and_then(|pace| pace.right_label.clone()),
                marker: pace.as_ref().and_then(|pace| pace.marker_remaining),
                deficit: pace.as_ref().map(|pace| pace.deficit).unwrap_or(false),
            })
        }
        ProgressFormat::Dollars => {
            let fill = if limit > 0.0 {
                (used / limit * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            Some(Meter {
                title: label.to_string(),
                reset_text,
                resets_in_ms,
                fill,
                shows_left: false,
                left: format!("${used:.2} / ${limit:.2}"),
                pace_left: None,
                pace_right: None,
                marker: None,
                deficit: fill >= 95.0,
            })
        }
        ProgressFormat::Count { suffix } => {
            let fill = if limit > 0.0 {
                (used / limit * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            Some(Meter {
                title: label.to_string(),
                reset_text,
                resets_in_ms,
                fill,
                shows_left: false,
                left: format!("{used:.0}/{limit:.0} {suffix}"),
                pace_left: None,
                pace_right: None,
                marker: None,
                deficit: fill >= 95.0,
            })
        }
    }
}

fn window_minutes(label: &str) -> Option<i64> {
    let label = label.to_ascii_lowercase();
    if label.contains("session") || label == "5h" || label.contains("5 hour") {
        Some(SESSION_MINUTES)
    } else if label.contains("week") {
        Some(WEEKLY_MINUTES)
    } else {
        None
    }
}

/// A reset can be spent only when this Codex observation still reports one.
pub fn can_use_reset(output: &ProviderOutput) -> bool {
    if output.provider_id.split('/').next().unwrap_or("") != "codex" {
        return false;
    }
    let Some(observed) = &output.reset_inventory else {
        return false;
    };
    if observed.availability != Availability::Available || observed.freshness != Freshness::Fresh {
        return false;
    }
    observed
        .value
        .as_ref()
        .is_some_and(|inventory| inventory.available > 0)
}

fn reset_copy(output: &ProviderOutput, now_ms: i64) -> (Option<String>, Option<String>) {
    let Some(observed) = &output.reset_inventory else {
        return (None, None);
    };
    let Some(inventory) = &observed.value else {
        return (None, None);
    };
    let current = observed.freshness == Freshness::Fresh;
    let noun = if current {
        "available"
    } else {
        "last reported"
    };
    let count = if inventory.available == 1 {
        format!("1 reset {noun}")
    } else {
        format!("{} resets {noun}", inventory.available)
    };
    let expiry = next_expiry(inventory, now_ms);
    (Some(count), expiry)
}

fn next_expiry(inventory: &ResetInventory, now_ms: i64) -> Option<String> {
    let mut soonest: Option<i64> = None;
    for credit in &inventory.credits {
        let Some(expires) = credit.expires_at_ms else {
            continue;
        };
        if expires <= now_ms {
            continue;
        }
        soonest = Some(
            soonest
                .map(|current| current.min(expires))
                .unwrap_or(expires),
        );
    }
    soonest.map(|ms| format!("Next expires in {}", duration_text(ms - now_ms)))
}

fn cost_view(cost: Option<&CostSummary>) -> (Vec<Stat>, Vec<ChartBar>, Option<String>, bool) {
    let Some(cost) = cost else {
        return (Vec::new(), Vec::new(), None, false);
    };
    if cost.total_tokens == 0 && cost.daily.is_empty() {
        return (Vec::new(), Vec::new(), None, cost.partial);
    }
    let today = cost.daily.last();
    let latest_tokens = today.map(|day| day.tokens).unwrap_or(0);
    let stats = vec![
        Stat {
            title: "Today".into(),
            value: money(today.map(|day| day.cost).unwrap_or(0.0)),
            emphasis: true,
        },
        Stat {
            title: "Last 31 days Cost".into(),
            value: money(cost.total_cost),
            emphasis: true,
        },
        Stat {
            title: "Last 31 days tokens".into(),
            value: util::fmt_tokens(cost.total_tokens),
            emphasis: false,
        },
        Stat {
            title: "Latest tokens".into(),
            value: util::fmt_tokens(latest_tokens),
            emphasis: false,
        },
    ];
    let max = cost
        .daily
        .iter()
        .map(|day| day.cost)
        .fold(0.0_f64, f64::max)
        .max(0.01);
    let bars = cost
        .daily
        .iter()
        .map(|day| ChartBar {
            label: day.date.clone(),
            value: (day.cost / max * 100.0).clamp(0.0, 100.0),
            detail: format!("{} · {}", day.date, money(day.cost)),
        })
        .collect();
    let top = cost.by_model.first().map(|model| model.model.clone());
    (stats, bars, top, cost.partial || cost.total_cost > 0.0)
}

fn credits_meter(text: Option<&str>) -> (Option<String>, Option<f64>, Option<String>) {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return (None, None, None);
    };
    if text.eq_ignore_ascii_case("unlimited") {
        return (Some("Unlimited".into()), Some(100.0), None);
    }
    let digits: String = text
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();
    let amount = digits
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.0);
    let Some(amount) = amount else {
        return (Some(text.to_string()), None, None);
    };
    let scale = if amount <= 0.0 {
        1_000.0
    } else {
        10f64.powf(amount.log10().floor() + 1.0)
    };
    let fill = (amount / scale * 100.0).clamp(0.0, 100.0);
    let scale_label = if scale >= 1000.0 {
        format!("{} tokens", util::fmt_tokens(scale as u64))
    } else {
        format!("{scale:.0} tokens")
    };
    (
        Some(format!("{amount:.0} left")),
        Some(fill),
        Some(scale_label),
    )
}

fn buy_url(provider_id: &str) -> Option<String> {
    let id = provider_id.split('/').next().unwrap_or(provider_id);
    match id {
        "codex" => Some("https://chatgpt.com/".into()),
        "claude" => Some("https://claude.ai/settings/billing".into()),
        "grok" => Some("https://grok.com/".into()),
        "copilot" => Some("https://github.com/settings/copilot".into()),
        _ => None,
    }
}

fn duration_text(ms: i64) -> String {
    let minutes = (ms.max(0) / 60_000).max(0);
    if minutes >= 1_440 {
        let days = minutes / 1_440;
        let hours = (minutes / 60) % 24;
        if hours == 0 {
            format!("{days}d")
        } else {
            format!("{days}d {hours}h")
        }
    } else if minutes >= 60 {
        let hours = minutes / 60;
        let rest = minutes % 60;
        if rest == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {rest}m")
        }
    } else if minutes == 0 {
        "now".into()
    } else {
        format!("{minutes}m")
    }
}

fn updated_text(age_ms: i64) -> String {
    let minutes = age_ms.max(0) / 60_000;
    if minutes < 1 {
        "Updated just now".into()
    } else if minutes < 60 {
        format!("Updated {minutes}m ago")
    } else {
        format!("Updated {}h ago", minutes / 60)
    }
}

fn money(amount: f64) -> String {
    let negative = amount < 0.0;
    let cents = (amount.abs() * 100.0).round() as i64;
    let whole = cents / 100;
    let frac = cents % 100;
    let text = whole.to_string();
    let mut grouped = String::new();
    for (index, ch) in text.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let body: String = grouped.chars().rev().collect();
    let sign = if negative { "-" } else { "" };
    format!("${sign}{body}.{frac:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::DayCost;
    use crate::model::MetricLine;
    use crate::usage_stats::ModelCost;
    use fabrials_types::{Availability, Observation, ResetCredit};

    fn table() -> crate::pricing::PricingMap {
        crate::pricing::table_from(None, None)
    }

    fn output() -> ProviderOutput {
        ProviderOutput::new(
            "codex",
            "Codex",
            vec![
                MetricLine::percent("Session", 40.0, Some("2026-06-28T12:00:00Z".into())),
                MetricLine::percent("Weekly", 27.0, Some("2026-07-03T14:00:00Z".into())),
                MetricLine::text(crate::model::MetricKind::Plan, "Credits", "0"),
            ],
        )
        .with_plan(Some("Plus".into()))
    }

    #[test]
    fn reserve_when_usage_is_behind_the_window() {
        let now = 1_750_000_000_000;
        let start = now - 60 * 60_000;
        let resets = start + SESSION_MINUTES * 60_000;
        let pace = pace(10.0, resets, SESSION_MINUTES, now).unwrap();
        assert!(pace.left_label.contains("reserve"), "{}", pace.left_label);
        assert_eq!(pace.right_label.as_deref(), Some("Lasts until reset"));
        assert!(pace.marker_remaining.is_some());
        assert!(!pace.deficit);
    }

    #[test]
    fn weekly_run_out_when_ahead_of_pace() {
        let now = 1_750_000_000_000;
        let resets = now + 5 * 86_400_000;
        let pace = pace(80.0, resets, WEEKLY_MINUTES, now).unwrap();
        assert!(pace.left_label.contains("deficit"), "{}", pace.left_label);
        assert!(pace.deficit);
        let right = pace.right_label.unwrap();
        assert!(right.starts_with("Runs out"), "{right}");
    }

    #[test]
    fn card_matches_codex_sections_and_escapes() {
        let mut sample = output();
        sample.reset_inventory = Some(Observation {
            availability: Availability::Available,
            freshness: Freshness::Fresh,
            observed_at_ms: Some(1),
            source: "test".into(),
            value: Some(ResetInventory {
                available: 1,
                credits: vec![ResetCredit {
                    valid_from_ms: None,
                    expires_at_ms: Some(1_782_000_000_000),
                }],
                details_complete: true,
            }),
            error: None,
        });
        sample.display_name = "Codex <script>".into();
        let now = 1_782_000_000_000 - 28 * 86_400_000;
        let cost = CostSummary {
            total_cost: 5251.23,
            total_tokens: 5_900_000_000,
            partial: true,
            daily: vec![DayCost {
                date: "2026-06-28".into(),
                cost: 51.56,
                tokens: 85_000_000,
            }],
            by_model: vec![ModelCost {
                model: "Example-1".into(),
                tokens: 85_000_000,
                cost: 51.56,
                input: 1,
                output: 1,
                cache_read: 0,
                cache_create: 0,
            }],
            cache: Default::default(),
        };
        let card = build(CardSources {
            output: &sample,
            account: Some("user@example.com"),
            cost: Some(&cost),
            now_ms: now,
            updated_ms: now,
        });
        assert_eq!(card.plan, "Plus");
        assert_eq!(card.meters.len(), 2);
        assert_eq!(card.meters[0].left, "60% left");
        assert_eq!(card.meters[1].left, "73% left");
        assert_eq!(card.reset_credits.as_deref(), Some("1 reset available"));
        assert!(card.reset_expiry.as_deref().unwrap().contains("28d"));
        assert_eq!(card.stats[0].value, "$51.56");
        assert_eq!(card.stats[1].value, "$5,251.23");
        assert_eq!(card.stats[2].value, "5.9B");
        assert_eq!(card.stats[3].value, "85M");
        assert_eq!(card.credits_left.as_deref(), Some("0 left"));
        assert_eq!(card.buy_url.as_deref(), Some("https://chatgpt.com/"));
        assert!(card.can_use_reset);
        let html = render(&[card], true, None);
        if let Ok(path) = std::env::var("SPANREED_TRAY_PREVIEW") {
            std::fs::write(path, &html).expect("preview");
        }
        assert!(html.contains("Limit Reset Credits"));
        assert!(html.contains("Last 31 days Cost"));
        assert!(html.contains("Top model: Example-1"));
        assert!(html.contains("Buy credits"));
        assert!(html.contains("data-reset=\"user@example.com\""));
        assert!(html.contains("Use a limit reset?"));
        assert!(html.contains("This cannot be undone."));
        assert!(html.contains("data-reset-confirm"));
        assert!(html.contains("--brand:"));
        assert!(html.contains("data-act=\"refresh\""));
        assert!(html.contains("data-act=\"dashboard\""));
        assert!(html.contains("data-act=\"settings\""));
        assert!(html.contains("Codex &lt;script&gt;"));
        assert!(!html.contains("Codex <script>"));
        assert!(html.contains("<title>OpenAI</title>"));
    }

    #[test]
    fn status_line_is_only_present_while_set() {
        let shown = present(&[], true, Some("Reset used"), 0, &table());
        assert!(shown.contains("data-act=\"dashboard\""));
        assert!(shown.contains("data-act=\"settings\""));
        assert!(!shown.contains("Reset used"));
        assert!(!shown.contains("class=\"status\""));
        let cleared = present(&[], true, None, 0, &table());
        assert!(!cleared.contains("Reset used"));
        assert!(!cleared.contains("class=\"status\""));
    }

    fn inventory(available: u32, fresh: Freshness) -> Observation<ResetInventory> {
        Observation {
            availability: Availability::Available,
            freshness: fresh,
            observed_at_ms: Some(1),
            source: "test".into(),
            value: Some(ResetInventory {
                available,
                credits: vec![],
                details_complete: true,
            }),
            error: None,
        }
    }

    #[test]
    fn use_reset_requires_a_fresh_codex_credit() {
        let now = 1_700_000_000_000;
        let mut sample = output();
        sample.reset_inventory = Some(inventory(1, Freshness::Fresh));
        let card = build(CardSources {
            output: &sample,
            account: Some("user@example.com"),
            cost: None,
            now_ms: now,
            updated_ms: now,
        });
        assert!(card.can_use_reset);
        let html = render(&[card], true, None);
        assert!(html.contains(">Use reset</button>"));
        let open = html.find("data-reset=").unwrap();
        let confirm = html.find("post(\"reset\")").unwrap();
        assert!(open < confirm);

        sample.reset_inventory = Some(inventory(0, Freshness::Fresh));
        let empty = build(CardSources {
            output: &sample,
            account: None,
            cost: None,
            now_ms: now,
            updated_ms: now,
        });
        assert!(!empty.can_use_reset);
        assert!(!render(&[empty], true, None).contains("data-reset="));

        sample.reset_inventory = Some(inventory(2, Freshness::Stale));
        let stale = build(CardSources {
            output: &sample,
            account: None,
            cost: None,
            now_ms: now,
            updated_ms: now,
        });
        assert_eq!(
            stale.reset_credits.as_deref(),
            Some("2 resets last reported")
        );
        assert!(!stale.can_use_reset);

        sample.provider_id = "grok".into();
        sample.reset_inventory = Some(inventory(1, Freshness::Fresh));
        let grok = build(CardSources {
            output: &sample,
            account: None,
            cost: None,
            now_ms: now,
            updated_ms: now,
        });
        assert!(!grok.can_use_reset);
    }

    fn quota(id: &str, name: &str, plan: &str, session: f64, weekly: f64, now: i64) -> TrayCard {
        let session_at = crate::util::ms_to_iso(now + 2 * 3_600_000).unwrap();
        let weekly_at = crate::util::ms_to_iso(now + 3 * 86_400_000).unwrap();
        let sample = ProviderOutput::new(
            id,
            name,
            vec![
                MetricLine::percent("Session", session, Some(session_at)),
                MetricLine::percent("Weekly", weekly, Some(weekly_at)),
            ],
        )
        .with_plan(Some(plan.into()));
        build(CardSources {
            output: &sample,
            account: None,
            cost: None,
            now_ms: now,
            updated_ms: now,
        })
    }

    #[test]
    fn menu_splits_attention_from_room_and_lists_resets() {
        let now = 1_750_000_000_000;
        let cards = vec![
            quota("claude", "Claude", "Max", 78.0, 40.0, now),
            quota("codex", "Codex", "Plus", 37.0, 27.0, now),
            quota("cursor", "Cursor", "Pro", 46.0, 20.0, now),
        ];
        assert!(needs_attention(&cards[0]));
        assert!(!needs_attention(&cards[1]));
        let html = render(&cards, true, None);
        assert!(html.contains("class=\"switch on\""));
        assert!(
            html.contains("data-panel=\"0\" class=\"panel on\"")
                || html.contains("class=\"panel on\"")
        );
        assert!(html.contains("% used"));
        assert!(html.contains("Resets in"));
        assert!(html.contains("Pace:"));
        assert!(html.contains(">Quit</button>"));
        assert!(html.contains("<title>Anthropic</title>"));
        assert!(html.contains("<title>Cursor</title>"));
        assert!(html.contains("<title>OpenAI</title>"));
        if let Ok(path) = std::env::var("SPANREED_TRAY_MENU") {
            let mut preview = cards;
            for (id, name) in [
                ("grok", "Grok"),
                ("nous", "Nous"),
                ("opencode-go", "OpenCode Go"),
                ("copilot", "Copilot"),
                ("kimi", "Kimi"),
                ("amp", "Amp"),
                ("factory", "Factory"),
                ("jetbrains-ai-assistant", "JetBrains"),
            ] {
                preview.push(quota(id, name, "Pro", 20.0, 15.0, now));
            }
            std::fs::write(path, render(&preview, true, None)).expect("menu preview");
        }
    }

    #[test]
    fn provider_marks_replace_letter_glyphs() {
        let now = 1_750_000_000_000;
        let cards = vec![
            quota("copilot", "Copilot", "Pro", 10.0, 10.0, now),
            quota("unknown-vendor", "Mystery", "Free", 10.0, 10.0, now),
        ];
        let html = render(&cards, true, None);
        assert!(html.contains("<title>Copilot</title>"));
        assert!(html.contains("data-icon=\"fallback\""));
        assert!(!html.contains(">C</span>"));
        assert!(!html.contains(">M</span>"));
    }

    #[test]
    fn present_includes_the_provider_without_local_logs() {
        let html = present(
            &[output()],
            true,
            Some("Ready"),
            1_700_000_000_000,
            &table(),
        );
        assert!(html.contains("Codex"));
        assert!(!html.contains("Ready"));
        assert!(!html.contains("class=\"status\""));
        assert!(!html.contains("Capture is down"));
    }
}
