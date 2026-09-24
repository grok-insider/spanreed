#![cfg_attr(not(feature = "tray"), allow(dead_code))]
//! CodexBar-style tray card: quota pace, reset credits, and local cost.
//!
//! Pure data and HTML. The `tray` feature hosts this document in a borderless
//! window on macOS, Windows, and Linux. Colors are the Fabrials dark/light tokens.

use fabrials_core::{Availability, Freshness, ResetInventory};

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
    let (title, subtitle) = menu_summary(cards);
    let title = esc(&title);
    let subtitle = esc(&subtitle);
    let updated = esc(cards
        .first()
        .map(|card| card.updated.as_str())
        .unwrap_or(""));
    let (attention, plenty): (Vec<_>, Vec<_>) = cards
        .iter()
        .enumerate()
        .partition(|(_, card)| needs_attention(card));
    let body = if cards.is_empty() {
        "<p class=\"empty\">No locally detected providers</p>".into()
    } else {
        format!(
            "{}{}",
            menu_section("Needs attention", &attention),
            menu_section("Plenty of room", &plenty)
        )
    };
    let timeline = limits_timeline(cards);
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
        r##"<!DOCTYPE html>
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
button.link, button.act, button.reset {{
  border: 0;
  background: transparent;
  color: var(--brand);
  font: inherit;
  padding: 0;
  cursor: pointer;
}}
button.reset {{
  margin-top: 8px;
  min-height: 32px;
  padding: 4px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  color: var(--fg);
}}
.dialog {{
  position: fixed;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 16px;
  background: oklch(0.2 0.01 85 / 45%);
}}
.dialog[hidden] {{ display: none; }}
.dialog-card {{
  width: min(100%, 320px);
  padding: 16px;
  border-radius: 12px;
  background: var(--card);
  color: var(--fg);
  box-shadow: var(--shadow);
}}
.dialog-card h2 {{ margin-top: 0; }}
.dialog-card p {{ margin: 0; color: var(--muted); line-height: 1.4; }}
.dialog-actions {{
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}}
.dialog-actions button {{
  min-height: 36px;
  padding: 4px 12px;
  border-radius: 8px;
  border: 1px solid var(--line);
  background: transparent;
  color: var(--fg);
  font: inherit;
  cursor: pointer;
}}
.dialog-actions button.confirm {{
  background: var(--brand);
  border-color: transparent;
  color: white;
}}
.dialog-actions button:disabled {{ opacity: 0.6; cursor: default; }}
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
.summary {{ display: flex; justify-content: space-between; gap: 12px; align-items: flex-start; }}
.summary h1 {{ margin: 0; font-size: 16px; font-weight: 650; letter-spacing: -0.01em; }}
.summary p, .ago, .kicker, .when, .pace, .pill {{ color: var(--muted); }}
.ago {{ font-size: 11px; white-space: nowrap; }}
.kicker {{
  display: flex;
  justify-content: space-between;
  margin: 16px 0 6px;
  font-size: 10px;
  font-weight: 650;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}}
.agent {{ padding: 8px 0 10px; border-top: 1px solid var(--line); }}
.agent-main {{
  display: grid;
  grid-template-columns: 28px 1fr;
  gap: 10px;
  align-items: start;
  width: 100%;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}}
