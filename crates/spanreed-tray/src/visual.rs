//! Tray icon tint, title and tooltip, and the menu labels that follow state.

use image::RgbaImage;
use tray_icon::{Icon, TrayIcon};

use super::menu::TrayMenu;
use super::state::{Shared, context};
use super::{MENU_AGENT_START, MENU_AGENT_STOP, MENU_CHECK, MENU_LINK_SHARE, MENU_SHARE_NOW};
use spanreed_app::app;
use spanreed_domain::tray_format::{self, TraySeverity};

const MASTER_PNG: &[u8] = include_bytes!("assets/tray/spanreed-32.png");

pub(super) fn apply_visual(state: &Shared, tray: &mut TrayIcon, menu: &TrayMenu) {
    let item_update = &menu.update;
    let item_check = &menu.check;
    let item_share_primary = &menu.share;
    let item_unlink = &menu.unlink;
    let ctx = context(state);
    let (sev, tip, title, update_enabled, check_label, share_logged_in) = {
        let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
        g.dirty = false;
        let sev = tray_format::severity(g.capture_up, g.max_used);
        let title = tray_format::indicator_title(g.status_note.as_deref(), app::PRODUCT_NAME);
        let tip = tray_format::compose_tooltip(
            g.status_note.as_deref(),
            &g.share_line,
            &tray_format::format_tooltip(&g.outputs, g.capture_up, g.update_note.as_deref()),
        );
        let update_enabled = app::updates::can_apply_self_update(&ctx)
            && g.update_note
                .as_deref()
                .map(|n| n.contains("available"))
                .unwrap_or(false);
        let check_label = MENU_CHECK.to_string();
        let share_logged_in = g.share_logged_in;
        (
            sev,
            tip,
            title,
            update_enabled,
            check_label,
            share_logged_in,
        )
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
    let agent_running = app::agent::running(&ctx);
    menu.agent.set_text(if agent_running {
        MENU_AGENT_STOP
    } else {
        MENU_AGENT_START
    });
    let _ = tray.set_tooltip(Some(tip));
    tray.set_title(Some(title));
    if let Ok(icon) = icon_for_severity(sev) {
        let _ = tray.set_icon(Some(icon));
    }
}

pub(super) fn icon_for_severity(sev: TraySeverity) -> Result<Icon, String> {
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
