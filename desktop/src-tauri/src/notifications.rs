use tauri::plugin::PermissionState;
use tauri_plugin_notification::NotificationExt;

#[tauri::command]
pub async fn notification_settings() -> Result<spanreed::notifications::Settings, String> {
    tauri::async_runtime::spawn_blocking(spanreed::notifications::settings)
        .await
        .map_err(|_| "Notification worker stopped".to_string())?
}
#[tauri::command]
pub async fn set_reset_notifications(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if enabled
            && app
                .notification()
                .request_permission()
                .map_err(|_| "Could not request notification permission")?
                != PermissionState::Granted
        {
            return Err("Notifications are not permitted by the operating system".into());
        }
        spanreed::notifications::set_enabled(enabled)
    })
    .await
    .map_err(|_| "Notification worker stopped".to_string())?
}
#[tauri::command]
pub async fn check_reset_notifications(app: tauri::AppHandle) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !spanreed::notifications::settings()?.reset_expiry {
            return Ok(0);
        }
        if app
            .notification()
            .permission_state()
            .map_err(|_| "Notification permission unavailable")?
            != PermissionState::Granted
        {
            return Err("Notifications are disabled by the operating system".into());
        }
        spanreed::notifications::deliver(|title, body| {
            app.notification()
                .builder()
                .title(title)
                .body(body)
                .show()
                .map_err(|_| "Could not deliver reset notification".into())
        })
    })
    .await
    .map_err(|_| "Notification worker stopped".to_string())?
}

#[tauri::command]
pub async fn test_reset_notification(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !spanreed::notifications::settings()?.reset_expiry {
            return Err("Enable and save reset notifications first".into());
        }
        if app
            .notification()
            .permission_state()
            .map_err(|_| "Notification permission unavailable")?
            != PermissionState::Granted
        {
            return Err("Notifications are disabled by the operating system".into());
        }
        app.notification()
            .builder()
            .title("Spanreed notification test")
            .body("System notifications are working. This is a test, not a reset expiry alert.")
            .show()
            .map_err(|_| "Could not send test notification".into())
    })
    .await
    .map_err(|_| "Notification worker stopped".to_string())?
}
