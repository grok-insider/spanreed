use crate::commands::{Ctx, blocking};
use spanreed::app;
use tauri::plugin::PermissionState;
use tauri_plugin_notification::NotificationExt;

const NOTIFICATION: &str = "Notification worker stopped";

#[tauri::command]
pub async fn notification_settings(ctx: Ctx<'_>) -> Result<app::notifications::Settings, String> {
    let ctx = ctx.inner().clone();
    blocking(NOTIFICATION, move || app::notifications::settings(&ctx)).await
}

#[tauri::command]
pub async fn set_reset_notifications(
    ctx: Ctx<'_>,
    handle: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let ctx = ctx.inner().clone();
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
        app::notifications::set_enabled(&ctx, enabled)
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
        app::notifications::check_user_visible(&ctx)
    })
    .await
}

#[tauri::command]
pub async fn test_reset_notification(
    ctx: Ctx<'_>,
    _handle: tauri::AppHandle,
) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(NOTIFICATION, move || app::notifications::test(&ctx)).await
}
