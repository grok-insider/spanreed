use crate::commands::{Ctx, blocking};
use spanreed::app;
use tauri::plugin::PermissionState;
use tauri_plugin_notification::NotificationExt;

const NOTIFICATION: &str = "Notification worker stopped";

#[tauri::command]
pub async fn notification_settings() -> Result<app::notifications::Settings, String> {
    blocking(NOTIFICATION, app::notifications::settings).await
}

#[tauri::command]
pub async fn set_reset_notifications(
    handle: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    blocking(NOTIFICATION, move || {
        if enabled
            && handle
                .notification()
                .request_permission()
                .map_err(|_| "Could not request notification permission")?
                != PermissionState::Granted
        {
            return Err("Notifications are not permitted by the operating system".into());
        }
        app::notifications::set_enabled(enabled)
    })
    .await
}

#[tauri::command]
pub async fn check_reset_notifications(
    _handle: tauri::AppHandle,
    ctx: Ctx<'_>,
) -> Result<u32, String> {
    let ctx = ctx.inner().clone();
    blocking(NOTIFICATION, move || {
        app::notifications::check(&ctx, app::notifications::deliver_user_visible)
    })
    .await
}

#[tauri::command]
pub async fn test_reset_notification(_handle: tauri::AppHandle) -> Result<(), String> {
    blocking(NOTIFICATION, || {
        app::notifications::test(app::notifications::deliver_user_visible)
    })
    .await
}
