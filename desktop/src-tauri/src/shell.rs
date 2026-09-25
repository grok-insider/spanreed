//! Window routing requested by other front ends, alert hand-off, and the
//! desktop tray icon.
use spanreed::app::window;
use tauri::Manager;
use tauri::plugin::PermissionState;
use tauri_plugin_notification::NotificationExt;

/// Follow routes and alerts that the CLI or the tray hand to this window.
pub fn watch_routes(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut applied = String::new();
        loop {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                apply_route(&handle, &mut applied);
                deliver_alert(&handle);
            }));
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    });
}

fn apply_route(handle: &tauri::AppHandle, applied: &mut String) {
    let Some(href) = window::peek() else {
        applied.clear();
        return;
    };
    let Some(main) = handle.get_webview_window("main") else {
        return;
    };
    let _ = main.unminimize();
    let _ = main.show();
    let _ = main.set_focus();
    if *applied != href
        && let Some(script) = window::route_location_script(&href)
        && main.eval(&script).is_ok()
    {
        applied.clone_from(&href);
    }
}

fn deliver_alert(handle: &tauri::AppHandle) {
    let Some(alert) = window::take_alert() else {
        return;
    };
    let permitted = handle
        .notification()
        .permission_state()
        .ok()
        .is_some_and(|state| state == PermissionState::Granted);
    let shown = permitted
        && handle
            .notification()
            .builder()
            .title(alert.title)
            .body(alert.body)
            .show()
            .is_ok();
    if shown {
        window::ack_alert(&alert.id);
    }
}

pub fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = tauri::menu::MenuItem::with_id(app, "show", "Open dashboard", true, None::<&str>)?;
    let settings = tauri::menu::MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = tauri::menu::MenuItem::with_id(app, "quit", "Quit Spanreed", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(app, &[&show, &settings, &quit])?;
    tauri::tray::TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Spanreed")
        .icon(
            app.default_window_icon()
                .expect("bundled application icon")
                .clone(),
        )
        .on_menu_event(|app, event| {
            let page = match event.id.as_ref() {
                "show" => "overview",
                "settings" => "settings",
                "quit" => {
                    app.exit(0);
                    return;
                }
                _ => return,
            };
            let _ = window::open(page);
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.unminimize();
                let _ = main.show();
                let _ = main.set_focus();
            }
        })
        .build(app)?;
    Ok(())
}
