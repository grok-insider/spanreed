//! System tray companion: Spanreed icon, usage tooltip, capture health, updates.
//!
//! ```text
//! spanreed tray [--interval S]
//! ```
//!
//! Built only with `--features tray`. Does not own the capture worker — Quit
//! leaves capture running.
//!
//! Menu actions that need user-visible feedback use a short toast/tooltip update
//! (Windows: balloon tip when available, always tooltip + MessageBox fallback for
//! long results like update checks). Never kill capture from the tray; use
//! `capture ensure` only.

use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use image::RgbaImage;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::api;
use crate::capture_log;
use crate::model::ProviderOutput;
use crate::probe;
use crate::self_update;
use crate::setup;
use crate::share;
use crate::share_schedule;
use crate::share_session;
use crate::tray_format::{self, TraySeverity};
use crate::util;

const DEFAULT_INTERVAL_SECS: u64 = 60;
const MASTER_PNG: &[u8] = include_bytes!("assets/tray/spanreed-32.png");
const LOCK_FILE: &str = "tray.lock";

/// Menu labels (kept in one place so docs/tests stay aligned).
pub const MENU_REFRESH: &str = "Refresh now";
pub const MENU_ENSURE: &str = "Ensure capture";
pub const MENU_LOG: &str = "Open capture log";
pub const MENU_CHECK: &str = "Check for updates";
pub const MENU_UPDATE: &str = "Install update…";
pub const MENU_LINK_SHARE: &str = "Link share (X)…";
pub const MENU_SHARE_NOW: &str = "Share now";
pub const MENU_UNLINK_SHARE: &str = "Unlink share";
pub const MENU_DASHBOARD: &str = "Open dashboard";
pub const MENU_SETTINGS: &str = "Settings";
pub const MENU_QUIT: &str = "Quit tray";

struct TrayState {
    outputs: Vec<ProviderOutput>,
    capture_up: bool,
    max_used: Option<f64>,
    update_note: Option<String>,
    share_logged_in: bool,
    share_line: String,
    /// Short status line shown at the top of the tooltip (action feedback).
    status_note: Option<String>,
    status_at: Option<Instant>,
    last_notify_proxy: Option<Instant>,
    last_notify_quota: Option<Instant>,
    /// Background thread sets this; UI thread clears after repaint.
    dirty: bool,
    /// Idempotency key for an in-progress Codex reset. Reused until success.
    reset_request_id: Option<String>,
    reset_in_flight: bool,
}

impl Default for TrayState {
    fn default() -> Self {
        Self {
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
            reset_request_id: None,
            reset_in_flight: false,
        }
    }
}

