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
pub async fn forget_migration(ctx: Ctx<'_>, id: String) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(MIGRATION, move || app::migration::forget(&ctx, &id)).await
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
pub async fn migration_authorizations(ctx: Ctx<'_>, id: String) -> Result<Vec<String>, String> {
    let ctx = ctx.inner().clone();
    blocking(MIGRATION, move || app::migration::authorizations(&ctx, &id)).await
}

#[tauri::command]
pub async fn saved_migrations(ctx: Ctx<'_>) -> Result<Vec<app::migration::SavedSession>, String> {
    let ctx = ctx.inner().clone();
    blocking(MIGRATION, move || app::migration::saved(&ctx)).await
}

#[tauri::command]
pub async fn migration_inventory(
    ctx: Ctx<'_>,
    id: String,
) -> Result<Vec<app::migration::MigrationCandidate>, String> {
    let ctx = ctx.inner().clone();
    blocking(MIGRATION, move || app::migration::inventory(&ctx, &id)).await
}

#[tauri::command]
pub async fn propose_migration(
    ctx: Ctx<'_>,
    id: String,
    selection: Vec<app::migration::MigrationSelection>,
) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking(MIGRATION, move || {
        json(
            app::migration::propose(&ctx, &id, &selection)?,
            "Invalid migration view",
        )
    })
    .await
}

#[tauri::command]
pub async fn execute_migration(
    ctx: Ctx<'_>,
    id: String,
    revision: String,
) -> Result<Vec<String>, String> {
    let ctx = ctx.inner().clone();
    blocking(MIGRATION, move || {
        app::migration::execute(&ctx, &id, &revision)
    })
    .await
}

#[tauri::command]
pub async fn pair_migration(
    ctx: Ctx<'_>,
    origin: String,
    id: String,
    invitation: String,
) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking(MIGRATION, move || {
        json(
            app::migration::pair(&ctx, &origin, &id, &invitation)?,
            "Invalid migration view",
        )
    })
    .await
}

#[tauri::command]
pub async fn migration_status(ctx: Ctx<'_>, id: String) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking(MIGRATION, move || {
        json(app::migration::status(&ctx, &id)?, "Invalid migration view")
    })
    .await
}

#[tauri::command]
pub async fn cancel_migration(ctx: Ctx<'_>, id: String) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(MIGRATION, move || app::migration::cancel(&ctx, &id)).await
}

