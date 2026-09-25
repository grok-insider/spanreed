//! Host accounts: summaries, API keys, device logins and model discovery.
use crate::context::AppContext;

pub use fabrials_providers::device_flow::Progress;

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct LoginView {
    pub id: String,
    pub alias: String,
    pub provider: String,
    pub device: fabrials_providers::device_flow::DeviceView,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub account_id: String,
    pub observed_at_ms: i64,
    pub models: Vec<String>,
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

/// Port: the account registry and its secrets (keyring / private files).
pub trait AccountStore: Send + Sync {
    fn accounts(&self) -> Result<Vec<AccountSummary>, String>;
    fn add_api_key(&self, provider: &str, alias: &str, key: &str) -> Result<(), String>;
    fn replace_api_key(&self, id: &str, generation: Option<&str>, key: &str) -> Result<(), String>;
    fn remove(&self, id: &str, generation: Option<&str>) -> Result<(), String>;
    fn activate(&self, id: &str) -> Result<(), String>;
    /// `spanreed account …`: output text or an error with usage.
    fn command(&self, args: &[String]) -> Result<String, String>;
    fn link_copilot(&self, args: &[String]) -> Result<(), String>;
    fn unlink_copilot(&self) -> Result<(), String>;
    /// Models the account's upstream lists.
    fn models(&self, account_id: &str) -> Result<Vec<String>, String>;
    fn redeem_codex_reset(&self, request_id: &str) -> Result<(), &'static str>;
    fn valid_reset_request(&self, request_id: &str) -> bool;
}

/// Port: the one device authorization in progress.
pub trait DeviceLogins: Send + Sync {
    fn begin(&self, provider: &str, alias: String) -> Result<LoginView, String>;
    fn begin_inactive(&self, provider: &str, alias: String) -> Result<LoginView, String>;
    fn reauthorize(&self, id: &str) -> Result<LoginView, String>;
    fn poll(&self, id: &str) -> Result<Progress, String>;
    fn cancel(&self, id: &str) -> Result<(), String>;
    fn verification_url(&self, id: &str) -> Result<String, String>;
}

fn store(ctx: &AppContext) -> &dyn AccountStore {
    ctx.services().accounts.as_ref()
}

fn logins(ctx: &AppContext) -> &dyn DeviceLogins {
    ctx.services().logins.as_ref()
}

/// Redeem a Codex limit-reset credit; `request_id` makes retries idempotent.
pub fn redeem_codex_reset(ctx: &AppContext, request_id: &str) -> Result<(), &'static str> {
    store(ctx).redeem_codex_reset(request_id)
}

pub fn valid_reset_request(ctx: &AppContext, request_id: &str) -> bool {
    store(ctx).valid_reset_request(request_id)
}

/// `spanreed auth copilot [--user LOGIN | --token-stdin]`.
pub fn link_copilot(ctx: &AppContext, args: &[String]) -> Result<(), String> {
    store(ctx).link_copilot(args)
}

pub fn unlink_copilot(ctx: &AppContext) -> Result<(), String> {
    store(ctx).unlink_copilot()
}

/// `spanreed account …` (and the `spanreed grok …` addon shim).
pub fn command(ctx: &AppContext, args: &[String]) -> Result<String, String> {
    store(ctx).command(args)
}

pub fn add_api_key(ctx: &AppContext, provider: &str, alias: &str, key: &str) -> Result<(), String> {
    store(ctx).add_api_key(provider, alias, key)
}

pub fn replace_api_key(
    ctx: &AppContext,
    id: &str,
    generation: Option<&str>,
    key: &str,
) -> Result<(), String> {
    store(ctx).replace_api_key(id, generation, key)
}

pub fn remove(ctx: &AppContext, id: &str, generation: Option<&str>) -> Result<(), String> {
    store(ctx).remove(id, generation)
}

pub fn activate(ctx: &AppContext, id: &str) -> Result<(), String> {
    store(ctx).activate(id)
}

pub fn begin_login(ctx: &AppContext, provider: &str, alias: String) -> Result<LoginView, String> {
    logins(ctx).begin(provider, alias)
}

pub fn begin_inactive_login(
    ctx: &AppContext,
    provider: &str,
    alias: String,
) -> Result<LoginView, String> {
    logins(ctx).begin_inactive(provider, alias)
}

pub fn reauthorize(ctx: &AppContext, id: &str) -> Result<LoginView, String> {
    logins(ctx).reauthorize(id)
}

pub fn poll_login(ctx: &AppContext, id: &str) -> Result<Progress, String> {
    logins(ctx).poll(id)
}

pub fn cancel_login(ctx: &AppContext, id: &str) -> Result<(), String> {
    logins(ctx).cancel(id)
}

pub fn login_url(ctx: &AppContext, id: &str) -> Result<String, String> {
    logins(ctx).verification_url(id)
}

pub fn models(ctx: &AppContext, account_id: &str) -> Result<ModelCatalog, String> {
    let models = store(ctx).models(account_id)?;
    Ok(ModelCatalog {
        account_id: account_id.into(),
        observed_at_ms: crate::util::now_ms(),
        models,
    })
}

pub fn accounts(ctx: &AppContext) -> Result<AccountsView, String> {
    Ok(AccountsView {
        accounts: store(ctx).accounts()?,
    })
}