/// CLI entry.
pub fn cmd(args: &[String]) -> ExitCode {
    let mut interval = DEFAULT_INTERVAL_SECS;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--interval" => {
                i += 1;
                let s = args
                    .get(i)
                    .ok_or_else(|| "--interval needs a value".to_string())
                    .and_then(|v| v.parse::<u64>().map_err(|_| format!("bad --interval: {v}")));
                match s {
                    Ok(n) if n >= 5 => interval = n,
                    Ok(_) => {
                        eprintln!("tray: --interval minimum is 5s");
                        return ExitCode::FAILURE;
                    }
                    Err(e) => {
                        eprintln!("tray: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "-h" | "--help" => {
                println!(
                    "spanreed tray — system tray status (Spanreed icon)\n\n\
                     \t--interval S   Refresh every S seconds (default {DEFAULT_INTERVAL_SECS})\n\
                     Left click opens the usage card. Right click keeps the menu:\n\
                     \tOpen dashboard, Settings, Refresh, Ensure, Open log,\n\
                     \tLink/Share/Unlink, Check/Install update, Quit tray"
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("tray: unknown arg: {other}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    if let Err(e) = run_tray(interval) {
        eprintln!("tray: {e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run_tray(interval_secs: u64) -> Result<(), String> {
    let _lock = acquire_single_instance()?;

    let event_loop = EventLoopBuilder::new().build();

    let menu = Menu::new();
    let item_refresh = MenuItem::new(MENU_REFRESH, true, None);
    let item_ensure = MenuItem::new(MENU_ENSURE, true, None);
    let item_log = MenuItem::new(MENU_LOG, true, None);
    let item_check = MenuItem::new(MENU_CHECK, true, None);
    let item_update = MenuItem::new(MENU_UPDATE, false, None);
    let item_share_primary = MenuItem::new(MENU_LINK_SHARE, true, None);
    let item_unlink = MenuItem::new(MENU_UNLINK_SHARE, false, None);
    let item_dashboard = MenuItem::new(MENU_DASHBOARD, true, None);
    let item_settings = MenuItem::new(MENU_SETTINGS, true, None);
    let item_quit = MenuItem::new(MENU_QUIT, true, None);
    menu.append(&item_dashboard)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_settings)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_refresh)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_ensure)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_log).map_err(|e| format!("menu: {e}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_share_primary)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_unlink)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_check).map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_update)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_quit).map_err(|e| format!("menu: {e}"))?;

    let id_refresh = item_refresh.id().clone();
    let id_ensure = item_ensure.id().clone();
    let id_log = item_log.id().clone();
    let id_share_primary = item_share_primary.id().clone();
    let id_unlink = item_unlink.id().clone();
    let id_check = item_check.id().clone();
    let id_update = item_update.id().clone();
    let id_dashboard = item_dashboard.id().clone();
    let id_settings = item_settings.id().clone();
    let id_quit = item_quit.id().clone();

    let state = Arc::new(Mutex::new(TrayState::default()));
    // First probe on main thread so tooltip is ready.
    refresh_state(&state);

    let mut popover = crate::tray_popover::Popover::new(&event_loop)?;
    let icon = icon_for_severity(TraySeverity::Ok)?;
    let mut tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(tooltip_from(&state))
        .with_icon(icon)
        .with_title(crate::app::APP_NAME)
        .build()
        .map_err(|e| format!("tray icon: {e}"))?;

    apply_visual(
        &state,
        &mut tray,
        &item_update,
        &item_check,
        &item_share_primary,
        &item_unlink,
    );

    // Background: probe only (TrayIcon/MenuItem are !Send).
    let stop = Arc::new(AtomicBool::new(false));
    let stop_bg = stop.clone();
    let state_bg = state.clone();
    thread::spawn(move || {
        while !stop_bg.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_secs(interval_secs));
            if stop_bg.load(Ordering::Relaxed) {
                break;
            }
            refresh_state(&state_bg);
        }
    });
    let proxy = event_loop.create_proxy();
    let stop_tick = stop.clone();
    thread::spawn(move || {
        while !stop_tick.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(250));
            if proxy.send_event(()).is_err() {
                break;
            }
        }
    });

    let menu_channel = MenuEvent::receiver();
    let click_channel = TrayIconEvent::receiver();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250));
        {
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
                }
            } else if tray_format::status_expired(
                guard.status_note.as_deref(),
                guard.status_at,
                guard.reset_in_flight,
                now,
            ) {
                guard.status_note = None;
                guard.status_at = None;
                guard.dirty = true;
            }
        }

        if let Event::WindowEvent {
            event: WindowEvent::Focused(false),
            ..
        } = &event
        {
            if popover.blur_should_close() {
                popover.hide();
            }
        }

        while let Ok(click) = click_channel.try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = click
            {
                let html = usage_card(&state);
                popover.toggle(rect, html);
            }
        }

        while let Some(message) = popover.poll() {
            if message == "close" {
                popover.hide();
            } else if message == "refresh" {
                set_status(&state, "Refreshing usage…");
                let st = state.clone();
                thread::spawn(move || {
                    refresh_state(&st);
                    note_refreshed(&st);
                });
            } else if message == "ensure" {
                set_status(&state, "Ensuring capture…");
                let st = state.clone();
                thread::spawn(move || {
                    let msg = match setup::service_ensure(false) {
                        Ok(m) => format!("Capture: {m}"),
                        Err(e) => format!("Capture ensure failed: {e}"),
                    };
                    set_status(&st, &msg);
                    refresh_state(&st);
                });
            } else if message == "log" {
                let path = capture_log::capture_log_path();
                match open_path(&path) {
                    Ok(()) => set_status(&state, "Opened capture log"),
                    Err(e) => set_status(&state, &format!("Could not open log: {e}")),
                }
            } else if message == "dashboard" || message == "settings" {
                let page = if message == "settings" { "settings" } else { "overview" };
                match crate::desktop_open::request(page) {
                    Ok(()) => set_status(&state, "Opened Spanreed"),
                    Err(error) => set_status(&state, &format!("Could not open Spanreed: {error}")),
                }
            } else if message == "quit" {
                stop.store(true, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
            } else if let Some(url) = message.strip_prefix("buy ") {
                if crate::tray_popover::allowed_buy_url(url.trim()) {
                    let _ = open_url(url.trim());
                }
            } else if message == "reset" {
                redeem_from_card(&state);
            }
        }

        while let Ok(ev) = menu_channel.try_recv() {
            let id = ev.id;
            if id == id_dashboard || id == id_settings {
                let page = if id == id_settings { "settings" } else { "overview" };
                match crate::desktop_open::request(page) {
                    Ok(()) => set_status(&state, "Opened Spanreed"),
                    Err(error) => set_status(&state, &format!("Could not open Spanreed: {error}")),
                }
            } else if id == id_quit {
                stop.store(true, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
            } else if id == id_refresh {
                set_status(&state, "Refreshing usage…");
                let st = state.clone();
                thread::spawn(move || {
                    refresh_state(&st);
                    note_refreshed(&st);
                });
            } else if id == id_ensure {
                set_status(&state, "Ensuring capture…");
                let st = state.clone();
                thread::spawn(move || {
                    let msg = match setup::service_ensure(false) {
                        Ok(m) => {
                            log::info!("ensure: {m}");
                            format!("Capture: {m}")
                        }
                        Err(e) => {
                            log::warn!("ensure: {e}");
                            format!("Capture ensure failed: {e}")
                        }
                    };
                    set_status(&st, &msg);
                    refresh_state(&st);
                    user_notify("spanreed — capture", &msg, false);
                });
            } else if id == id_log {
                let path = capture_log::capture_log_path();
                match open_path(&path) {
                    Ok(()) => {
                        let msg = format!("Opened log:\n{}", path.display());
                        set_status(&state, "Opened capture log");
                        log::info!("tray: {msg}");
                    }
                    Err(e) => {
                        let msg = format!("Could not open log:\n{}\n\n{}", path.display(), e);
                        set_status(&state, "Failed to open capture log");
                        user_notify("spanreed — capture log", &msg, true);
                    }
                }
            } else if id == id_share_primary {
                let logged_in = state
                    .lock()
                    .map(|g| g.share_logged_in)
                    .unwrap_or(false);
                if logged_in {
                    set_status(&state, "Sharing usage…");
                    let st = state.clone();
                    thread::spawn(move || {
                        let msg = match share::share_once(false) {
                            Ok(m) => m,
                            Err(e) => e,
                        };
                        set_status(&st, &msg);
                        refresh_state(&st);
                        user_notify("spanreed — share", &msg, true);
                    });
                } else {
                    set_status(&state, "Starting share login…");
                    let st = state.clone();
                    thread::spawn(move || match share_session::start_device_login() {
                        Ok(pending) => {
                            let _ = open_url(&pending.verification_uri);
                            let code_msg = format!(
                                "Approve in the browser.\nCode: {}\n{}",
                                pending.user_code, pending.verification_uri
                            );
                            set_status(&st, &format!("Share code: {}", pending.user_code));
                            user_notify("spanreed — share login", &code_msg, true);
                            match share_session::wait_device_login(&pending) {
                                Ok(_) => {
                                    let _ = share_schedule::enable(false);
                                    set_status(&st, "Share linked");
                                    refresh_state(&st);
                                    user_notify(
                                        "spanreed — share",
                                        "Linked. Daily share can run without the browser.",
                                        true,
                                    );
                                }
                                Err(e) => {
                                    set_status(&st, &e);
                                    user_notify("spanreed — share login", &e, true);
                                }
                            }
                        }
                        Err(e) => {
                            set_status(&st, &e);
                            user_notify("spanreed — share login", &e, true);
                        }
                    });
                }
            } else if id == id_unlink {
                match share_session::clear() {
                    Ok(()) => {
                        set_status(&state, "Share unlinked");
                        refresh_state(&state);
                        user_notify("spanreed — share", "Local share session removed.", false);
                    }
                    Err(e) => {
                        set_status(&state, &e);
                        user_notify("spanreed — share", &e, true);
                    }
                }
            } else if id == id_check {
                item_check.set_text("Checking for updates…");
                set_status(&state, "Checking for updates…");
                let st = state.clone();
                thread::spawn(move || {
                    let summary = run_update_check(&st);
                    set_status(&st, &summary);
                    user_notify("spanreed — updates", &summary, true);
                });
            } else if id == id_update {
                if let Some(why) = self_update::apply_blocked_reason() {
                    set_status(&state, why);
                    user_notify("spanreed — updates", why, true);
                    continue;
                }
                set_status(&state, "Starting self-update…");
                let exe = std::env::current_exe().unwrap_or_default();
                let st = state.clone();
                thread::spawn(move || match Command::new(&exe)
                    .args(["self-update", "--yes"])
                    .status()
                {
                    Ok(s) if s.success() => {
                        set_status(&st, "Self-update finished — restart tray if the icon dies");
                        user_notify(
                            "spanreed — updates",
                            "Self-update finished.\n\
                             Capture was restarted when needed. Restart the tray if the icon is gone.",
                            true,
                        );
                    }
                    Ok(s) => {
                        let msg = format!("self-update exited {s}");
                        set_status(&st, &msg);
                        user_notify("spanreed — updates", &msg, true);
                    }
                    Err(e) => {
                        let msg = format!("Could not start self-update: {e}");
                        set_status(&st, &msg);
                        user_notify("spanreed — updates", &msg, true);
                    }
                });
            }
        }

        let dirty = state.lock().map(|guard| guard.dirty).unwrap_or(false);
        if dirty {
            apply_visual(
                &state,
                &mut tray,
                &item_update,
                &item_check,
                &item_share_primary,
                &item_unlink,
            );
            if popover.visible() {
                popover.load(usage_card(&state));
            }
        }
        if popover.visible() {
            let status = state
                .lock()
                .ok()
                .and_then(|guard| guard.status_note.clone());
            popover.sync_status(status.as_deref());
        }
    });
}

