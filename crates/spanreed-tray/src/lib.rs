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
//!
//! Layout: `menu` (items and actions), `state` (shared state and refresh),
//! `actions` (what each action does, through `spanreed_app::app`), `visual` (icon and
//! tooltip), `popover` (the usage card window), `platform` (instance lock,
//! opening URLs/files, notifications).

#![cfg(feature = "tray")]

mod actions;
mod menu;
mod platform;
mod popover;
mod state;
mod visual;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tray_icon::menu::MenuEvent;
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use menu::{Action, TrayMenu};
use spanreed_app::app::AppContext;
use spanreed_domain::tray_format::TraySeverity;
use state::{Shared, TrayState};

pub const DEFAULT_INTERVAL_SECS: u64 = 60;
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
pub const MENU_AGENT_START: &str = "Start agent host";
pub const MENU_AGENT_STOP: &str = "Stop agent host";
pub const MENU_QUIT: &str = "Quit tray";

/// Run the tray until Quit; refreshes usage every `interval_secs`.
pub fn run(ctx: AppContext, interval_secs: u64) -> Result<(), String> {
    let _lock = platform::acquire_single_instance(&ctx.paths().data)?;
    let event_loop = EventLoopBuilder::new().build();
    let (context_menu, menu) = TrayMenu::build()?;
    let state = Arc::new(Mutex::new(TrayState::new(ctx)));
    // First probe on main thread so tooltip is ready.
    state::refresh_state(&state);

    let popover = popover::Popover::new(&event_loop)?;
    let mut tray = TrayIconBuilder::new()
        .with_menu(Box::new(context_menu))
        .with_menu_on_left_click(false)
        .with_tooltip(state::tooltip_from(&state))
        .with_icon(visual::icon_for_severity(TraySeverity::Ok)?)
        .with_title(spanreed_app::app::PRODUCT_NAME)
        .build()
        .map_err(|e| format!("tray icon: {e}"))?;
    visual::apply_visual(&state, &mut tray, &menu);

    let stop = Arc::new(AtomicBool::new(false));
    spawn_refresher(&state, &stop, interval_secs);
    spawn_ticker(event_loop.create_proxy(), &stop);

    let menu_channel = MenuEvent::receiver();
    let click_channel = TrayIconEvent::receiver();
    let mut card = CardView {
        popover,
        loaded_epoch: 0,
        blur_close_at: None,
    };
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250));
        state::expire_status(&state);
        card.on_focus(&event);
        while let Ok(click) = click_channel.try_recv() {
            card.on_click(click, &state);
        }
        while let Some(message) = card.popover.poll() {
            card.blur_close_at = None;
            if actions::popover_message(&message, &state, &mut card.popover) == Flow::Quit {
                stop.store(true, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
            }
        }
        while let Ok(event) = menu_channel.try_recv() {
            match menu.action(&event.id) {
                Some(Action::Quit) => {
                    stop.store(true, Ordering::Relaxed);
                    *control_flow = ControlFlow::Exit;
                }
                Some(action) => actions::menu_action(action, &state, &menu),
                None => {}
            }
        }
        card.close_after_blur();
        card.repaint(&state, &mut tray, &menu);
    });
}

#[derive(PartialEq, Eq)]
enum Flow {
    Continue,
    Quit,
}

/// Background: probe only (TrayIcon/MenuItem are !Send).
fn spawn_refresher(state: &Shared, stop: &Arc<AtomicBool>, interval_secs: u64) {
    let stop = stop.clone();
    let state = state.clone();
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_secs(interval_secs));
            if stop.load(Ordering::Relaxed) {
                break;
            }
            state::refresh_state(&state);
        }
    });
}

/// Wakes the event loop so status expiry and repaints run without input.
fn spawn_ticker(proxy: EventLoopProxy<()>, stop: &Arc<AtomicBool>) {
    let stop = stop.clone();
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(250));
            if proxy.send_event(()).is_err() {
                break;
            }
        }
    });
}

/// The usage card popover and what it last showed.
struct CardView {
    popover: popover::Popover,
    loaded_epoch: u64,
    blur_close_at: Option<Instant>,
}

impl CardView {
    fn on_focus(&mut self, event: &Event<()>) {
        if let Event::WindowEvent {
            event: WindowEvent::Focused(focused),
            ..
        } = event
        {
            if *focused {
                self.blur_close_at = None;
            } else if self.popover.blur_should_close() {
                // A click inside the card can blur the window before the button
                // message arrives. Hide on the next tick unless that message comes.
                self.blur_close_at = Some(Instant::now() + Duration::from_millis(200));
            }
        }
    }

    fn on_click(&mut self, click: TrayIconEvent, state: &Shared) {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            rect,
            ..
        } = click
        {
            self.popover.toggle(rect, state::usage_card(state));
            if self.popover.visible() {
                self.loaded_epoch = state::content_epoch(state).unwrap_or(self.loaded_epoch);
            }
        }
    }

    fn close_after_blur(&mut self) {
        if self.blur_close_at.is_some_and(|at| Instant::now() >= at) {
            self.popover.hide();
            self.blur_close_at = None;
        }
    }

    fn repaint(&mut self, state: &Shared, tray: &mut TrayIcon, menu: &TrayMenu) {
        let dirty = state.lock().map(|guard| guard.dirty).unwrap_or(false);
        if dirty {
            visual::apply_visual(state, tray, menu);
            if self.popover.visible() {
                let epoch = state::content_epoch(state).unwrap_or(self.loaded_epoch);
                if epoch != self.loaded_epoch {
                    self.popover.load(state::usage_card(state));
                    self.loaded_epoch = epoch;
                }
            }
        }
        if self.popover.visible() {
            let status = state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .status_note
                .clone();
            self.popover.sync_status(status.as_deref());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::platform::open_path;
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
        let p: PathBuf = spanreed_app::app::capture::log_path();
        let s = p.to_string_lossy();
        assert!(
            s.contains("spanreed") && s.contains("capture.log"),
            "unexpected log path {s}"
        );
    }
}
