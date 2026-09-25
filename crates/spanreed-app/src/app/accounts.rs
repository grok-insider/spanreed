//! Host accounts: summaries, API keys, device logins and model discovery.
use crate::context::AppContext;

pub use crate::account_login::LoginView;
pub use fabrials_providers::device_flow::Progress;

/// Redeem a Codex limit-reset credit; `request_id` makes retries idempotent.
pub fn redeem_codex_reset(request_id: &str) -> Result<(), &'static str> {
    crate::providers::codex::redeem_reset(request_id)
}

pub fn valid_reset_request(request_id: &str) -> bool {
    crate::providers::codex::valid_redeem_request_id(request_id)
}

/// `spanreed auth copilot [--user LOGIN | --token-stdin]`.
pub fn link_copilot(args: &[String]) -> Result<(), String> {
    crate::providers::copilot::cmd_auth(args)
}

pub fn unlink_copilot() -> Result<(), String> {
    crate::providers::copilot::cmd_logout()
}

/// `spanreed account …` (and the `spanreed grok …` addon shim).
pub fn command(args: &[String]) -> Result<String, String> {
    crate::drivers::dispatch_account(args)
}

pub fn add_api_key(provider: &str, alias: &str, key: &str) -> Result<(), String> {
    crate::account_keys::add(provider, alias, key).map(|_| ())
}

pub fn replace_api_key(id: &str, generation: Option<&str>, key: &str) -> Result<(), String> {
    crate::account_keys::replace(id, generation, key)
}

pub fn remove(id: &str, generation: Option<&str>) -> Result<(), String> {
    crate::accounts::remove_if_current(id, generation)
}

pub fn activate(id: &str) -> Result<(), String> {
    crate::accounts::set_active(id).map(|_| ())
}

pub fn begin_login(ctx: &AppContext, provider: &str, alias: String) -> Result<LoginView, String> {
    ctx.logins().begin(provider, alias)
}

pub fn begin_inactive_login(
    ctx: &AppContext,
    provider: &str,
    alias: String,
) -> Result<LoginView, String> {
    ctx.logins().begin_inactive(provider, alias)
}

pub fn reauthorize(ctx: &AppContext, id: &str) -> Result<LoginView, String> {
    ctx.logins().reauthorize(id)
}

pub fn poll_login(ctx: &AppContext, id: &str) -> Result<Progress, String> {
    ctx.logins().poll(id)
}

pub fn cancel_login(ctx: &AppContext, id: &str) -> Result<(), String> {
    ctx.logins().cancel(id)
}

pub fn login_url(ctx: &AppContext, id: &str) -> Result<String, String> {
    ctx.logins().verification_url(id)
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub account_id: String,
    pub observed_at_ms: i64,
    pub models: Vec<String>,
}
pub fn models(account_id: &str) -> Result<ModelCatalog, String> {
    let models = crate::local_relay::models_for_account(account_id)?;
    Ok(ModelCatalog {
        account_id: account_id.into(),
        observed_at_ms: crate::util::now_ms(),
        models,
    })
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct AccountSummary {
    pub id: String,
    pub provider: String,
    pub alias: String,
    pub generation: Option<String>,
    pub active: bool,
    pub plan_label: Option<String>,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct AccountsView {
    pub accounts: Vec<AccountSummary>,
}
pub fn accounts() -> Result<AccountsView, String> {
    Ok(AccountsView {
        accounts: crate::accounts::routing_registry()?
            .accounts
            .into_iter()
            .map(|account| AccountSummary {
                id: account.id,
                provider: account.provider,
                alias: account.alias,
                generation: account.generation,
                active: account.active,
                plan_label: account.plan_label,
            })
            .collect(),
    })
}
