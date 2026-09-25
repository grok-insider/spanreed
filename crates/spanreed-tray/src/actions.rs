//! What each tray action does. Work goes through `spanreed_app::app`; results are
//! shown as tray status and, for longer ones, a notification.

use std::process::Command;
use std::thread;
use std::time::Instant;

use super::Flow;
use super::menu::{Action, TrayMenu};
use super::platform::{open_path, open_url, user_notify};
use super::popover::{Popover, allowed_buy_url};
use super::state::{
    Shared, begin_refresh, refresh_state, run_update_check, set_status, stamp_status,
};
use spanreed_app::app;
use spanreed_domain::tray_format;

/// A message posted by the usage card.
pub(super) fn popover_message(message: &str, state: &Shared, popover: &mut Popover) -> Flow {
    match message {
        "close" => popover.hide(),
        "refresh" => begin_refresh(state),
        "ensure" => ensure_capture(state, false),
        "log" => open_log(state, false),
        "dashboard" => open_window(state, "overview"),
        "settings" => open_window(state, "settings"),
        "quit" => return Flow::Quit,
        "reset" => redeem_from_card(state),
        other => {
            if let Some(url) = other.strip_prefix("buy ")
                && allowed_buy_url(url.trim())
            {
                let _ = open_url(url.trim());
            }
        }
    }
    Flow::Continue
}

/// A context menu item other than Quit.
pub(super) fn menu_action(action: Action, state: &Shared, menu: &TrayMenu) {
    match action {
        Action::Dashboard => open_window(state, "overview"),
        Action::Settings => open_window(state, "settings"),
        Action::Refresh => begin_refresh(state),
        Action::EnsureCapture => ensure_capture(state, true),
        Action::OpenLog => open_log(state, true),
        Action::Share => share(state),
        Action::Unlink => unlink(state),
        Action::CheckUpdates => {
            menu.check.set_text("Checking for updates…");
            check_updates(state);
        }
        Action::InstallUpdate => install_update(state),
        Action::AgentHost => toggle_agent_host(state),
        Action::Quit => {}
    }
}

fn open_window(state: &Shared, page: &str) {
    match app::window::open(page) {
        Ok(()) => set_status(state, "Opened Spanreed"),
        Err(error) => set_status(state, &format!("Could not open Spanreed: {error}")),
    }
}

/// `from_menu` also logs and notifies the result.
fn ensure_capture(state: &Shared, from_menu: bool) {
    set_status(state, "Ensuring capture…");
    let state = state.clone();
    thread::spawn(move || {
        let message = match app::capture::ensure(false) {
            Ok(m) => {
                if from_menu {
                    log::info!("ensure: {m}");
                }
                format!("Capture: {m}")
            }
            Err(e) => {
                if from_menu {
                    log::warn!("ensure: {e}");
                }
                format!("Capture ensure failed: {e}")
            }
        };
        set_status(&state, &message);
        refresh_state(&state);
        if from_menu {
            user_notify("spanreed — capture", &message, false);
        }
    });
}

/// `from_menu` reports a failure with a dialog instead of only the status.
fn open_log(state: &Shared, from_menu: bool) {
    let path = app::capture::log_path();
    match open_path(&path) {
        Ok(()) => {
            set_status(state, "Opened capture log");
            if from_menu {
                log::info!("tray: Opened log:\n{}", path.display());
            }
        }
        Err(e) if from_menu => {
            let message = format!("Could not open log:\n{}\n\n{}", path.display(), e);
            set_status(state, "Failed to open capture log");
            user_notify("spanreed — capture log", &message, true);
        }
        Err(e) => set_status(state, &format!("Could not open log: {e}")),
    }
}

/// Share now when linked; otherwise start the share login.
fn share(state: &Shared) {
    let linked = state.lock().map(|g| g.share_logged_in).unwrap_or(false);
    if linked {
        set_status(state, "Sharing usage…");
        let state = state.clone();
        thread::spawn(move || {
            let ctx = state.lock().unwrap_or_else(|e| e.into_inner()).ctx.clone();
            let message = app::sharing::share_now(&ctx, false).unwrap_or_else(|e| e);
            set_status(&state, &message);
            refresh_state(&state);
            user_notify("spanreed — share", &message, true);
        });
        return;
    }
    set_status(state, "Starting share login…");
    let state = state.clone();
    thread::spawn(move || link_share(&state));
}

