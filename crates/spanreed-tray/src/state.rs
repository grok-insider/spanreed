//! Shared tray state and its refresh from `spanreed_app::app`.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::platform::user_notify;
use spanreed_app::app::{self, AppContext};
use spanreed_domain::model::ProviderOutput;
use spanreed_domain::tray_format;
use spanreed_domain::util;

pub(super) type Shared = Arc<Mutex<TrayState>>;

/// The application context the tray was started with.
pub(super) fn context(state: &Shared) -> AppContext {
    state.lock().unwrap_or_else(|e| e.into_inner()).ctx.clone()
}

pub(super) struct TrayState {
    pub(super) ctx: AppContext,
    pub(super) outputs: Vec<ProviderOutput>,
    pub(super) capture_up: bool,
    pub(super) max_used: Option<f64>,
    pub(super) update_note: Option<String>,
    pub(super) share_logged_in: bool,
    pub(super) share_line: String,
    /// Short status line shown at the top of the tooltip (action feedback).
    pub(super) status_note: Option<String>,
    pub(super) status_at: Option<Instant>,
    pub(super) last_notify_proxy: Option<Instant>,
    pub(super) last_notify_quota: Option<Instant>,
    /// Background thread sets this; UI thread clears after repaint.
    pub(super) dirty: bool,
    /// Bumped when the card body changes. Status text does not bump it.
    pub(super) content_epoch: u64,
    /// Idempotency key for an in-progress Codex reset. Reused until success.
    pub(super) reset_request_id: Option<String>,
    pub(super) reset_in_flight: bool,
}

impl TrayState {
    pub(super) fn new(ctx: AppContext) -> Self {
        Self {
            ctx,
            outputs: Vec::new(),
            capture_up: false,
            max_used: None,
            update_note: None,
            share_logged_in: false,
            share_line: tray_format::format_share_line(false, None, ""),
            status_note: None,
            status_at: None,
            last_notify_proxy: None,
            last_notify_quota: None,
            dirty: true,
            content_epoch: 0,
            reset_request_id: None,
            reset_in_flight: false,
        }
    }
}

pub(super) fn begin_refresh(state: &Shared) {
    {
        let mut guard = state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(note) = tray_format::begin_refresh_status(guard.status_note.as_deref()) {
            stamp_status(&mut guard, note);
        }
    }
    let state = state.clone();
    thread::spawn(move || {
        refresh_state(&state);
        note_refreshed(&state);
    });
}

pub(super) fn set_status(state: &Shared, note: &str) {
    let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
    stamp_status(&mut g, note);
}

pub(super) fn note_refreshed(state: &Shared) {
    let mut guard = state.lock().unwrap_or_else(|error| error.into_inner());
    if guard.status_note.as_deref() == Some("Capture proxy is DOWN") {
        return;
    }
    stamp_status(&mut guard, "Usage refreshed");
}

pub(super) fn stamp_status(guard: &mut TrayState, note: impl Into<String>) {
    guard.status_note = Some(note.into());
    guard.status_at = Some(Instant::now());
    guard.dirty = true;
}

pub(super) fn refresh_state(state: &Shared) {
    let ctx = context(state);
    let capture_up = app::capture::is_up(&ctx);
    let outputs = app::usage::cached_or_probe(&ctx);
    let max_used = tray_format::max_used_pct(&outputs);

    let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
    let prev_used = g.max_used;
    let prev_up = g.capture_up;
    g.capture_up = capture_up;
    if capture_up && g.status_note.as_deref() == Some("Capture proxy is DOWN") {
        g.status_note = None;
        g.status_at = None;
    }
    g.outputs = outputs;
    g.max_used = max_used;
    g.share_logged_in = app::sharing::is_linked(&ctx);
    let today = util::today_day_key_madrid();
    g.share_line = tray_format::format_share_line(
        g.share_logged_in,
        app::sharing::last_shared_day(&ctx).as_deref(),
        &today,
    );
    g.dirty = true;
    g.content_epoch = g.content_epoch.wrapping_add(1);

    let now = Instant::now();
    if prev_up && !capture_up {
        let cool = g
            .last_notify_proxy
            .map(|t| now.duration_since(t) > Duration::from_secs(300))
            .unwrap_or(true);
        if cool {
            log::warn!("spanreed tray: capture proxy is DOWN");
            g.last_notify_proxy = Some(now);
            stamp_status(&mut g, "Capture proxy is DOWN");
            drop(g);
            user_notify(
                state,
                "spanreed — capture",
                "Capture proxy is DOWN. Run Ensure capture or `spanreed capture ensure`.",
                false,
            );
            return;
        }
    }
    if !capture_up && g.status_note.is_none() {
        stamp_status(&mut g, "Capture proxy is DOWN");
    }
    if let Some(band) = tray_format::crossed_threshold(prev_used, max_used) {
        let cool = g
            .last_notify_quota
            .map(|t| now.duration_since(t) > Duration::from_secs(600))
            .unwrap_or(true);
        if cool {
            log::warn!("spanreed tray: quota entered {band} band");
            g.last_notify_quota = Some(now);
            drop(g);
            let body = if band == "critical" {
                "Usage is at or above 95%."
            } else {
                "Usage is at or above 80%."
            };
            user_notify(state, "spanreed — usage", body, false);
        }
    }
}