fn redeem_from_card(state: &Arc<Mutex<TrayState>>) {
    let request_id = {
        let mut guard = state.lock().unwrap_or_else(|error| error.into_inner());
        if guard.reset_in_flight {
            return;
        }
        if !guard.outputs.iter().any(crate::tray_card::can_use_reset) {
            stamp_status(&mut guard, "No limit reset credit is available");
            return;
        }
        if guard.reset_request_id.is_none() {
            match new_redeem_request_id() {
                Ok(id) => guard.reset_request_id = Some(id),
                Err(message) => {
                    stamp_status(&mut guard, message);
                    return;
                }
            }
        }
        guard.reset_in_flight = true;
        stamp_status(&mut guard, "Using reset…");
        guard
            .reset_request_id
            .clone()
            .unwrap_or_else(|| "invalid".into())
    };
    if !crate::providers::codex::valid_redeem_request_id(&request_id) {
        let mut guard = state.lock().unwrap_or_else(|error| error.into_inner());
        guard.reset_in_flight = false;
        guard.reset_request_id = None;
        stamp_status(&mut guard, "Could not start the reset");
        return;
    }
    let state = state.clone();
    thread::spawn(move || {
        let _reset = ResetFlight {
            state: state.clone(),
        };
        let result = crate::providers::codex::redeem_reset(&request_id);
        {
            let mut guard = state.lock().unwrap_or_else(|error| error.into_inner());
            guard.reset_in_flight = false;
            match result {
                Ok(()) => {
                    guard.reset_request_id = None;
                    stamp_status(&mut guard, "Reset used");
                }
                Err(message) => stamp_status(&mut guard, message),
            }
        }
        refresh_state(&state);
    });
}