fn link_share(state: &Shared) {
    let pending = match app::sharing::begin_link() {
        Ok(pending) => pending,
        Err(e) => {
            set_status(state, &e);
            user_notify("spanreed — share login", &e, true);
            return;
        }
    };
    let _ = open_url(&pending.verification_uri);
    let code_message = format!(
        "Approve in the browser.\nCode: {}\n{}",
        pending.user_code, pending.verification_uri
    );
    set_status(state, &format!("Share code: {}", pending.user_code));
    user_notify("spanreed — share login", &code_message, true);
    match app::sharing::finish_link(&pending) {
        Ok(()) => {
            set_status(state, "Share linked");
            refresh_state(state);
            user_notify(
                "spanreed — share",
                "Linked. Daily share can run without the browser.",
                true,
            );
        }
        Err(e) => {
            set_status(state, &e);
            user_notify("spanreed — share login", &e, true);
        }
    }
}

fn unlink(state: &Shared) {
    match app::sharing::unlink() {
        Ok(()) => {
            set_status(state, "Share unlinked");
            refresh_state(state);
            user_notify("spanreed — share", "Local share session removed.", false);
        }
        Err(e) => {
            set_status(state, &e);
            user_notify("spanreed — share", &e, true);
        }
    }
}

fn check_updates(state: &Shared) {
    set_status(state, "Checking for updates…");
    let state = state.clone();
    thread::spawn(move || {
        let summary = run_update_check(&state);
        set_status(&state, &summary);
        user_notify("spanreed — updates", &summary, true);
    });
}

/// Start the agent host for desktop.grok.me on its own port, or stop it.
fn toggle_agent_host(state: &Shared) {
    let ctx = state.lock().unwrap_or_else(|e| e.into_inner()).ctx.clone();
    let running = app::agent::status(&ctx)
        .is_ok_and(|status| status.state == app::proxy::ProxyState::Running);
    set_status(
        state,
        if running {
            "Stopping agent host…"
        } else {
            "Starting agent host…"
        },
    );
    let state = state.clone();
    thread::spawn(move || {
        let message = if running {
            app::agent::stop(&ctx).map(|_| "Agent host stopped".to_string())
        } else {
            app::agent::start(&ctx).map(|status| match status.origin {
                Some(origin) => format!("Agent host listening on {origin}"),
                None => "Agent host starting".into(),
            })
        }
        .unwrap_or_else(|error| format!("Agent host: {error}"));
        set_status(&state, &message);
        user_notify("spanreed — agent host", &message, false);
    });
}

fn install_update(state: &Shared) {
    if let Some(why) = app::updates::apply_blocked_reason() {
        set_status(state, why);
        user_notify("spanreed — updates", why, true);
        return;
    }
    set_status(state, "Starting self-update…");
    let exe = std::env::current_exe().unwrap_or_default();
    let state = state.clone();
    thread::spawn(move || {
        let message = match Command::new(&exe).args(["self-update", "--yes"]).status() {
            Ok(s) if s.success() => {
                set_status(
                    &state,
                    "Self-update finished — restart tray if the icon dies",
                );
                user_notify(
                    "spanreed — updates",
                    "Self-update finished.\n\
                     Capture was restarted when needed. Restart the tray if the icon is gone.",
                    true,
                );
                return;
            }
            Ok(s) => format!("self-update exited {s}"),
            Err(e) => format!("Could not start self-update: {e}"),
        };
        set_status(&state, &message);
        user_notify("spanreed — updates", &message, true);
    });
}

fn redeem_from_card(state: &Shared) {
    let request_id = {
        let mut guard = state.lock().unwrap_or_else(|error| error.into_inner());
        if guard.reset_in_flight {
            return;
        }
        if !guard.outputs.iter().any(app::usage::can_use_reset) {
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
    if !app::accounts::valid_reset_request(&request_id) {
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
        let result = app::accounts::redeem_codex_reset(&request_id);
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
    state: Shared,
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
            guard.content_epoch = guard.content_epoch.wrapping_add(1);
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
