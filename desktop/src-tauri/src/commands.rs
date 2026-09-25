//! Tauri commands. Each one only adapts arguments and results; the work is
//! done by `spanreed::app`, off the renderer thread.
use serde_json::Value;
use spanreed::app::{self, AppContext};
use tauri::Manager;

pub type Ctx<'a> = tauri::State<'a, AppContext>;

/// Run blocking application work on the blocking pool.
pub async fn blocking<T: Send + 'static>(
    stopped: &'static str,
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|_| stopped.to_string())?
}

fn json<T: serde::Serialize>(value: T, invalid: &str) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|_| invalid.to_string())
}

const MIGRATION: &str = "Migration worker stopped";
const AUTHORIZATION: &str = "Authorization worker stopped";
const CONFIGURATION: &str = "Configuration worker stopped";
const PROXY: &str = "Proxy control worker stopped";
const ACCOUNT: &str = "Account worker stopped";
const USAGE: &str = "Usage worker stopped";
const SYNC: &str = "Sync worker stopped";
const PUBLICATION: &str = "Publication worker stopped";
const FABRIALS: &str = "Fabrials connection worker stopped";
const ROUTING: &str = "Routing worker stopped";

#[tauri::command]
pub async fn forget_migration(id: String) -> Result<(), String> {
    blocking(MIGRATION, move || app::migration::forget(&id)).await
}

#[tauri::command]
pub async fn begin_migration_authorization(
    ctx: Ctx<'_>,
    id: String,
    source_id: String,
) -> Result<app::accounts::LoginView, String> {
    let ctx = ctx.inner().clone();
    blocking(AUTHORIZATION, move || {
        app::migration::begin_authorization(&ctx, &id, &source_id)
    })
    .await
}

#[tauri::command]
pub async fn migration_authorizations(id: String) -> Result<Vec<String>, String> {
    blocking(MIGRATION, move || app::migration::authorizations(&id)).await
}

#[tauri::command]
pub async fn saved_migrations() -> Result<Vec<app::migration::SavedSession>, String> {
    blocking(MIGRATION, app::migration::saved).await
}

#[tauri::command]
pub async fn migration_inventory(
    id: String,
) -> Result<Vec<app::migration::MigrationCandidate>, String> {
    blocking(MIGRATION, move || app::migration::inventory(&id)).await
}

#[tauri::command]
pub async fn propose_migration(
    id: String,
    selection: Vec<app::migration::MigrationSelection>,
) -> Result<Value, String> {
    blocking(MIGRATION, move || {
        json(
            app::migration::propose(&id, &selection)?,
            "Invalid migration view",
        )
    })
    .await
}

#[tauri::command]
pub async fn execute_migration(id: String, revision: String) -> Result<Vec<String>, String> {
    blocking(MIGRATION, move || app::migration::execute(&id, &revision)).await
}

#[tauri::command]
pub async fn pair_migration(
    origin: String,
    id: String,
    invitation: String,
) -> Result<Value, String> {
    blocking(MIGRATION, move || {
        json(
            app::migration::pair(&origin, &id, &invitation)?,
            "Invalid migration view",
        )
    })
    .await
}

#[tauri::command]
pub async fn migration_status(id: String) -> Result<Value, String> {
    blocking(MIGRATION, move || {
        json(app::migration::status(&id)?, "Invalid migration view")
    })
    .await
}

#[tauri::command]
pub async fn cancel_migration(id: String) -> Result<(), String> {
    blocking(MIGRATION, move || app::migration::cancel(&id)).await
}

#[tauri::command]
pub async fn migration_candidates() -> Result<Vec<app::migration::MigrationCandidate>, String> {
    blocking(
        "Migration inventory worker stopped",
        app::migration::candidates,
    )
    .await
}

#[tauri::command]
pub async fn preview_opencode_remove(
    ctx: Ctx<'_>,
    provider: String,
    alias: String,
) -> Result<app::clients::Preview, String> {
    let ctx = ctx.inner().clone();
    blocking(CONFIGURATION, move || {
        app::clients::preview_opencode_remove(&ctx, &provider, &alias)
    })
    .await
}