.glyph {{
  width: 28px;
  height: 28px;
  display: grid;
  place-items: center;
  border-radius: 8px;
  background: var(--track);
  font-size: 12px;
  font-weight: 650;
}}
.stack {{ min-width: 0; display: flex; flex-direction: column; gap: 6px; }}
.topline, .subline {{ display: flex; justify-content: space-between; gap: 8px; align-items: baseline; }}
.name {{ font-size: 13px; font-weight: 600; }}
.pill {{
  margin-left: 6px;
  padding: 1px 6px;
  border-radius: 999px;
  background: var(--track);
  font-size: 10px;
  font-weight: 550;
}}
.pace {{ font-size: 11px; }}
.dot {{
  display: inline-block;
  width: 6px;
  height: 6px;
  margin-right: 6px;
  border-radius: 999px;
  background: var(--ok);
  vertical-align: 1px;
}}
.pace.warn {{ color: var(--danger); }}
.pace.warn .dot {{ background: var(--danger); }}
.topline b {{ font-size: 13px; font-weight: 650; }}
.when {{ font-size: 11px; white-space: nowrap; }}
.stack .track {{ height: 4px; overflow: visible; }}
.stack .fill {{ display: block; }}
.agent .fill {{ background: var(--brand); }}
.mark {{ background: var(--fg); width: 2px; height: 8px; top: -2px; }}
.detail {{ display: none; }}
.agent.open .detail {{ display: block; margin: 8px 0 2px 38px; }}
.detail h2 {{ font-size: 12px; margin: 8px 0 4px; }}
.actions button.act {{ color: var(--muted); }}
.returns {{ margin-top: 8px; padding-top: 8px; border-top: 1px solid var(--line); }}
.returns h2 {{ margin: 0; font-size: 13px; }}
.range {{ display: flex; gap: 4px; }}
.range button {{
  border: 0;
  background: transparent;
  color: var(--muted);
  font: inherit;
  font-size: 11px;
  padding: 2px 6px;
  border-radius: 999px;
  cursor: pointer;
}}
.range button.on {{ background: var(--track); color: var(--fg); }}
.timeline {{ position: relative; height: 42px; margin-top: 14px; }}
.timeline .rail {{
  position: absolute;
  left: 0; right: 0; top: 4px;
  height: 2px;
  background: var(--track);
}}
.tick {{
  position: absolute;
  top: 0;
  transform: translateX(-50%);
  text-align: center;
  font-size: 10px;
  color: var(--muted);
}}
.tick i {{
  display: block;
  width: 7px;
  height: 7px;
  margin: 0 auto 4px;
  border-radius: 999px;
  background: var(--brand);
}}
.tick[hidden] {{ display: none; }}
.none {{ margin: 8px 0 0; font-size: 11px; color: var(--muted); }}
</style>
</head>
<body>
<main class="card">
{banner}{status}
<header class="summary">
  <div><h1>{title}</h1><p>{subtitle}</p></div>
  <span class="ago">{updated}</span>
</header>
{body}
{timeline}
<div class="actions">
  <button type="button" class="act" data-act="refresh">Refresh</button>
  <button type="button" class="act" data-act="ensure">Ensure capture</button>
  <button type="button" class="act" data-act="log">Open log</button>
  <button type="button" class="act" data-act="quit">Quit tray</button>
</div>
<div class="dialog" hidden>
  <div class="dialog-card" role="dialog" aria-modal="true" aria-labelledby="reset-title">
    <h2 id="reset-title">Use a limit reset?</h2>
    <p id="reset-body"></p>
    <div class="dialog-actions">
      <button type="button" data-reset-cancel>Cancel</button>
      <button type="button" class="confirm" data-reset-confirm>Use reset</button>
    </div>
  </div>
</div>
</main>
<script>
const post = (msg) => {{
  try {{ window.ipc.postMessage(msg); }} catch (e) {{}}
}};
document.querySelectorAll("[data-open]").forEach((button) => {{
  button.addEventListener("click", () => {{
    button.closest(".agent").classList.toggle("open");
  }});
}});
const applyRange = (hours) => {{
  const max = Number(hours) * 3600000;
  let shown = 0;
  document.querySelectorAll("[data-reset-ms]").forEach((node) => {{
    const ms = Number(node.dataset.resetMs);
    const visible = ms > 0 && ms <= max;
    node.hidden = !visible;
    if (visible) {{
      shown += 1;
      node.style.left = Math.min(96, Math.max(4, ms / max * 100)) + "%";
    }}
  }});
  const empty = document.querySelector(".none");
  if (empty) empty.hidden = shown > 0;
}};
document.querySelectorAll("[data-range]").forEach((button) => {{
  button.addEventListener("click", () => {{
    document.querySelectorAll("[data-range]").forEach((node) => node.classList.toggle("on", node === button));
    applyRange(button.dataset.range);
  }});
}});
applyRange(168);
document.querySelectorAll("[data-act]").forEach((button) => {{
  button.addEventListener("click", () => post(button.dataset.act));
}});
document.querySelectorAll("[data-buy]").forEach((button) => {{
  button.addEventListener("click", () => post("buy " + button.dataset.buy));
}});
const resetDialog = document.querySelector(".dialog");
const resetBody = document.querySelector("#reset-body");
const resetCancel = document.querySelector("[data-reset-cancel]");
const resetConfirm = document.querySelector("[data-reset-confirm]");
let resetPending = false;
document.querySelectorAll("[data-reset]").forEach((button) => {{
  button.addEventListener("click", () => {{
    if (resetPending) return;
    const account = button.dataset.reset || "this account";
    resetBody.textContent = "This spends one limit reset credit for " + account + " and asks Codex to clear the current rate-limit windows. This cannot be undone.";
    resetDialog.hidden = false;
  }});
}});
resetCancel.addEventListener("click", () => {{
  if (resetPending) return;
  resetDialog.hidden = true;
}});
resetConfirm.addEventListener("click", () => {{
  if (resetPending) return;
  resetPending = true;
  resetConfirm.textContent = "Using reset…";
  resetConfirm.disabled = true;
  resetCancel.disabled = true;
  post("reset");
}});
document.querySelectorAll(".bars i").forEach((bar) => {{
  bar.addEventListener("mouseenter", () => {{
    const note = bar.closest(".card-body").querySelector(".hover");
    if (note) note.textContent = bar.dataset.detail || "";
  }});
}});
document.addEventListener("keydown", (event) => {{
  if (event.key !== "Escape") return;
  if (!resetDialog.hidden && !resetPending) {{
    resetDialog.hidden = true;
    return;
  }}
  post("close");
}});
</script>
</body>
</html>"##,
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

