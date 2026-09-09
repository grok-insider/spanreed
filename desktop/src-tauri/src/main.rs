#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use tauri::Manager;
mod notifications;

#[tauri::command]
async fn forget_migration(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::migration::session::forget(&id)).await.map_err(|_| "Migration worker stopped".to_string())?
}

#[tauri::command]
async fn begin_migration_authorization(id: String, source_id: String) -> Result<spanreed::account_login::LoginView, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::migration::session::begin_authorization(&id, &source_id)).await.map_err(|_| "Authorization worker stopped".to_string())?
}
#[tauri::command]
async fn migration_authorizations(id: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::migration::session::authorizations(&id)).await.map_err(|_| "Migration worker stopped".to_string())?
}

#[tauri::command]
async fn preview_opencode_remove(provider:String,alias:String)->Result<spanreed::client_configuration::Preview,String>{
    tauri::async_runtime::spawn_blocking(move || spanreed::client_configuration::preview_opencode_remove(&provider,&alias)).await.map_err(|_| "Configuration worker stopped".to_string())?
}

#[tauri::command]
async fn preview_opencode_update(provider: String, alias: String, model: String) -> Result<spanreed::client_configuration::Preview, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::client_configuration::preview_opencode_change(&provider,&alias,&model,true)).await.map_err(|_| "Configuration worker stopped".to_string())?
}

#[tauri::command]
async fn saved_migrations() -> Result<Vec<spanreed::migration::session::SavedSession>, String> {
    tauri::async_runtime::spawn_blocking(spanreed::migration::session::saved).await.map_err(|_| "Migration worker stopped".to_string())?
}

#[tauri::command]
async fn begin_inactive_device_login(provider: String, alias: String) -> Result<spanreed::account_login::LoginView, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::account_login::begin_inactive(&provider, alias)).await.map_err(|_| "Authorization worker stopped".to_string())?
}

#[tauri::command]
async fn migration_inventory(id: String) -> Result<Vec<spanreed::migration::MigrationCandidate>, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::migration::session::inventory(&id)).await.map_err(|_| "Migration worker stopped".to_string())?
}
#[tauri::command]
async fn propose_migration(id: String, selection: Vec<spanreed::migration::MigrationSelection>) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::migration::session::propose(&id, &selection).and_then(|view| serde_json::to_value(view).map_err(|_| "Invalid migration view".into()))).await.map_err(|_| "Migration worker stopped".to_string())?
}

#[tauri::command]
async fn execute_migration(id: String, revision: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::migration::session::execute(&id, &revision)).await.map_err(|_| "Migration worker stopped".to_string())?
}

#[tauri::command]
async fn pair_migration(origin: String, id: String, invitation: String) -> Result<serde_json::Value,String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::migration::session::pair(&origin,&id,&invitation).and_then(|view|serde_json::to_value(view).map_err(|_|"Invalid migration view".into()))).await.map_err(|_|"Migration worker stopped".to_string())?
}
#[tauri::command]
async fn migration_status(id: String) -> Result<serde_json::Value,String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::migration::session::status(&id).and_then(|view|serde_json::to_value(view).map_err(|_|"Invalid migration view".into()))).await.map_err(|_|"Migration worker stopped".to_string())?
}
#[tauri::command]
async fn cancel_migration(id: String) -> Result<(),String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::migration::session::cancel(&id)).await.map_err(|_|"Migration worker stopped".to_string())?
}

#[tauri::command]
async fn migration_candidates() -> Result<Vec<spanreed::migration::MigrationCandidate>, String> {
    tauri::async_runtime::spawn_blocking(spanreed::migration::candidates).await.map_err(|_| "Migration inventory worker stopped".to_string())?
}

#[tauri::command]
async fn preview_grok_configuration(alias: String) -> Result<spanreed::client_configuration::Preview, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::client_configuration::preview_grok(&alias)).await.map_err(|_| "Configuration worker stopped".to_string())?
}

#[tauri::command]
async fn preview_opencode_configuration(provider: String, alias: String, model: String) -> Result<spanreed::client_configuration::Preview, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::client_configuration::preview_opencode(&provider, &alias, &model)).await.map_err(|_| "Configuration worker stopped".to_string())?
}
#[tauri::command]
async fn apply_client_configuration(id: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::client_configuration::apply_configuration(&id)).await.map_err(|_| "Configuration worker stopped".to_string())?
}