#[tauri::command]
pub async fn preview_opencode_update(
    ctx: Ctx<'_>,
    provider: String,
    alias: String,
    model: String,
) -> Result<app::clients::Preview, String> {
    let ctx = ctx.inner().clone();
    blocking(CONFIGURATION, move || {
        app::clients::preview_opencode_update(&ctx, &provider, &alias, &model)
    })
    .await
}

#[tauri::command]
pub async fn preview_grok_configuration(
    ctx: Ctx<'_>,
    alias: String,
    model: Option<String>,
) -> Result<app::clients::Preview, String> {
    let model = model.filter(|value| !value.is_empty());
    let ctx = ctx.inner().clone();
    blocking(CONFIGURATION, move || {
        app::clients::preview_grok(&ctx, &alias, model.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn preview_opencode_configuration(
    ctx: Ctx<'_>,
    provider: String,
    alias: String,
    model: String,
) -> Result<app::clients::Preview, String> {
    let ctx = ctx.inner().clone();
    blocking(CONFIGURATION, move || {
        app::clients::preview_opencode(&ctx, &provider, &alias, &model)
    })
    .await
}

#[tauri::command]
pub async fn apply_client_configuration(
    ctx: Ctx<'_>,
    id: String,
) -> Result<Option<String>, String> {
    let ctx = ctx.inner().clone();
    blocking(CONFIGURATION, move || app::clients::apply(&ctx, &id)).await
}

#[tauri::command]
pub async fn preview_hosted_client(
    ctx: Ctx<'_>,
    owner: String,
    client: String,
    alias: String,
    key: String,
    model: String,
) -> Result<app::clients::HostedClientReview, String> {
    let ctx = ctx.inner().clone();
    blocking(CONFIGURATION, move || {
        app::clients::preview_hosted(
            &ctx,
            app::clients::HostedRequest {
                owner: &owner,
                client: &client,
                alias: &alias,
                key: &key,
                model: &model,
            },
        )
    })
    .await
}

#[tauri::command]
pub async fn apply_hosted_client(
    ctx: Ctx<'_>,
    owner: String,
    id: String,
) -> Result<String, String> {
    let ctx = ctx.inner().clone();
    blocking(CONFIGURATION, move || {
        app::clients::apply_hosted(&ctx, &owner, &id)
    })
    .await
}

#[tauri::command]
pub async fn codex_session_move(
    owner: String,
    operation: String,
    id: Option<String>,
    alias: Option<String>,
) -> Result<Value, String> {
    blocking(
        "Session move worker stopped; reopen its recovery view",
        move || {
            let view =
                app::clients::session_move(&owner, &operation, id.as_deref(), alias.as_deref())?;
            json(view, "Invalid session view")
        },
    )
    .await
}

#[tauri::command]
pub async fn local_proxy_status(ctx: Ctx<'_>) -> Result<app::proxy::Status, String> {
    let ctx = ctx.inner().clone();
    blocking(PROXY, move || app::proxy::status(&ctx)).await
}

#[tauri::command]
pub async fn start_local_proxy(ctx: Ctx<'_>, bind: String) -> Result<app::proxy::Status, String> {
    let ctx = ctx.inner().clone();
    blocking(PROXY, move || app::proxy::start(&ctx, &bind)).await
}

#[tauri::command]
pub async fn stop_local_proxy(ctx: Ctx<'_>) -> Result<app::proxy::Status, String> {
    let ctx = ctx.inner().clone();
    blocking(PROXY, move || app::proxy::stop(&ctx)).await
}

#[tauri::command]
pub async fn models(account_id: String) -> Result<app::accounts::ModelCatalog, String> {
    blocking("Model discovery worker stopped", move || {
        app::accounts::models(&account_id)
    })
    .await
}

#[tauri::command]
pub async fn reauthorize_account(ctx: Ctx<'_>, id: String) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking(AUTHORIZATION, move || {
        json(
            app::accounts::reauthorize(&ctx, &id)?,
            "Invalid authorization view",
        )
    })
    .await
}

#[tauri::command]
pub async fn add_api_key(provider: String, alias: String, key: String) -> Result<(), String> {
    blocking(ACCOUNT, move || {
        app::accounts::add_api_key(&provider, &alias, &key)
    })
    .await
}

#[tauri::command]
pub async fn replace_api_key(
    id: String,
    generation: Option<String>,
    key: String,
) -> Result<(), String> {
    blocking(ACCOUNT, move || {
        app::accounts::replace_api_key(&id, generation.as_deref(), &key)
    })
    .await
}

#[tauri::command]
pub async fn remove_account(id: String, generation: Option<String>) -> Result<(), String> {
    blocking(ACCOUNT, move || {
        app::accounts::remove(&id, generation.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn activate_account(id: String) -> Result<(), String> {
    blocking(ACCOUNT, move || app::accounts::activate(&id)).await
}

#[tauri::command]
pub async fn accounts() -> Result<app::accounts::AccountsView, String> {
    blocking(ACCOUNT, app::accounts::accounts).await
}

#[tauri::command]
pub async fn begin_device_login(
    ctx: Ctx<'_>,
    provider: String,
    alias: String,
) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking(AUTHORIZATION, move || {
        json(
            app::accounts::begin_login(&ctx, &provider, alias)?,
            "Invalid authorization view",
        )
    })
    .await
}

#[tauri::command]
pub async fn begin_inactive_device_login(
    ctx: Ctx<'_>,
    provider: String,
    alias: String,
) -> Result<app::accounts::LoginView, String> {
    let ctx = ctx.inner().clone();
    blocking(AUTHORIZATION, move || {
        app::accounts::begin_inactive_login(&ctx, &provider, alias)
    })
    .await
}

#[tauri::command]
pub async fn poll_device_login(ctx: Ctx<'_>, id: String) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking(AUTHORIZATION, move || {
        json(
            app::accounts::poll_login(&ctx, &id)?,
            "Invalid authorization state",
        )
    })
    .await
}

#[tauri::command]
pub async fn cancel_device_login(ctx: Ctx<'_>, id: String) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(AUTHORIZATION, move || {
        app::accounts::cancel_login(&ctx, &id)
    })
    .await
}

#[tauri::command]
pub fn open_device_login(ctx: Ctx<'_>, id: String) -> Result<(), String> {
    open_url(&app::accounts::login_url(&ctx, &id)?)
}

#[tauri::command]
pub async fn hops() -> Result<Value, String> {
    blocking("Request history worker stopped", || {
        json(app::usage::hops()?, "Invalid request history")
    })
    .await
}

#[tauri::command]
pub async fn history() -> Result<Value, String> {
    blocking("History worker stopped", || {
        json(app::usage::history()?, "Invalid history")
    })
    .await
}

#[tauri::command]
pub async fn routing() -> Result<app::routing::RoutingSnapshot, String> {
    blocking(ROUTING, app::routing::routing).await
}

#[tauri::command]
pub async fn set_routing(provider: String, on: bool, threshold: f64) -> Result<(), String> {
    blocking(ROUTING, move || {
        app::routing::set_routing(&provider, on, threshold)
    })
    .await
}

#[tauri::command]
pub async fn disconnect_usage_source(client: String) -> Result<(), String> {
    blocking(USAGE, move || app::usage::connections::disconnect(&client)).await
}

#[tauri::command]
pub async fn connect_usage_source(
    connection: app::usage::connections::Connection,
) -> Result<(), String> {
    blocking(USAGE, move || app::usage::connections::save(connection)).await
}

#[tauri::command]
pub async fn usage_report(
    ctx: Ctx<'_>,
    filter: fabrials_types::consumption::UsageFilter,
    force: bool,
) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking(USAGE, move || {
        serde_json::to_value(app::usage::report(&ctx, filter, force)?).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn usage_sources() -> Result<Value, String> {
    blocking(USAGE, app::usage::sources).await
}

#[tauri::command]
pub async fn save_usage_sources(
    settings: app::usage::discovery::UsageSettings,
) -> Result<(), String> {
    blocking(USAGE, move || app::usage::discovery::save(&settings)).await
}

#[tauri::command]
pub async fn snapshot(ctx: Ctx<'_>, force: bool) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking(USAGE, move || {
        json(app::usage::snapshot(&ctx, force), "Could not encode usage")
    })
    .await
}

#[tauri::command]
pub fn detection() -> Vec<app::usage::Detection> {
    app::usage::detection()
}

#[tauri::command]
pub fn privacy() -> app::sharing::SharingConsent {
    app::sharing::consent()
}

#[tauri::command]
pub fn set_privacy(consent: app::sharing::SharingConsent) -> Result<(), String> {
    app::sharing::save_consent(&consent)
}

#[tauri::command]
pub async fn private_history(before: Option<i64>) -> Result<Value, String> {
    blocking("History worker stopped", move || {
        json(
            app::sync::recent(before)?,
            "Could not encode private history",
        )
    })
    .await
}

#[tauri::command]
pub async fn sync_settings() -> Result<app::sync::SyncSettings, String> {
    blocking(SYNC, app::sync::settings).await
}

#[tauri::command]
pub async fn save_sync_settings(settings: app::sync::SyncSettings) -> Result<(), String> {
    blocking(SYNC, move || app::sync::save_settings(settings)).await
}

#[tauri::command]
pub fn sync_status() -> app::sync::SyncStatus {
    app::sync::status()
}

#[tauri::command]
pub async fn link_codex_source() -> Result<String, String> {
    blocking("Identity matching interrupted", app::sync::link_codex).await
}

#[tauri::command]
pub async fn sync_now(ctx: Ctx<'_>) -> Result<String, String> {
    let ctx = ctx.inner().clone();
    blocking(SYNC, move || app::sync::run(&ctx)).await
}

#[tauri::command]
pub async fn publication_status() -> Result<app::sharing::PublicationStatus, String> {
    blocking(PUBLICATION, || Ok(app::sharing::status())).await
}

#[tauri::command]
pub async fn publish_metrics(ctx: Ctx<'_>) -> Result<String, String> {
    let ctx = ctx.inner().clone();
    blocking(PUBLICATION, move || app::sharing::publish(&ctx)).await
}

#[tauri::command]
pub async fn set_publication_schedule(enabled: bool) -> Result<String, String> {
    blocking(PUBLICATION, move || app::sharing::schedule(enabled)).await
}

#[tauri::command]
pub fn remote_open_authorization(url: String) -> Result<(), String> {
    open_url(&app::fabrials::authorization_url(&url)?)
}

#[tauri::command]
pub async fn remote_request(
    operation: app::fabrials::RemoteOperation,
    body: Value,
    days: Option<u32>,
    owner: Option<String>,
) -> Result<Value, String> {
    blocking("Remote workspace worker stopped", move || {
        app::fabrials::remote(operation, body, days, owner.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn fabrials_status() -> Result<app::fabrials::LinkView, String> {
    blocking(FABRIALS, app::fabrials::status).await
}

#[tauri::command]
pub async fn fabrials_begin() -> Result<app::fabrials::LinkView, String> {
    blocking(FABRIALS, app::fabrials::begin).await
}

#[tauri::command]
pub async fn fabrials_disconnect() -> Result<(), String> {
    blocking(FABRIALS, app::fabrials::disconnect).await
}

#[tauri::command]
pub async fn fabrials_poll(id: String) -> Result<app::fabrials::LinkView, String> {
    blocking(FABRIALS, move || app::fabrials::poll(&id)).await
}

#[tauri::command]
pub async fn fabrials_cancel(id: String) -> Result<(), String> {
    blocking(FABRIALS, move || app::fabrials::cancel(&id)).await
}

#[tauri::command]
pub async fn fabrials_open(id: String) -> Result<(), String> {
    let url = blocking(FABRIALS, move || app::fabrials::verification_url(&id)).await?;
    open_url(&url)
}

#[tauri::command]
pub fn open_hosted() -> Result<(), String> {
    open_url("https://ai.fabrials.com")
}

#[tauri::command]
pub fn take_desktop_route(handle: tauri::AppHandle) -> Option<String> {
    let href = app::window::take()?;
    if let Some(window) = handle.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    Some(href)
}

pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    result
        .map(|_| ())
        .map_err(|_| "Could not open the browser".into())
}