fn card_html(card: &TrayCard, index: usize) -> String {
    let meters = card
        .meters
        .iter()
        .map(meter_html)
        .collect::<Vec<_>>()
        .join("");
    let credits_block = match (&card.reset_credits, &card.reset_expiry) {
        (Some(count), expiry) => {
            let action = if card.can_use_reset {
                let who = if card.account.is_empty() {
                    card.name.clone()
                } else {
                    card.account.clone()
                };
                format!(
                    "<button type=\"button\" class=\"reset\" data-reset=\"{}\">Use reset</button>",
                    esc(&who)
                )
            } else {
                String::new()
            };
            format!(
                "<section class=\"credits\"><div class=\"row\"><div><h2>Limit Reset Credits</h2><div>{}</div></div><div class=\"sub\">{}</div></div>{action}</section>",
                esc(count),
                esc(expiry.as_deref().unwrap_or(""))
            )
        }
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
        format!("<span class=\"pill\">{}</span>", esc(&card.plan))
    };
    let meter = primary_meter(card);
    let value = meter.map(|item| item.left.as_str()).unwrap_or("—");
    let pace = meter.map(pace_line).unwrap_or_default();
    let when = meter
        .and_then(|item| item.resets_in_ms)
        .map(duration_text)
        .unwrap_or_default();
    let fill = meter.map(|item| item.fill).unwrap_or(0.0);
    let mark = meter
        .and_then(|item| item.marker)
        .map(|percent| format!("<i class=\"mark\" style=\"left:{percent:.1}%\"></i>"))
        .unwrap_or_default();
    let warn = if needs_attention(card) { " warn" } else { "" };
    let attention = if needs_attention(card) {
        " attention"
    } else {
        ""
    };
    let initial = card
        .name
        .chars()
        .next()
        .map(|ch| ch.to_ascii_uppercase())
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_string())
        .unwrap_or_else(|| "•".into());
    let account = if card.account.is_empty() {
        String::new()
    } else {
        format!("<p class=\"note\">{}</p>", esc(&card.account))
    };
    format!(
        r#"<article class="agent{attention}" data-card="{index}">
<button type="button" class="agent-main" data-open="{index}">
  <span class="glyph">{initial}</span>
  <span class="stack">
    <span class="topline"><span class="name">{name} {plan}</span><b>{value}</b></span>
    <span class="track"><span class="fill" style="width:{fill:.1}%"></span>{mark}</span>
    <span class="subline"><span class="pace{warn}"><i class="dot"></i>{pace}</span><span class="when">{when}</span></span>
  </span>
</button>
<div class="detail">
{error}
{account}
{meters}{credits_block}{stats}{bars}{notes}{credits}
</div>
</article>"#,
        index = index,
        initial = esc(&initial),
        name = esc(&card.name),
        plan = plan,
        pace = esc(&pace),
        value = esc(value),
        when = esc(&when),
        fill = fill,
        account = account,
    )
}

