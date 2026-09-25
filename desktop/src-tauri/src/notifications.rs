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
pub async fn check_reset_notifications(
    _app: tauri::AppHandle,
    ctx: tauri::State<'_, spanreed::context::AppContext>,
) -> Result<u32, String> {
    let ctx = ctx.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        spanreed::desktop::deliver_notifications(&ctx, spanreed::notifications::deliver_user_visible)
    })
    .await
    .map_err(|_| "Notification worker stopped".to_string())?
}

#[tauri::command]
pub async fn test_reset_notification(_app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        if !spanreed::notifications::settings()?.reset_expiry {
            return Err("Enable and save reset notifications first".into());
        }
        spanreed::notifications::deliver_user_visible(
            "Spanreed notification test",
            "System notifications are working. This is a test, not a reset expiry alert.",
        )
    })
    .await
    .map_err(|_| "Notification worker stopped".to_string())?
}