#[tauri::command]
async fn local_proxy_status() -> Result<spanreed::desktop_runtime::Status, String> {
    tauri::async_runtime::spawn_blocking(spanreed::desktop_runtime::status).await.map_err(|_| "Proxy control worker stopped".to_string())?
}
#[tauri::command]
async fn start_local_proxy(bind: String) -> Result<spanreed::desktop_runtime::Status, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::desktop_runtime::start(&bind)).await.map_err(|_| "Proxy control worker stopped".to_string())?
}
#[tauri::command]
async fn stop_local_proxy() -> Result<spanreed::desktop_runtime::Status, String> {
    tauri::async_runtime::spawn_blocking(spanreed::desktop_runtime::stop).await.map_err(|_| "Proxy control worker stopped".to_string())?
}

#[tauri::command]
async fn models(account_id: String) -> Result<spanreed::desktop::ModelCatalog, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::desktop::models(&account_id))
        .await.map_err(|_| "Model discovery worker stopped".to_string())?
}

#[tauri::command]
async fn reauthorize_account(id: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::account_login::reauthorize(&id)
        .and_then(|view| serde_json::to_value(view).map_err(|_| "Invalid authorization view".into())))
        .await.map_err(|_| "Authorization worker stopped".to_string())?
}

#[tauri::command]
async fn add_api_key(provider: String, alias: String, key: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::account_keys::add(&provider, &alias, &key).map(|_| ()))
        .await.map_err(|_| "Account worker stopped".to_string())?
}

#[tauri::command]
async fn replace_api_key(id: String, generation: Option<String>, key: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::account_keys::replace(&id, generation.as_deref(), &key))
        .await.map_err(|_| "Account worker stopped".to_string())?
}

#[tauri::command]
async fn remove_account(id: String, generation: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::accounts::remove_if_current(&id, generation.as_deref()))
        .await.map_err(|_| "Account worker stopped".to_string())?
}

#[tauri::command]
async fn hops() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(|| spanreed::desktop::hops()
        .and_then(|rows| serde_json::to_value(rows).map_err(|_| "Invalid request history".into())))
        .await.map_err(|_| "Request history worker stopped".to_string())?
}


#[tauri::command]
async fn history() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(|| spanreed::desktop::history()
        .and_then(|rows| serde_json::to_value(rows).map_err(|_| "Invalid history".into())))
        .await.map_err(|_| "History worker stopped".to_string())?
}

#[tauri::command]
async fn begin_device_login(provider: String, alias: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::account_login::begin(&provider, alias)
        .and_then(|view| serde_json::to_value(view).map_err(|_| "Invalid authorization view".into())))
        .await.map_err(|_| "Authorization worker stopped".to_string())?
}
#[tauri::command]
async fn poll_device_login(id: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::account_login::poll(&id)
        .and_then(|progress| serde_json::to_value(progress).map_err(|_| "Invalid authorization state".into())))
        .await.map_err(|_| "Authorization worker stopped".to_string())?
}
#[tauri::command]
async fn cancel_device_login(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::account_login::cancel(&id))
        .await.map_err(|_| "Authorization worker stopped".to_string())?
}

#[tauri::command]
async fn routing() -> Result<spanreed::desktop::RoutingSnapshot, String> {
    tauri::async_runtime::spawn_blocking(spanreed::desktop::routing)
        .await.map_err(|_| "Routing worker stopped".to_string())?
}

#[tauri::command]
async fn set_routing(provider: String, on: bool, threshold: f64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::desktop::set_routing(&provider, on, threshold))
        .await.map_err(|_| "Routing worker stopped".to_string())?
}

#[tauri::command]
async fn snapshot(force: bool) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || serde_json::to_value(spanreed::desktop::snapshot(force)).map_err(|_| "Could not encode usage".into()))
        .await.map_err(|_| "Usage worker stopped".to_string())?
}
#[tauri::command]
fn detection() -> Vec<spanreed::desktop::Detection> { spanreed::desktop::detection() }
#[tauri::command]
async fn accounts() -> Result<spanreed::desktop::AccountsView, String> {
    tauri::async_runtime::spawn_blocking(spanreed::desktop::accounts)
        .await.map_err(|_| "Account worker stopped".to_string())?
}
#[tauri::command]
fn privacy() -> fabrials_core::SharingConsent { spanreed::privacy::load() }
#[tauri::command]
fn set_privacy(consent: fabrials_core::SharingConsent) -> Result<(), String> { spanreed::privacy::save(&consent) }
#[tauri::command]
async fn activate_account(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::accounts::set_active(&id).map(|_| ()))
        .await.map_err(|_| "Account worker stopped".to_string())?
}

