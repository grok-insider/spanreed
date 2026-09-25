#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use tauri::Manager;
mod commands;
mod notifications;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(spanreed::app::AppContext::new())
        .invoke_handler(tauri::generate_handler![
            commands::take_desktop_route,
            commands::disconnect_usage_source,
            commands::connect_usage_source,
            commands::usage_report,
            commands::usage_sources,
            commands::save_usage_sources,
            commands::preview_hosted_client,
            commands::apply_hosted_client,
            commands::codex_session_move,
            commands::private_history,
            commands::sync_settings,
            commands::save_sync_settings,
            commands::sync_status,
            commands::sync_now,
            commands::link_codex_source,
            commands::publication_status,
            commands::publish_metrics,
            commands::set_publication_schedule,
            commands::remote_open_authorization,
            commands::remote_request,
            commands::fabrials_status,
            commands::fabrials_begin,
            commands::fabrials_poll,
            commands::fabrials_cancel,
            commands::fabrials_disconnect,
            commands::fabrials_open,
            commands::forget_migration,
            commands::begin_migration_authorization,
            commands::migration_authorizations,
            commands::preview_opencode_remove,
            commands::preview_opencode_update,
            commands::saved_migrations,
            commands::begin_inactive_device_login,
            commands::migration_inventory,
            commands::propose_migration,
            commands::execute_migration,
            commands::pair_migration,
            commands::migration_status,
            commands::cancel_migration,
            commands::migration_candidates,
            commands::preview_grok_configuration,
            commands::preview_opencode_configuration,
            commands::apply_client_configuration,
            commands::replace_api_key,
            commands::remove_account,
            commands::local_proxy_status,
            commands::start_local_proxy,
            commands::stop_local_proxy,
            notifications::test_reset_notification,
            notifications::notification_settings,
            notifications::set_reset_notifications,
            notifications::check_reset_notifications,
            commands::models,
            commands::reauthorize_account,
            commands::add_api_key,
            commands::hops,
            commands::history,
            commands::snapshot,
            commands::detection,
            commands::accounts,
            commands::privacy,
            commands::set_privacy,
            commands::activate_account,
            commands::open_hosted,
            commands::routing,
            commands::set_routing,
            commands::begin_device_login,
            commands::poll_device_login,
            commands::cancel_device_login,
            commands::open_device_login,
        ])
        .setup(|app| {
            spanreed::app::window::mark_running();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut applied = String::new();
                loop {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Some(href) = spanreed::app::window::peek() {
                    use tauri::Manager;
                    if let Some(window) = handle.get_webview_window("main") {
                        let _ = window.unminimize();
                        let _ = window.show();
                        let _ = window.set_focus();
                        if applied != href {
                            if let Some(script) = spanreed::app::window::route_location_script(&href) {
                                if window.eval(&script).is_ok() {
                                    applied.clone_from(&href);
                                }
                            }
                        }
                    }
                } else {
                    applied.clear();
                }
                if let Some(alert) = spanreed::app::window::take_alert() {
                    use tauri::plugin::PermissionState;
                    use tauri_plugin_notification::NotificationExt;
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
                        spanreed::app::window::ack_alert(&alert.id);
                    }
                }
                }));
                std::thread::sleep(std::time::Duration::from_millis(200));
                }
            });
            let show = tauri::menu::MenuItem::with_id(app, "show", "Open dashboard", true, None::<&str>)?;
            let settings = tauri::menu::MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let quit = tauri::menu::MenuItem::with_id(app, "quit", "Quit Spanreed", true, None::<&str>)?;
            let menu = tauri::menu::Menu::with_items(app, &[&show, &settings, &quit])?;
            tauri::tray::TrayIconBuilder::new().menu(&menu).tooltip("Spanreed")
                .icon(app.default_window_icon().expect("bundled application icon").clone())
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
                    let _ = spanreed::app::window::open(page);
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.unminimize();
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }).build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("could not start Spanreed desktop")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                spanreed::app::window::unmark_running();
            }
        });
}
