#![cfg_attr(not(feature = "tray"), allow(dead_code))]
//! CodexBar-style tray card: quota pace, reset credits, and local cost.
//!
//! Pure data and HTML. The `tray` feature hosts this document in a borderless
//! window on macOS, Windows, and Linux. Colors are the Fabrials dark/light tokens.

use fabrials_core::{Freshness, ResetInventory};

use crate::cost::CostSummary;
use crate::model::{MetricLine, ProgressFormat, ProviderOutput};
use crate::util;

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
    /// Fill of the bar, 0–100. Quota meters show percent left.
    pub fill: f64,
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
                if let Some(meter) = meter(
                    label,
                    *used,
                    *limit,
                    format,
                    resets_at.as_deref(),
                    source.now_ms,
                ) {
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
) -> String {
    let cards = outputs
        .iter()
        .map(|output| {
            let account = account_label(output);
            let cost = local_cost(output);
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

fn local_cost(output: &ProviderOutput) -> Option<CostSummary> {
    match output.provider_id.split('/').next().unwrap_or("") {
        "codex" => crate::cost::estimate(crate::cost::Source::Codex),
        "claude" => crate::cost::estimate(crate::cost::Source::Claude),
        _ => None,
    }
}

pub fn render(cards: &[TrayCard], capture_up: bool, status: Option<&str>) -> String {
    let body = if cards.is_empty() {
        "<p class=\"empty\">No locally detected providers</p>".into()
    } else {
        cards
            .iter()
            .enumerate()
            .map(|(index, card)| card_html(card, index))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let tabs = if cards.len() > 1 {
        let buttons = cards
            .iter()
            .enumerate()
            .map(|(index, card)| {
                format!(
                    "<button type=\"button\" class=\"tab{}\" data-tab=\"{}\">{}</button>",
                    if index == 0 { " on" } else { "" },
                    index,
                    esc(&card.name)
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<div class=\"tabs\" role=\"tablist\">{buttons}</div>")
    } else {
        String::new()
    };
    let banner = if capture_up {
        String::new()
    } else {
        "<p class=\"banner\">Capture is down. Ensure capture before new hops are recorded.</p>"
            .into()
    };
    let status = status
        .filter(|text| !text.is_empty())
        .map(|text| format!("<p class=\"status\">{}</p>", esc(text)))
        .unwrap_or_default();
    format!(
        r#"<!DOCTYPE html>
<html lang="en" data-gem="stormlight">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Spanreed</title>
<style>
:root {{
  color-scheme: light;
  --bg: oklch(0.985 0.002 85);
  --fg: oklch(0.21 0.006 85);
  --muted: oklch(0.48 0.008 85);
  --card: oklch(1 0 0);
  --line: oklch(0.905 0.004 85);
  --track: oklch(0.94 0.004 85);
  --brand: oklch(0.54 0.16 250);
  --danger: oklch(0.55 0.2 27);
  --ok: oklch(0.52 0.11 162);
  --shadow: 0 12px 32px oklch(0.2 0.01 85 / 18%);
}}
@media (prefers-color-scheme: dark) {{
  :root {{
    color-scheme: dark;
    --bg: oklch(0.215 0.004 85);
    --fg: oklch(0.955 0.003 85);
    --muted: oklch(0.74 0.007 85);
    --card: oklch(0.245 0.004 85);
    --line: oklch(1 0 0 / 10%);
    --track: oklch(0.32 0.006 85);
    --brand: oklch(0.72 0.13 240);
    --danger: oklch(0.7 0.16 22);
    --ok: oklch(0.74 0.12 162);
    --shadow: 0 18px 40px oklch(0 0 0 / 45%);
  }}
}}
* {{ box-sizing: border-box; }}
html, body {{
  margin: 0;
  height: 100%;
  background: var(--card);
  color: var(--fg);
}}
body {{
  font-family: "IBM Plex Sans", ui-sans-serif, system-ui, sans-serif;
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  overflow: auto;
}}
.card {{
  width: 100%;
  min-height: 100%;
  padding: 16px 18px 12px;
  background: var(--card);
}}
.tabs {{ display: flex; gap: 6px; margin-bottom: 10px; flex-wrap: wrap; }}
.tab {{
  border: 1px solid var(--line);
  background: transparent;
  color: var(--muted);
  border-radius: 999px;
  padding: 4px 10px;
  font: inherit;
  cursor: pointer;
}}
.tab.on {{ color: var(--fg); border-color: var(--brand); }}
.head {{ display: flex; justify-content: space-between; gap: 12px; align-items: baseline; }}
.name {{ font-size: 16px; font-weight: 600; margin: 0; }}
.account, .meta, .sub, .note, .scale {{ color: var(--muted); }}
.row {{ display: flex; justify-content: space-between; gap: 12px; align-items: baseline; }}
h2 {{ font-size: 14px; font-weight: 500; margin: 14px 0 6px; }}
.track {{
  position: relative;
  height: 6px;
  border-radius: 999px;
  background: var(--track);
  overflow: hidden;
}}
.fill {{
  height: 100%;
  background: var(--brand);
  border-radius: inherit;
}}
.mark {{
  position: absolute;
  top: -1px;
  width: 2px;
  height: 8px;
  background: var(--ok);
}}
.mark.deficit {{ background: var(--danger); }}
.pair {{ display: flex; justify-content: space-between; gap: 8px; margin-top: 4px; font-size: 12px; }}
.stats {{
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px 12px;
  margin-top: 14px;
}}
.stat b {{ display: block; font-size: 16px; font-weight: 600; }}
.stat span {{ color: var(--muted); font-size: 11px; }}
.bars {{
  display: flex;
  align-items: flex-end;
  gap: 2px;
  height: 58px;
  margin-top: 10px;
  border-bottom: 1px solid var(--line);
}}
.bars i {{
  display: block;
  flex: 1;
  min-width: 0;
  background: var(--brand);
  border-radius: 2px 2px 0 0;
  height: var(--h);
}}
.note {{ margin: 8px 0 0; font-size: 12px; }}
.banner, .status, .error {{
  margin: 0 0 10px;
  color: var(--danger);
  font-size: 12px;
}}
.status {{ color: var(--muted); }}
button.link, button.act {{
  border: 0;
  background: transparent;
  color: var(--brand);
  font: inherit;
  padding: 0;
  cursor: pointer;
}}
.actions {{
  display: flex;
  gap: 12px;
  margin-top: 12px;
  padding-top: 8px;
  border-top: 1px solid var(--line);
}}
.actions button {{ color: var(--fg); }}
.card-body {{ display: none; }}
.card-body.on {{ display: block; }}
.empty {{ padding: 24px; }}
</style>
</head>
<body>
<main class="card">
{banner}{status}{tabs}
{body}
<div class="actions">
  <button type="button" class="act" data-act="refresh">Refresh</button>
  <button type="button" class="act" data-act="ensure">Ensure capture</button>
  <button type="button" class="act" data-act="log">Open log</button>
  <button type="button" class="act" data-act="quit">Quit tray</button>
</div>
</main>
<script>
const post = (msg) => {{
  try {{ window.ipc.postMessage(msg); }} catch (e) {{}}
}};
document.querySelectorAll("[data-tab]").forEach((tab) => {{
  tab.addEventListener("click", () => {{
    document.querySelectorAll("[data-tab]").forEach((node) => node.classList.toggle("on", node === tab));
    document.querySelectorAll(".card-body").forEach((node) => {{
      node.classList.toggle("on", node.dataset.card === tab.dataset.tab);
    }});
  }});
}});
document.querySelectorAll("[data-act]").forEach((button) => {{
  button.addEventListener("click", () => post(button.dataset.act));
}});
document.querySelectorAll("[data-buy]").forEach((button) => {{
  button.addEventListener("click", () => post("buy " + button.dataset.buy));
}});
document.querySelectorAll(".bars i").forEach((bar) => {{
  bar.addEventListener("mouseenter", () => {{
    const note = bar.closest(".card-body").querySelector(".hover");
    if (note) note.textContent = bar.dataset.detail || "";
  }});
}});
document.addEventListener("keydown", (event) => {{
  if (event.key === "Escape") post("close");
}});
</script>
</body>
</html>"#,
    )
}

fn meter(
    label: &str,
    used: f64,
    limit: f64,
    format: &ProgressFormat,
    resets_at: Option<&str>,
    now_ms: i64,
) -> Option<Meter> {
    let resets_ms = resets_at.and_then(|iso| {
        util::parse_iso_dt(iso).map(|dt| (dt.unix_timestamp_nanos() / 1_000_000) as i64)
    });
    let reset_text = resets_ms
        .filter(|ms| *ms > now_ms)
        .map(|ms| format!("Resets in {}", duration_text(ms - now_ms)));
    match format {
        ProgressFormat::Percent => {
            let used = used.clamp(0.0, 100.0);
            let pace = window_minutes(label)
                .and_then(|minutes| resets_ms.and_then(|ms| pace(used, ms, minutes, now_ms)));
            Some(Meter {
                title: label.to_string(),
                reset_text,
                fill: (100.0 - used).clamp(0.0, 100.0),
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
                fill,
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
                fill,
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

fn card_html(card: &TrayCard, index: usize) -> String {
    let meters = card
        .meters
        .iter()
        .map(meter_html)
        .collect::<Vec<_>>()
        .join("");
    let credits_block = match (&card.reset_credits, &card.reset_expiry) {
        (Some(count), expiry) => format!(
            "<section class=\"row\"><div><h2>Limit Reset Credits</h2><div>{}</div></div><div class=\"sub\">{}</div></section>",
            esc(count),
            esc(expiry.as_deref().unwrap_or(""))
        ),
        _ => String::new(),
    };
    let stats = if card.stats.is_empty() {
        String::new()
    } else {
        let cells = card
            .stats
            .iter()
            .map(|stat| {
                format!(
                    "<div class=\"stat{}\"><span>{}</span><b>{}</b></div>",
                    if stat.emphasis { " emphasis" } else { "" },
                    esc(&stat.title),
                    esc(&stat.value)
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<div class=\"stats\">{cells}</div>")
    };
    let bars = if card.bars.is_empty() {
        String::new()
    } else {
        let marks = card
            .bars
            .iter()
            .map(|bar| {
                format!(
                    "<i style=\"--h:{}%\" data-detail=\"{}\" title=\"{}\"></i>",
                    bar.value.max(2.0),
                    esc(&bar.detail),
                    esc(&bar.label)
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<div class=\"bars\" role=\"img\" aria-label=\"Daily cost\">{marks}</div><p class=\"note hover\"></p>")
    };
    let notes = card
        .notes
        .iter()
        .map(|note| format!("<p class=\"note\">{}</p>", esc(note)))
        .collect::<Vec<_>>()
        .join("");
    let credits = match &card.credits_left {
        Some(left) => {
            let fill = card.credits_fill.unwrap_or(0.0);
            let scale = card
                .credits_scale
                .as_deref()
                .map(|scale| format!("<span class=\"scale\">{}</span>", esc(scale)))
                .unwrap_or_default();
            let buy = card
                .buy_url
                .as_deref()
                .map(|url| {
                    format!(
                        "<button type=\"button\" class=\"link\" data-buy=\"{}\">Buy credits</button>",
                        esc(url)
                    )
                })
                .unwrap_or_default();
            format!(
                "<h2>Credits</h2><div class=\"track\"><div class=\"fill\" style=\"width:{fill:.1}%\"></div></div><div class=\"pair\"><span>{left}</span>{scale}</div><p class=\"note\">{buy}</p>",
                left = esc(left)
            )
        }
        None => String::new(),
    };
    let error = card
        .error
        .as_deref()
        .map(|text| format!("<p class=\"error\">{}</p>", esc(text)))
        .unwrap_or_default();
    let plan = if card.plan.is_empty() {
        String::new()
    } else {
        format!("<span>{}</span>", esc(&card.plan))
    };
    format!(
        r#"<section class="card-body{on}" data-card="{index}">
{error}
<div class="head"><h1 class="name">{name}</h1><span class="account">{account}</span></div>
<div class="row meta"><span>{updated}</span>{plan}</div>
{meters}{credits_block}{stats}{bars}{notes}{credits}
</section>"#,
        on = if index == 0 { " on" } else { "" },
        index = index,
        name = esc(&card.name),
        account = esc(&card.account),
        updated = esc(&card.updated),
        plan = plan,
        meters = meters,
        credits_block = credits_block,
        stats = stats,
        bars = bars,
        notes = notes,
        credits = credits,
    )
}

fn meter_html(meter: &Meter) -> String {
    let mark = meter
        .marker
        .map(|percent| {
            let class = if meter.deficit {
                "mark deficit"
            } else {
                "mark"
            };
            format!("<i class=\"{class}\" style=\"left:{percent:.1}%\"></i>")
        })
        .unwrap_or_default();
    let pace_left = meter
        .pace_left
        .as_deref()
        .map(|text| format!("<span class=\"sub\">{}</span>", esc(text)))
        .unwrap_or_default();
    let pace_right = meter
        .pace_right
        .as_deref()
        .map(|text| format!("<span class=\"sub\">{}</span>", esc(text)))
        .unwrap_or_default();
    let reset = meter
        .reset_text
        .as_deref()
        .map(|text| format!("<span class=\"sub\">{}</span>", esc(text)))
        .unwrap_or_default();
    format!(
        "<h2 class=\"row\"><span>{title}</span>{reset}</h2><div class=\"track\"><div class=\"fill\" style=\"width:{fill:.1}%\"></div>{mark}</div><div class=\"pair\"><span>{left} {pace_left}</span>{pace_right}</div>",
        title = esc(&meter.title),
        fill = meter.fill,
        left = esc(&meter.left),
    )
}

fn duration_text(ms: i64) -> String {
    let minutes = (ms.max(0) / 60_000).max(0);
    if minutes >= 1_440 {
        format!("{}d {}h", minutes / 1_440, (minutes / 60) % 24)
    } else if minutes >= 60 {
        format!("{}h {}m", minutes / 60, minutes % 60)
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

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::DayCost;
    use crate::model::MetricLine;
    use crate::usage_stats::ModelCost;
    use fabrials_core::{Availability, Observation, ResetCredit};

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
        let html = render(&[card], true, None);
        if let Ok(path) = std::env::var("SPANREED_TRAY_PREVIEW") {
            std::fs::write(path, &html).expect("preview");
        }
        assert!(html.contains("Limit Reset Credits"));
        assert!(html.contains("Last 31 days Cost"));
        assert!(html.contains("Top model: Example-1"));
        assert!(html.contains("Buy credits"));
        assert!(html.contains("--brand:"));
        assert!(html.contains("data-act=\"refresh\""));
        assert!(html.contains("Codex &lt;script&gt;"));
        assert!(!html.contains("Codex <script>"));
    }

    #[test]
    fn present_includes_the_provider_without_local_logs() {
        let html = present(&[output()], true, Some("Ready"), 1_700_000_000_000);
        assert!(html.contains("Codex"));
        assert!(html.contains("Ready"));
        assert!(!html.contains("Capture is down"));
    }
}