#[tauri::command]
async fn private_history(before: Option<i64>) -> Result<serde_json::Value,String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::sync::recent(before).and_then(|page|serde_json::to_value(page).map_err(|_|"Could not encode private history".into()))).await.map_err(|_|"History worker stopped".to_string())?
}
#[tauri::command]
async fn sync_settings() -> Result<spanreed::sync::SyncSettings,String> {
    tauri::async_runtime::spawn_blocking(spanreed::sync::settings).await.map_err(|_|"Sync worker stopped".to_string())?
}
#[tauri::command]
async fn save_sync_settings(settings: spanreed::sync::SyncSettings) -> Result<(),String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::sync::save_settings(settings)).await.map_err(|_|"Sync worker stopped".to_string())?
}
#[tauri::command]
fn sync_status() -> spanreed::sync::SyncStatus {spanreed::sync::status()}
#[tauri::command]
async fn link_codex_source()->Result<String,String> {
    tauri::async_runtime::spawn_blocking(spanreed::sync::link_codex).await.map_err(|_|"Identity matching interrupted".to_string())?
}
#[tauri::command]
async fn sync_now() -> Result<String,String> {
    tauri::async_runtime::spawn_blocking(|| spanreed::sync::run(true)).await.map_err(|_|"Sync worker stopped".to_string())?
}
#[tauri::command]
async fn publication_status() -> Result<spanreed::sharing_control::PublicationStatus,String> {
    tauri::async_runtime::spawn_blocking(spanreed::sharing_control::status).await.map_err(|_|"Publication worker stopped".to_string())
}
#[tauri::command]
async fn publish_metrics() -> Result<String,String> {
    tauri::async_runtime::spawn_blocking(spanreed::sharing_control::publish).await.map_err(|_|"Publication worker stopped".to_string())?
}
#[tauri::command]
async fn set_publication_schedule(enabled: bool) -> Result<String,String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::sharing_control::schedule(enabled)).await.map_err(|_|"Publication worker stopped".to_string())?
}

#[tauri::command]
fn remote_open_authorization(url: String) -> Result<(), String> {
    open_url(&spanreed::remote_workspace::verification_url(&url)?)
}

#[tauri::command]
async fn preview_hosted_client(owner:String,client:String,alias:String,key:String,model:String)->Result<spanreed::hosted_client_configuration::HostedClientReview,String> {
    tauri::async_runtime::spawn_blocking(move||spanreed::hosted_client_configuration::preview(&owner,&client,&alias,&key,&model)).await.map_err(|_|"Configuration worker stopped".to_string())?
}
#[tauri::command]
async fn apply_hosted_client(owner:String,id:String)->Result<String,String> {
    tauri::async_runtime::spawn_blocking(move||spanreed::hosted_client_configuration::apply(&owner,&id)).await.map_err(|_|"Configuration worker stopped".to_string())?
}

#[tauri::command]
async fn codex_session_move(owner:String, operation:String, id:Option<String>, alias:Option<String>)->Result<serde_json::Value,String> {
    tauri::async_runtime::spawn_blocking(move || {
        use spanreed::codex_session_move as session;
        let view=match operation.as_str() {
            "current"=>return serde_json::to_value(session::current(&owner)?).map_err(|_|"Invalid session view".into()),
            "preview"=>session::preview(&owner,alias.as_deref().ok_or("Choose an account name")?)?,
            "apply"=>session::apply(&owner,id.as_deref().ok_or("Select a saved session move")?)?,
            "recover"|"cancel"=>session::recover(&owner,id.as_deref().ok_or("Select a saved session move")?,operation=="cancel")?,
            "dismiss"=>{session::dismiss(&owner,id.as_deref().ok_or("Select a saved session move")?)?;return Ok(serde_json::Value::Null);},
            _=>return Err("Unknown session move operation".into()),
        };
        serde_json::to_value(view).map_err(|_|"Invalid session view".into())
    }).await.map_err(|_|"Session move worker stopped; reopen its recovery view".to_string())?
}