struct ResetFlight {
    state: Arc<Mutex<TrayState>>,
}

impl Drop for ResetFlight {
    fn drop(&mut self) {
        let mut guard = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let mut in_flight = guard.reset_in_flight;
        let mut status = guard.status_note.clone();
        let before = status.clone();
        tray_format::abandon_reset(&mut in_flight, &mut status);
        guard.reset_in_flight = in_flight;
        guard.status_note = status;
        if guard.status_note != before {
            guard.status_at = Some(Instant::now());
            guard.dirty = true;
        }
    }
}

fn new_redeem_request_id() -> Result<String, &'static str> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|_| "Could not start the reset")?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut id = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            id.push('-');
        }
        id.push_str(&format!("{byte:02x}"));
    }
    Ok(id)
}

fn set_status(state: &Arc<Mutex<TrayState>>, note: &str) {
    let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
    stamp_status(&mut g, note);
}

fn note_refreshed(state: &Arc<Mutex<TrayState>>) {
    let mut guard = state.lock().unwrap_or_else(|error| error.into_inner());
    if guard.status_note.as_deref() == Some("Capture proxy is DOWN") {
        return;
    }
    stamp_status(&mut guard, "Usage refreshed");
}

fn stamp_status(guard: &mut TrayState, note: impl Into<String>) {
    guard.status_note = Some(note.into());
    guard.status_at = Some(Instant::now());
    guard.dirty = true;
}