/// Run GitHub Releases check; returns a user-facing summary string.
pub(super) fn run_update_check(state: &Shared) -> String {
    let ctx = context(state);
    if app::updates::offline(&ctx) {
        let msg = "Offline (SPANREED_OFFLINE=1) — not checking GitHub.".to_string();
        let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
        g.update_note = Some("Update: offline".into());
        g.dirty = true;
        return msg;
    }
    match app::updates::check_for_update(&ctx) {
        Ok(r) if r.newer => {
            let how = if let Some(why) = app::updates::apply_blocked_reason(&ctx) {
                format!("\n\nCannot auto-install: {why}")
            } else {
                "\n\nUse “Install update…” to apply.".into()
            };
            let msg = format!(
                "Update available: {} → {} ({}){how}",
                r.current, r.latest, r.tag
            );
            let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
            g.update_note = Some(format!("Update: {} available", r.latest));
            g.dirty = true;
            msg
        }
        Ok(r) => {
            let msg = format!("Up to date: {} ({})", r.current, r.tag);
            let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
            g.update_note = Some(format!("Update: up to date ({})", r.current));
            g.dirty = true;
            msg
        }
        Err(e) => {
            let msg = format!("Update check failed:\n{e}");
            let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
            g.update_note = Some(format!("Update: check failed ({e})"));
            g.dirty = true;
            msg
        }
    }
}

pub(super) fn usage_card(state: &Shared) -> String {
    let guard = state.lock().unwrap_or_else(|error| error.into_inner());
    app::usage::card_html(
        &guard.ctx,
        app::usage::CardInput {
            outputs: &guard.outputs,
            capture_up: guard.capture_up,
            status: guard.status_note.as_deref(),
        },
    )
}

pub(super) fn tooltip_from(state: &Shared) -> String {
    let g = state.lock().unwrap_or_else(|e| e.into_inner());
    tray_format::compose_tooltip(
        g.status_note.as_deref(),
        &g.share_line,
        &tray_format::format_tooltip(&g.outputs, g.capture_up, g.update_note.as_deref()),
    )
}

/// Bumped when the card body changes.
pub(super) fn content_epoch(state: &Shared) -> Option<u64> {
    state.lock().ok().map(|guard| guard.content_epoch)
}

/// Clear a hung reset and expire transient status lines (once per tick).
pub(super) fn expire_status(state: &Shared) {
    let mut guard = state.lock().unwrap_or_else(|error| error.into_inner());
    let now = Instant::now();
    if tray_format::reset_hung(guard.status_at, guard.reset_in_flight, now) {
        let mut in_flight = guard.reset_in_flight;
        let mut status = guard.status_note.clone();
        let before = status.clone();
        tray_format::abandon_reset(&mut in_flight, &mut status);
        guard.reset_in_flight = in_flight;
        guard.status_note = status;
        if guard.status_note != before {
            guard.status_at = Some(now);
            guard.dirty = true;
            guard.content_epoch = guard.content_epoch.wrapping_add(1);
        }
    } else if tray_format::status_expired(
        guard.status_note.as_deref(),
        guard.status_at,
        guard.reset_in_flight,
        now,
    ) {
        match tray_format::next_status_after_expiry(guard.capture_up) {
            Some(note) => {
                guard.status_note = Some(note.into());
                guard.status_at = Some(now);
            }
            None => {
                guard.status_note = None;
                guard.status_at = None;
            }
        }
        guard.dirty = true;
    }
}