#[tauri::command]
async fn remote_request(operation: spanreed::remote_workspace::RemoteOperation, body: serde_json::Value, days: Option<u32>, owner: Option<String>) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::remote_workspace::request(operation, body, days, owner.as_deref()))
        .await.map_err(|_| "Remote workspace worker stopped".to_string())?
}

#[tauri::command]
async fn fabrials_status() -> Result<spanreed::fabrials_login::LinkView, String> {
    tauri::async_runtime::spawn_blocking(spanreed::fabrials_login::status)
        .await.map_err(|_| "Fabrials connection worker stopped".to_string())?
}
#[tauri::command]
async fn fabrials_begin() -> Result<spanreed::fabrials_login::LinkView, String> {
    tauri::async_runtime::spawn_blocking(spanreed::fabrials_login::begin)
        .await.map_err(|_| "Fabrials connection worker stopped".to_string())?
}
#[tauri::command]
async fn fabrials_disconnect() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(spanreed::fabrials_login::disconnect)
        .await.map_err(|_| "Fabrials connection worker stopped".to_string())?
}
#[tauri::command]
async fn fabrials_poll(id: String) -> Result<spanreed::fabrials_login::LinkView, String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::fabrials_login::poll(&id))
        .await.map_err(|_| "Fabrials connection worker stopped".to_string())?
}
#[tauri::command]
async fn fabrials_cancel(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || spanreed::fabrials_login::cancel(&id))
        .await.map_err(|_| "Fabrials connection worker stopped".to_string())?
}
#[tauri::command]
async fn fabrials_open(id: String) -> Result<(), String> {
    let url = tauri::async_runtime::spawn_blocking(move || spanreed::fabrials_login::verification_url(&id))
        .await.map_err(|_| "Fabrials connection worker stopped".to_string())??;
    open_url(&url)
}

#[tauri::command]
fn open_hosted() -> Result<(), String> {
    open_url("https://ai.fabrials.com")
}

#[tauri::command]
fn open_device_login(id: String) -> Result<(), String> {
    open_url(&spanreed::account_login::verification_url(&id)?)
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    result.map(|_| ()).map_err(|_| "Could not open the browser".into())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![preview_hosted_client, apply_hosted_client, codex_session_move, private_history, sync_settings, save_sync_settings, sync_status, sync_now, link_codex_source, publication_status, publish_metrics, set_publication_schedule, remote_open_authorization, remote_request, fabrials_status, fabrials_begin, fabrials_poll, fabrials_cancel, fabrials_disconnect, fabrials_open, forget_migration, begin_migration_authorization, migration_authorizations, preview_opencode_remove, preview_opencode_update, saved_migrations, begin_inactive_device_login, migration_inventory, propose_migration, execute_migration, pair_migration, migration_status, cancel_migration, migration_candidates, preview_grok_configuration, preview_opencode_configuration, apply_client_configuration, replace_api_key, remove_account, local_proxy_status, start_local_proxy, stop_local_proxy, notifications::test_reset_notification, notifications::notification_settings, notifications::set_reset_notifications, notifications::check_reset_notifications, models, reauthorize_account, add_api_key, hops, history, snapshot, detection, accounts, privacy, set_privacy, activate_account, open_hosted, routing, set_routing, begin_device_login, poll_device_login, cancel_device_login, open_device_login])
        .setup(|app| {
            let show = tauri::menu::MenuItem::with_id(app, "show", "Open Spanreed", true, None::<&str>)?;
            let quit = tauri::menu::MenuItem::with_id(app, "quit", "Quit Spanreed", true, None::<&str>)?;
            let menu = tauri::menu::Menu::with_items(app, &[&show, &quit])?;
            tauri::tray::TrayIconBuilder::new().menu(&menu).tooltip("Spanreed")
                .icon(app.default_window_icon().expect("bundled application icon").clone())
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => { if let Some(window) = app.get_webview_window("main") { let _ = window.show(); let _ = window.set_focus(); } }
                    "quit" => app.exit(0), _ => {}
                }).build(app)?;
            Ok(())
        })
        .run(tauri::generate_context!()).expect("could not start Spanreed desktop");
}
