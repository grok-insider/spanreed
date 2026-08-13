//! System tray companion: Behelit icon, usage tooltip, capture health, updates.
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
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::api;
use crate::capture_log;
use crate::model::ProviderOutput;
use crate::probe;
use crate::self_update;
use crate::setup;
use crate::tray_format::{self, TraySeverity};

const DEFAULT_INTERVAL_SECS: u64 = 60;
const MASTER_PNG: &[u8] = include_bytes!("assets/tray/behelit-32.png");
const LOCK_FILE: &str = "tray.lock";

/// Menu labels (kept in one place so docs/tests stay aligned).
pub const MENU_REFRESH: &str = "Refresh now";
pub const MENU_ENSURE: &str = "Ensure capture";
pub const MENU_LOG: &str = "Open capture log";
pub const MENU_CHECK: &str = "Check for updates";
pub const MENU_UPDATE: &str = "Install update…";
pub const MENU_QUIT: &str = "Quit tray";

struct TrayState {
    outputs: Vec<ProviderOutput>,
    capture_up: bool,
    max_used: Option<f64>,
    update_note: Option<String>,
    /// Short status line shown at the top of the tooltip (action feedback).
    status_note: Option<String>,
    last_notify_proxy: Option<Instant>,
    last_notify_quota: Option<Instant>,
    /// Background thread sets this; UI thread clears after repaint.
    dirty: bool,
}

impl Default for TrayState {
    fn default() -> Self {
        Self {
            outputs: Vec::new(),
            capture_up: false,
            max_used: None,
            update_note: None,
            status_note: None,
            last_notify_proxy: None,
            last_notify_quota: None,
            dirty: true,
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
                    "spanreed tray — system tray status (Behelit icon)\n\n\
                     \t--interval S   Refresh every S seconds (default {DEFAULT_INTERVAL_SECS})\n\
                     Menu: Refresh, Ensure capture, Open log, Check/Install update, Quit tray"
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
    let item_quit = MenuItem::new(MENU_QUIT, true, None);
    menu.append(&item_refresh)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_ensure)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_log).map_err(|e| format!("menu: {e}"))?;
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
    let id_check = item_check.id().clone();
    let id_update = item_update.id().clone();
    let id_quit = item_quit.id().clone();

    let state = Arc::new(Mutex::new(TrayState::default()));
    // First probe on main thread so tooltip is ready.
    refresh_state(&state);

    let icon = icon_for_severity(TraySeverity::Ok)?;
    let mut tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip_from(&state))
        .with_icon(icon)
        .with_title("spanreed")
        .build()
        .map_err(|e| format!("tray icon: {e}"))?;

    apply_visual(&state, &mut tray, &item_update, &item_check);

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

    let menu_channel = MenuEvent::receiver();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250));

        if let Event::NewEvents(_) = event {
            let dirty = state.lock().map(|g| g.dirty).unwrap_or(false);
            if dirty {
                apply_visual(&state, &mut tray, &item_update, &item_check);
            }
        }

        while let Ok(ev) = menu_channel.try_recv() {
            let id = ev.id;
            if id == id_quit {
                stop.store(true, Ordering::Relaxed);
                *control_flow = ControlFlow::Exit;
            } else if id == id_refresh {
                set_status(&state, "Refreshing usage…");
                apply_visual(&state, &mut tray, &item_update, &item_check);
                let st = state.clone();
                thread::spawn(move || {
                    refresh_state(&st);
                    set_status(&st, "Usage refreshed");
                });
            } else if id == id_ensure {
                set_status(&state, "Ensuring capture…");
                apply_visual(&state, &mut tray, &item_update, &item_check);
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
                        apply_visual(&state, &mut tray, &item_update, &item_check);
                        log::info!("tray: {msg}");
                    }
                    Err(e) => {
                        let msg = format!("Could not open log:\n{}\n\n{}", path.display(), e);
                        set_status(&state, "Failed to open capture log");
                        apply_visual(&state, &mut tray, &item_update, &item_check);
                        user_notify("spanreed — capture log", &msg, true);
                    }
                }
            } else if id == id_check {
                item_check.set_text("Checking for updates…");
                set_status(&state, "Checking for updates…");
                apply_visual(&state, &mut tray, &item_update, &item_check);
                let st = state.clone();
                thread::spawn(move || {
                    let summary = run_update_check(&st);
                    set_status(&st, &summary);
                    user_notify("spanreed — updates", &summary, true);
                });
            } else if id == id_update {
                if let Some(why) = self_update::apply_blocked_reason() {
                    set_status(&state, why);
                    apply_visual(&state, &mut tray, &item_update, &item_check);
                    user_notify("spanreed — updates", why, true);
                    continue;
                }
                set_status(&state, "Starting self-update…");
                apply_visual(&state, &mut tray, &item_update, &item_check);
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
    });
}