#[tauri::command]
pub async fn migration_candidates(
    ctx: Ctx<'_>,
) -> Result<Vec<app::migration::MigrationCandidate>, String> {
    let ctx = ctx.inner().clone();
    blocking("Migration inventory worker stopped", move || {
        app::migration::candidates(&ctx)
    })
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
    ctx: Ctx<'_>,
    owner: String,
    operation: String,
    id: Option<String>,
    alias: Option<String>,
) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking(
        "Session move worker stopped; reopen its recovery view",
        move || {
            let view = app::clients::session_move(
                &ctx,
                &owner,
                &operation,
                id.as_deref(),
                alias.as_deref(),
            )?;
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

const AGENT: &str = "Agent host worker stopped";

#[tauri::command]
pub async fn agent_status(ctx: Ctx<'_>) -> Result<app::agent::AgentStatus, String> {
    let ctx = ctx.inner().clone();
    blocking(AGENT, move || app::agent::status(&ctx)).await
}

/// Run `spanreed agent serve` inside this process on the agent host's own port.
#[tauri::command]
pub async fn agent_start(ctx: Ctx<'_>) -> Result<app::agent::AgentStatus, String> {
    let ctx = ctx.inner().clone();
    blocking(AGENT, move || app::agent::start(&ctx)).await
}

#[tauri::command]
pub async fn agent_stop(ctx: Ctx<'_>) -> Result<app::agent::AgentStatus, String> {
    let ctx = ctx.inner().clone();
    blocking(AGENT, move || app::agent::stop(&ctx)).await
}

#[tauri::command]
pub async fn models(
    ctx: Ctx<'_>,
    account_id: String,
) -> Result<app::accounts::ModelCatalog, String> {
    let ctx = ctx.inner().clone();
    blocking("Model discovery worker stopped", move || {
        app::accounts::models(&ctx, &account_id)
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
pub async fn add_api_key(
    ctx: Ctx<'_>,
    provider: String,
    alias: String,
    key: String,
) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(ACCOUNT, move || {
        app::accounts::add_api_key(&ctx, &provider, &alias, &key)
    })
    .await
}

#[tauri::command]
pub async fn replace_api_key(
    ctx: Ctx<'_>,
    id: String,
    generation: Option<String>,
    key: String,
) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(ACCOUNT, move || {
        app::accounts::replace_api_key(&ctx, &id, generation.as_deref(), &key)
    })
    .await
}

#[tauri::command]
pub async fn remove_account(
    ctx: Ctx<'_>,
    id: String,
    generation: Option<String>,
) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(ACCOUNT, move || {
        app::accounts::remove(&ctx, &id, generation.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn activate_account(ctx: Ctx<'_>, id: String) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(ACCOUNT, move || app::accounts::activate(&ctx, &id)).await
}

#[tauri::command]
pub async fn accounts(ctx: Ctx<'_>) -> Result<app::accounts::AccountsView, String> {
    let ctx = ctx.inner().clone();
    blocking(ACCOUNT, move || app::accounts::accounts(&ctx)).await
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
    let ctx = ctx.inner();
    open_url(&app::accounts::login_url(ctx, &id)?)
}

#[tauri::command]
pub async fn hops(ctx: Ctx<'_>) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking("Request history worker stopped", move || {
        json(app::usage::hops(&ctx)?, "Invalid request history")
    })
    .await
}

#[tauri::command]
pub async fn history(ctx: Ctx<'_>) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking("History worker stopped", move || {
        json(app::usage::history(&ctx)?, "Invalid history")
    })
    .await
}

#[tauri::command]
pub async fn routing(ctx: Ctx<'_>) -> Result<app::routing::RoutingSnapshot, String> {
    let ctx = ctx.inner().clone();
    blocking(ROUTING, move || app::routing::routing(&ctx)).await
}

#[tauri::command]
pub async fn set_routing(
    ctx: Ctx<'_>,
    provider: String,
    on: bool,
    threshold: f64,
) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(ROUTING, move || {
        app::routing::set_routing(&ctx, &provider, on, threshold)
    })
    .await
}

#[tauri::command]
pub async fn disconnect_usage_source(ctx: Ctx<'_>, client: String) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(USAGE, move || app::usage::disconnect(&ctx, &client)).await
}

#[tauri::command]
pub async fn connect_usage_source(
    ctx: Ctx<'_>,
    connection: app::usage::Connection,
) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(USAGE, move || app::usage::save_connection(&ctx, connection)).await
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
pub async fn usage_sources(ctx: Ctx<'_>) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking(USAGE, move || app::usage::sources(&ctx)).await
}

#[tauri::command]
pub async fn save_usage_sources(
    ctx: Ctx<'_>,
    settings: app::usage::UsageSettings,
) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(USAGE, move || app::usage::save_discovery(&ctx, &settings)).await
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
pub fn detection(ctx: Ctx<'_>) -> Vec<app::usage::Detection> {
    let ctx = ctx.inner();
    app::usage::detection(ctx)
}

#[tauri::command]
pub fn privacy(ctx: Ctx<'_>) -> app::sharing::SharingConsent {
    let ctx = ctx.inner();
    app::sharing::consent(ctx)
}

#[tauri::command]
pub fn set_privacy(ctx: Ctx<'_>, consent: app::sharing::SharingConsent) -> Result<(), String> {
    let ctx = ctx.inner();
    app::sharing::save_consent(ctx, &consent)
}

#[tauri::command]
pub async fn private_history(ctx: Ctx<'_>, before: Option<i64>) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking("History worker stopped", move || {
        json(
            app::sync::recent(&ctx, before)?,
            "Could not encode private history",
        )
    })
    .await
}

#[tauri::command]
pub async fn sync_settings(ctx: Ctx<'_>) -> Result<app::sync::SyncSettings, String> {
    let ctx = ctx.inner().clone();
    blocking(SYNC, move || app::sync::settings(&ctx)).await
}