fn menu_section(label: &str, rows: &[(usize, &TrayCard)]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let body = rows
        .iter()
        .map(|(index, card)| card_html(card, *index))
        .collect::<Vec<_>>()
        .join("");
    format!(
        "<section><h2 class=\"kicker\"><span>{}</span><span>{}</span></h2>{}</section>",
        esc(label),
        rows.len(),
        body
    )
}

fn menu_summary(cards: &[TrayCard]) -> (String, String) {
    let attention = cards.iter().filter(|card| needs_attention(card)).count();
    let title = if attention == 0 {
        "Plenty of room".into()
    } else if attention == 1 {
        "1 needs attention".into()
    } else {
        format!("{attention} need attention")
    };
    let mut room: Vec<_> = cards
        .iter()
        .filter_map(|card| {
            primary_meter(card)
                .filter(|meter| meter.shows_left)
                .map(|meter| (card, meter))
        })
        .collect();
    room.sort_by(|left, right| right.1.fill.total_cmp(&left.1.fill));
    room.dedup_by(|left, right| left.0.name == right.0.name);
    let subtitle = match room.as_slice() {
        [(first, first_meter), (second, second_meter), ..] => format!(
            "Most room: {} {} · {} {}",
            first.name, first_meter.left, second.name, second_meter.left
        ),
        [(first, first_meter), ..] => format!("Most room: {} {}", first.name, first_meter.left),
        [] => String::new(),
    };
    (title, subtitle)
}

fn needs_attention(card: &TrayCard) -> bool {
    if card.error.is_some() {
        return true;
    }
    let Some(meter) = primary_meter(card) else {
        return false;
    };
    if meter.shows_left {
        meter.fill < 25.0 || meter.deficit
    } else {
        meter.fill >= 80.0 || meter.deficit
    }
}

fn primary_meter(card: &TrayCard) -> Option<&Meter> {
    card.meters
        .iter()
        .filter(|meter| meter.shows_left)
        .min_by(|left, right| left.fill.total_cmp(&right.fill))
        .or_else(|| card.meters.first())
}

fn pace_line(meter: &Meter) -> String {
    if let Some(pace) = &meter.pace_left {
        format!("{} · {pace}", meter.title)
    } else if let Some(pace) = &meter.pace_right {
        format!("{} · {pace}", meter.title)
    } else {
        meter.title.clone()
    }
}

fn limits_timeline(cards: &[TrayCard]) -> String {
    let mut marks = Vec::new();
    for card in cards {
        let Some(meter) = primary_meter(card) else {
            continue;
        };
        let Some(ms) = meter.resets_in_ms else {
            continue;
        };
        marks.push(format!(
            "<div class=\"tick\" data-reset-ms=\"{ms}\"><i></i><span>{}</span></div>",
            esc(&duration_text(ms))
        ));
    }
    if marks.is_empty() {
        return String::new();
    }
    format!(
        r#"<section class="returns"><div class="row"><div><h2>Limits come back</h2><p class="note">Tightest limit per provider</p></div><div class="range"><button type="button" data-range="24">24h</button><button type="button" class="on" data-range="168">7d</button></div></div><div class="timeline"><div class="rail"></div>{marks}</div><p class="none" hidden>No resets in this window</p></section>"#,
        marks = marks.join("")
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
        assert!(html.contains("Codex &lt;script&gt;"));
        assert!(!html.contains("Codex <script>"));
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
        assert!(html.contains("1 needs attention"));
        assert!(html.contains("Plenty of room"));
        assert!(html.contains("Limits come back"));
        assert!(html.contains("Most room:"));
        assert!(html.contains("in deficit") || html.contains("in reserve"));
        assert!(html.contains("class=\"dot\""));
        if let Ok(path) = std::env::var("SPANREED_TRAY_MENU") {
            std::fs::write(path, &html).expect("menu preview");
        }
    }

    #[test]
    fn present_includes_the_provider_without_local_logs() {
        let html = present(&[output()], true, Some("Ready"), 1_700_000_000_000);
        assert!(html.contains("Codex"));
        assert!(html.contains("Ready"));
        assert!(!html.contains("Capture is down"));
    }
}