fn set_status(state: &Arc<Mutex<TrayState>>, note: &str) {
    let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
    g.status_note = Some(note.to_string());
    g.dirty = true;
}

fn refresh_state(state: &Arc<Mutex<TrayState>>) {
    let capture_up = setup::capture_ports_up();
    let outputs = api::fetch_cached().unwrap_or_else(probe::probe_detected);
    let max_used = tray_format::max_used_pct(&outputs);

    let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
    let prev_used = g.max_used;
    let prev_up = g.capture_up;
    g.capture_up = capture_up;
    g.outputs = outputs;
    g.max_used = max_used;
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
            g.status_note = Some("Capture proxy is DOWN".into());
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
        }
    }
}

/// Run GitHub Releases check; returns a user-facing summary string.
fn run_update_check(state: &Arc<Mutex<TrayState>>) -> String {
    if std::env::var_os("SPANREED_OFFLINE").is_some() {
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

fn tooltip_from(state: &Arc<Mutex<TrayState>>) -> String {
    let g = state.lock().unwrap_or_else(|e| e.into_inner());
    let mut parts = Vec::new();
    if let Some(s) = g.status_note.as_deref() {
        if !s.is_empty() {
            parts.push(s.to_string());
        }
    }
    parts.push(tray_format::format_tooltip(
        &g.outputs,
        g.capture_up,
        g.update_note.as_deref(),
    ));
    parts.join("\n")
}

fn apply_visual(
    state: &Arc<Mutex<TrayState>>,
    tray: &mut TrayIcon,
    item_update: &MenuItem,
    item_check: &MenuItem,
) {
    let (sev, tip, update_enabled, check_label) = {
        let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
        g.dirty = false;
        let sev = tray_format::severity(g.capture_up, g.max_used);
        let mut tip_parts = Vec::new();
        if let Some(s) = g.status_note.as_deref() {
            if !s.is_empty() {
                tip_parts.push(s.to_string());
            }
        }
        tip_parts.push(tray_format::format_tooltip(
            &g.outputs,
            g.capture_up,
            g.update_note.as_deref(),
        ));
        let tip = tip_parts.join("\n");
        let update_enabled = self_update::can_apply_self_update()
            && g.update_note
                .as_deref()
                .map(|n| n.contains("available"))
                .unwrap_or(false);
        // Keep check label stable unless mid-check (caller sets text).
        let check_label = MENU_CHECK.to_string();
        let _ = &g;
        (sev, tip, update_enabled, check_label)
    };
    item_update.set_enabled(update_enabled);
    // Don't clobber "Checking…" if the menu item was set by the handler mid-flight
    // unless we're past that (handler restores MENU_CHECK after check).
    let current = item_check.text();
    if current != "Checking for updates…" {
        item_check.set_text(check_label);
    }
    if let Ok(icon) = icon_for_severity(sev) {
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_tooltip(Some(tip));
    }
}

fn icon_for_severity(sev: TraySeverity) -> Result<Icon, String> {
    let img = image::load_from_memory(MASTER_PNG)
        .map_err(|e| format!("decode behelit png: {e}"))?
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
    let dir = crate::creds::data_home().join("spanreed");
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
/// `modal`: Windows MessageBox for long copy (update check). Refresh/ensure use
/// tooltip + Linux `notify-send` only — do not spawn WPF for every click.
fn user_notify(title: &str, body: &str, modal: bool) {
    log::info!("tray notify: {title}: {body}");
    #[cfg(windows)]
    {
        if !modal {
            return;
        }
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let t = title.replace('\'', "''");
        let b = body.replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName PresentationFramework; \
             [System.Windows.MessageBox]::Show('{b}','{t}') | Out-Null"
        );
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let short = if body.len() > 280 {
            format!("{}…", body.chars().take(277).collect::<String>())
        } else {
            body.to_string()
        };
        let _ = Command::new("notify-send")
            .args(["-a", "spanreed", "--", title, &short])
            .spawn();
        if modal {
            eprintln!("spanreed tray: {title}: {body}");
        }
    }
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