#[tauri::command]
pub async fn save_sync_settings(
    ctx: Ctx<'_>,
    settings: app::sync::SyncSettings,
) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(SYNC, move || app::sync::save_settings(&ctx, settings)).await
}

#[tauri::command]
pub fn sync_status(ctx: Ctx<'_>) -> app::sync::SyncStatus {
    let ctx = ctx.inner();
    app::sync::status(ctx)
}

#[tauri::command]
pub async fn link_codex_source(ctx: Ctx<'_>) -> Result<String, String> {
    let ctx = ctx.inner().clone();
    blocking("Identity matching interrupted", move || {
        app::sync::link_codex(&ctx)
    })
    .await
}

#[tauri::command]
pub async fn sync_now(ctx: Ctx<'_>) -> Result<String, String> {
    let ctx = ctx.inner().clone();
    blocking(SYNC, move || app::sync::run(&ctx)).await
}

#[tauri::command]
pub async fn publication_status(ctx: Ctx<'_>) -> Result<app::sharing::PublicationStatus, String> {
    let ctx = ctx.inner().clone();
    blocking(PUBLICATION, move || Ok(app::sharing::status(&ctx))).await
}

#[tauri::command]
pub async fn publish_metrics(ctx: Ctx<'_>) -> Result<String, String> {
    let ctx = ctx.inner().clone();
    blocking(PUBLICATION, move || app::sharing::publish(&ctx)).await
}

#[tauri::command]
pub async fn set_publication_schedule(ctx: Ctx<'_>, enabled: bool) -> Result<String, String> {
    let ctx = ctx.inner().clone();
    blocking(PUBLICATION, move || app::sharing::schedule(&ctx, enabled)).await
}

#[tauri::command]
pub fn remote_open_authorization(ctx: Ctx<'_>, url: String) -> Result<(), String> {
    let ctx = ctx.inner();
    open_url(&app::fabrials::authorization_url(ctx, &url)?)
}

#[tauri::command]
pub async fn remote_request(
    ctx: Ctx<'_>,
    operation: app::fabrials::RemoteOperation,
    body: Value,
    days: Option<u32>,
    owner: Option<String>,
) -> Result<Value, String> {
    let ctx = ctx.inner().clone();
    blocking("Remote workspace worker stopped", move || {
        app::fabrials::remote(&ctx, operation, body, days, owner.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn fabrials_status(ctx: Ctx<'_>) -> Result<app::fabrials::LinkView, String> {
    let ctx = ctx.inner().clone();
    blocking(FABRIALS, move || app::fabrials::status(&ctx)).await
}

#[tauri::command]
pub async fn fabrials_begin(ctx: Ctx<'_>) -> Result<app::fabrials::LinkView, String> {
    let ctx = ctx.inner().clone();
    blocking(FABRIALS, move || app::fabrials::begin(&ctx)).await
}

#[tauri::command]
pub async fn fabrials_disconnect(ctx: Ctx<'_>) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(FABRIALS, move || app::fabrials::disconnect(&ctx)).await
}

#[tauri::command]
pub async fn fabrials_poll(ctx: Ctx<'_>, id: String) -> Result<app::fabrials::LinkView, String> {
    let ctx = ctx.inner().clone();
    blocking(FABRIALS, move || app::fabrials::poll(&ctx, &id)).await
}

#[tauri::command]
pub async fn fabrials_cancel(ctx: Ctx<'_>, id: String) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    blocking(FABRIALS, move || app::fabrials::cancel(&ctx, &id)).await
}

#[tauri::command]
pub async fn fabrials_open(ctx: Ctx<'_>, id: String) -> Result<(), String> {
    let ctx = ctx.inner().clone();
    let url = blocking(FABRIALS, move || app::fabrials::verification_url(&ctx, &id)).await?;
    open_url(&url)
}

#[tauri::command]
pub fn open_hosted() -> Result<(), String> {
    open_url("https://ai.fabrials.com")
}

#[tauri::command]
pub fn take_desktop_route(ctx: Ctx<'_>, handle: tauri::AppHandle) -> Option<String> {
    let ctx = ctx.inner();
    let href = app::window::take(ctx)?;
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