fn refresh_state(state: &Arc<Mutex<TrayState>>) {
    let capture_up = setup::capture_ports_up();
    let outputs = api::fetch_cached().unwrap_or_else(probe::probe_detected);
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
    g.share_logged_in = share_session::is_logged_in();
    let today = util::today_day_key_madrid();
    g.share_line = tray_format::format_share_line(
        g.share_logged_in,
        share::last_shared_day().as_deref(),
        &today,
    );
    g.dirty = true;

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
                "spanreed — capture",
                "Capture proxy is DOWN. Run Ensure capture or `spanreed capture ensure`.",
                false,
            );
            return;
        }
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
            user_notify("spanreed — usage", body, false);
        }
    }
}

/// Run GitHub Releases check; returns a user-facing summary string.
fn run_update_check(state: &Arc<Mutex<TrayState>>) -> String {
    if crate::app::env_offline() {
        let msg = "Offline (SPANREED_OFFLINE=1) — not checking GitHub.".to_string();
        let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
        g.update_note = Some("Update: offline".into());
        g.dirty = true;
        return msg;
    }
    match self_update::check_for_update() {
        Ok(r) if r.newer => {
            let how = if let Some(why) = self_update::apply_blocked_reason() {
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

fn usage_card(state: &Arc<Mutex<TrayState>>) -> String {
    let guard = state.lock().unwrap_or_else(|error| error.into_inner());
    crate::tray_card::present(
        &guard.outputs,
        guard.capture_up,
        guard.status_note.as_deref(),
        crate::util::now_ms(),
    )
}

fn tooltip_from(state: &Arc<Mutex<TrayState>>) -> String {
    let g = state.lock().unwrap_or_else(|e| e.into_inner());
    tray_format::compose_tooltip(
        g.status_note.as_deref(),
        &g.share_line,
        &tray_format::format_tooltip(&g.outputs, g.capture_up, g.update_note.as_deref()),
    )
}

fn apply_visual(
    state: &Arc<Mutex<TrayState>>,
    tray: &mut TrayIcon,
    item_update: &MenuItem,
    item_check: &MenuItem,
    item_share_primary: &MenuItem,
    item_unlink: &MenuItem,
) {
    let (sev, tip, update_enabled, check_label, share_logged_in) = {
        let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
        g.dirty = false;
        let sev = tray_format::severity(g.capture_up, g.max_used);
        let tip = tray_format::compose_tooltip(
            g.status_note.as_deref(),
            &g.share_line,
            &tray_format::format_tooltip(&g.outputs, g.capture_up, g.update_note.as_deref()),
        );
        let update_enabled = self_update::can_apply_self_update()
            && g.update_note
                .as_deref()
                .map(|n| n.contains("available"))
                .unwrap_or(false);
        let check_label = MENU_CHECK.to_string();
        let share_logged_in = g.share_logged_in;
        (sev, tip, update_enabled, check_label, share_logged_in)
    };
    item_share_primary.set_text(if share_logged_in {
        MENU_SHARE_NOW
    } else {
        MENU_LINK_SHARE
    });
    item_unlink.set_enabled(share_logged_in);
    item_update.set_enabled(update_enabled);
    // Don't clobber "Checking…" if the menu item was set by the handler mid-flight
    // unless we're past that (handler restores MENU_CHECK after check).
    let current = item_check.text();
    if current != "Checking for updates…" {
        item_check.set_text(check_label);
    }
    let _ = tray.set_tooltip(Some(tip));
    if let Ok(icon) = icon_for_severity(sev) {
        let _ = tray.set_icon(Some(icon));
    }
}

fn icon_for_severity(sev: TraySeverity) -> Result<Icon, String> {
    let img = image::load_from_memory(MASTER_PNG)
        .map_err(|e| format!("decode spanreed png: {e}"))?
        .into_rgba8();
    let tinted = tint_rgba(img, sev.tint_rgba());
    let (w, h) = tinted.dimensions();
    Icon::from_rgba(tinted.into_raw(), w, h).map_err(|e| format!("icon: {e}"))
}

fn tint_rgba(mut img: RgbaImage, tint: [u8; 4]) -> RgbaImage {
    for p in img.pixels_mut() {
        let a = p.0[3];
        if a == 0 {
            continue;
        }
        p.0[0] = ((p.0[0] as u16 * tint[0] as u16) / 255) as u8;
        p.0[1] = ((p.0[1] as u16 * tint[1] as u16) / 255) as u8;
        p.0[2] = ((p.0[2] as u16 * tint[2] as u16) / 255) as u8;
        p.0[3] = ((a as u16 * tint[3] as u16) / 255) as u8;
    }
    img
}

/// Single-instance guard: create `…/spanreed/tray.lock` with our PID.
/// Dropped on process exit (RAII removes the file).
struct InstanceLock {
    path: std::path::PathBuf,
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn acquire_single_instance() -> Result<InstanceLock, String> {
    let dir = crate::app::data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir tray lock: {e}"))?;
    let path = dir.join(LOCK_FILE);
    if path.exists() {
        // Stale lock from a crashed tray: if the PID is gone, take over.
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(pid) = text.trim().parse::<u32>() {
                if process_alive(pid) {
                    return Err(format!(
                        "tray already running (pid {pid}); quit the existing icon first"
                    ));
                }
            }
        }
        let _ = std::fs::remove_file(&path);
    }
    std::fs::write(&path, format!("{}\n", std::process::id()))
        .map_err(|e| format!("write tray lock: {e}"))?;
    Ok(InstanceLock { path })
}

fn process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| {
                let s = String::from_utf8_lossy(&o.stdout);
                s.contains(&pid.to_string())
            })
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        false
    }
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let u = url.replace('\'', "''");
        let script = format!("Start-Process '{u}'");
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("powershell Start-Process: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
    #[cfg(target_os = "macos")]
    {
        let st = Command::new("open")
            .arg(url)
            .status()
            .map_err(|e| format!("open: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!("open exited {st}"))
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let st = Command::new("xdg-open")
            .arg(url)
            .status()
            .map_err(|e| format!("xdg-open: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!("xdg-open exited {st}"))
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err(format!("open url not supported: {url}"))
    }
}

/// Open a path with the OS default handler. Creates an empty file if missing.
///
/// On Windows the tray is windowless, so `cmd /C start` is unreliable; use
/// PowerShell `Start-Process` instead (same pattern as capture autostart).
pub fn open_path(path: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir log dir: {e}"))?;
    }
    if !path.exists() {
        std::fs::write(path, b"").map_err(|e| format!("create log file: {e}"))?;
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let p = path.display().to_string().replace('\'', "''");
        // LiteralPath opens with the default app for .log (usually Notepad).
        let script = format!("Start-Process -LiteralPath '{p}'");
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("powershell Start-Process: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(if err.trim().is_empty() {
                format!("Start-Process failed (status {})", out.status)
            } else {
                err.trim().to_string()
            });
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let st = Command::new("open")
            .arg(path)
            .status()
            .map_err(|e| format!("open: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!("open exited {st}"))
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let st = Command::new("xdg-open")
            .arg(path)
            .status()
            .map_err(|e| format!("xdg-open: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!("xdg-open exited {st}"))
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err(format!("open not supported: {}", path.display()))
    }
}

/// User-visible notification.
///
/// Every alert goes through the platform path: notify-send, a Windows toast,
/// or macOS Notification Center. macOS and Windows also show a dialog, because
/// a successful command does not prove the banner was shown.
fn user_notify(title: &str, body: &str, modal: bool) {
    log::info!("tray notify: {title}: {body}");
    if modal {
        eprintln!("spanreed tray: {title}: {body}");
    }
    let title = title.to_string();
    let body = body.to_string();
    thread::spawn(move || {
        let _handed_to_desktop = crate::desktop_open::hand_off_alert(&title, &body);
        // A desktop show can report success and then drop the alert. Every
        // platform still delivers through its own notification path.
        let delivered = crate::notifications::deliver_os(&title, &body);
        if !crate::notifications::notification_confirmed(std::env::consts::OS, delivered.is_ok()) {
            if let Err(error) = &delivered {
                log::warn!("tray notify failed: {error}");
            }
            #[cfg(windows)]
            show_windows_message(&title, &body);
            #[cfg(target_os = "macos")]
            show_macos_dialog(&title, &body);
            #[cfg(all(unix, not(target_os = "macos")))]
            show_linux_dialog(&title, &body);
        }
    });
}

#[cfg(windows)]
fn show_windows_message(title: &str, body: &str) {
    let t = title.replace('\'', "''");
    let b = body.replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName PresentationFramework; \
         [System.Windows.MessageBox]::Show('{b}','{t}') | Out-Null"
    );
    let _ = Command::new(crate::notifications::powershell_program())
        .args(["-NoProfile", "-Command", &script])
        .spawn();
}

#[cfg(all(unix, not(target_os = "macos")))]
fn show_linux_dialog(title: &str, body: &str) {
    let args = crate::notifications::zenity_dialog_args(title, body);
    let _ = Command::new("zenity").args(&args).spawn();
}

#[cfg(target_os = "macos")]
fn show_macos_dialog(title: &str, body: &str) {
    let script = crate::notifications::osascript_dialog(title, body);
    let _ = Command::new(crate::notifications::osascript_program())
        .args(["-e", &script])
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn open_path_creates_missing_file() {
        let dir = std::env::temp_dir().join(format!("spanreed-tray-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("test.log");
        // On CI/headless, Start-Process may still succeed for notepad association.
        // We only assert the file exists after the call (create side-effect).
        let _ = open_path(&path);
        assert!(
            path.is_file(),
            "open_path should create missing log file at {}",
            path.display()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn capture_log_path_is_under_spanreed_logs() {
        let p: PathBuf = capture_log::capture_log_path();
        let s = p.to_string_lossy();
        assert!(
            s.contains("spanreed") && s.contains("capture.log"),
            "unexpected log path {s}"
        );
    }
}
