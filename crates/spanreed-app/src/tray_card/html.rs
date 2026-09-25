//! The tray card document: provider tabs, one panel per provider, the
//! menu and the reset dialog. Styles and script live in card.css / card.js.

use super::{Meter, TrayCard};

const STYLE: &str = include_str!("card.css");
const SCRIPT: &str = include_str!("card.js");

pub fn render(cards: &[TrayCard], capture_up: bool, status: Option<&str>) -> String {
    let selected = cards.iter().position(needs_attention).unwrap_or(0);
    let body = if cards.is_empty() {
        "<p class=\"empty\">No locally detected providers</p>".into()
    } else {
        let tabs = tabs_html(cards, selected);
        let panels = cards
            .iter()
            .enumerate()
            .map(|(index, card)| card_html(card, index, index == selected))
            .collect::<Vec<_>>()
            .join("");
        format!("<nav class=\"switcher\" aria-label=\"Providers\">{tabs}</nav>{panels}")
    };
    let banner = if capture_up {
        String::new()
    } else {
        "<p class=\"banner\">Capture is down. Ensure capture before new hops are recorded.</p>"
            .into()
    };
    // The open card paints status with a script. Baking it into the document
    // lets a late reload put "Reset used" back after the tray cleared it.
    let _ = status;
    let status = "";
    format!(
        r##"<!DOCTYPE html>
<html lang="en" data-gem="stormlight">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Spanreed</title>
<style>
{STYLE}</style>
</head>
<body>
<main class="card">
{banner}{status}
{body}
<nav class="menu">
  <button type="button" data-act="refresh">Refresh</button>
  <button type="button" data-act="ensure">Ensure capture</button>
  <button type="button" data-act="log">Open log</button>
  <button type="button" data-act="dashboard">Open dashboard</button>
  <button type="button" data-act="settings">Settings</button>
  <button type="button" data-act="quit">Quit</button>
</nav>
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
{SCRIPT}</script>
</body>
</html>"##,
    )
}

/// One switcher button per provider; `selected` starts open.
fn tabs_html(cards: &[TrayCard], selected: usize) -> String {
    cards
        .iter()
        .enumerate()
        .map(|(index, card)| {
            let on = if index == selected { " on" } else { "" };
            format!(
                "<button type=\"button\" class=\"switch{on}\" data-tab=\"{index}\"><span class=\"glyph\">{}</span><span>{}</span></button>",
                crate::provider_icons::icon(&card.provider_id),
                esc(&card.name)
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

pub(super) fn card_html(card: &TrayCard, index: usize, selected: bool) -> String {
    let meters = card
        .meters
        .iter()
        .map(meter_html)
        .collect::<Vec<_>>()
        .join("");
    let credits_block = reset_credits_html(card);
    let cost = cost_html(card);
    let credits = credits_html(card);
    let error = card
        .error
        .as_deref()
        .map(|text| format!("<p class=\"error\">{}</p>", esc(text)))
        .unwrap_or_default();
    let plan = if card.plan.is_empty() {
        String::new()
    } else {
        format!("<span class=\"plan\">{}</span>", esc(&card.plan))
    };
    let account = if card.account.is_empty() {
        String::new()
    } else {
        format!("<p class=\"note\">{}</p>", esc(&card.account))
    };
    let on = if selected { " on" } else { "" };
    format!(
        r#"<section class="panel{on}" data-panel="{index}">
<header class="head"><div><h1>{name}</h1><p class="meta">{updated}</p></div>{plan}</header>
{error}
{account}
{meters}{credits_block}{cost}{credits}
</section>"#,
        index = index,
        name = esc(&card.name),
        updated = esc(&card.updated),
        plan = plan,
        account = account,
    )
}

/// Limit reset credits, with a "Use reset" button when one can be redeemed.
fn reset_credits_html(card: &TrayCard) -> String {
    let Some(count) = &card.reset_credits else {
        return String::new();
    };
    let action = if card.can_use_reset {
        let who = if card.account.is_empty() {
            &card.name
        } else {
            &card.account
        };
        format!(
            "<button type=\"button\" class=\"reset\" data-reset=\"{}\">Use reset</button>",
            esc(who)
        )
    } else {
        String::new()
    };
    format!(
        "<section class=\"credits\"><div class=\"row\"><div><h2>Limit Reset Credits</h2><div>{}</div></div><div class=\"sub\">{}</div></div>{action}</section>",
        esc(count),
        esc(card.reset_expiry.as_deref().unwrap_or(""))
    )
}

/// The collapsible cost block: stats, daily bars and notes.
fn cost_html(card: &TrayCard) -> String {
    let stats = stats_html(card);
    let bars = bars_html(card);
    let notes = card
        .notes
        .iter()
        .map(|note| format!("<p class=\"note\">{}</p>", esc(note)))
        .collect::<Vec<_>>()
        .join("");
    if stats.is_empty() && bars.is_empty() && notes.is_empty() {
        String::new()
    } else {
        format!("<details class=\"cost\"><summary>Cost</summary>{stats}{bars}{notes}</details>")
    }
}

fn stats_html(card: &TrayCard) -> String {
    if card.stats.is_empty() {
        return String::new();
    }
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
}

fn bars_html(card: &TrayCard) -> String {
    if card.bars.is_empty() {
        return String::new();
    }
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
    format!(
        "<div class=\"bars\" role=\"img\" aria-label=\"Daily cost\">{marks}</div><p class=\"note hover\"></p>"
    )
}

/// Prepaid credits meter with an optional "Buy credits" link.
fn credits_html(card: &TrayCard) -> String {
    let Some(left) = &card.credits_left else {
        return String::new();
    };
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

pub(super) fn needs_attention(card: &TrayCard) -> bool {
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

fn meter_html(meter: &Meter) -> String {
    let (fill, caption) = if meter.shows_left {
        let used = (100.0 - meter.fill).clamp(0.0, 100.0);
        (used, format!("{}% used", used.round() as i64))
    } else {
        (meter.fill, meter.left.clone())
    };
    let reset = meter.reset_text.as_deref().map(esc).unwrap_or_default();
    let pace = pace_sentence(meter);
    format!(
        "<section class=\"quota\"><h2>{title}</h2><div class=\"track\"><div class=\"fill\" style=\"width:{fill:.1}%\"><i class=\"dot\"></i></div></div><div class=\"pair\"><span>{caption}</span><span>{reset}</span></div>{pace}</section>",
        title = esc(&meter.title),
        caption = esc(&caption),
    )
}

fn pace_sentence(meter: &Meter) -> String {
    let Some(left) = &meter.pace_left else {
        return String::new();
    };
    let stance = if left == "On pace" {
        "On pace".to_string()
    } else if meter.deficit {
        format!("Behind ({})", left.trim_end_matches(" in deficit"))
    } else {
        format!("Ahead ({})", left.trim_end_matches(" in reserve"))
    };
    let tail = meter.pace_right.as_deref().unwrap_or("Lasts until reset");
    format!(
        "<p class=\"pace-line\">Pace: {} · {}</p>",
        esc(&stance),
        esc(tail)
    )
}

pub(super) fn esc(text: &str) -> String {
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
