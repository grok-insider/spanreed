//! Reviewed client configuration: local proxy wiring, hosted relay keys and
//! Codex session moves.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::AppContext;

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub warnings: Vec<String>,
    pub id: String,
    pub path: String,
    pub provider_id: String,
    #[cfg_attr(feature = "contracts", ts(type = "unknown"))]
    pub addition: Value,
    pub client: ConfigurationClient,
    pub operation: Operation,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigurationClient {
    Opencode,
    Grok,
}

impl ConfigurationClient {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opencode => "opencode",
            Self::Grok => "grok",
        }
    }
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Create,
    Update,
    Remove,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Serialize, Deserialize, Clone)]
pub struct SessionMoveView {
    pub id: String,
    pub state: String,
    pub alias: String,
    pub source_path: String,
    pub config_path: String,
    pub endpoint: String,
    pub account_id: Option<String>,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Serialize, Clone)]
pub struct HostedClientReview {
    pub id: String,
    pub client: String,
    pub path: String,
    pub account_id: String,
    pub model: String,
    pub endpoint: String,
}

/// A hosted relay key for a local client (`codex` or `opencode`).
pub struct HostedRequest<'a> {
    pub owner: &'a str,
    pub client: &'a str,
    pub alias: &'a str,
    pub key: &'a str,
    pub model: &'a str,
}

/// Port: reviewed writes of client configuration files (Grok Build,
/// OpenCode, Codex) and Codex session moves.
pub trait ClientConfigurator: Send + Sync {
    fn preview_grok(&self, alias: &str, model: Option<&str>) -> Result<Preview, String>;
    fn preview_opencode(&self, provider: &str, alias: &str, model: &str)
    -> Result<Preview, String>;
    fn preview_opencode_update(
        &self,
        provider: &str,
        alias: &str,
        model: &str,
    ) -> Result<Preview, String>;
    fn preview_opencode_remove(&self, provider: &str, alias: &str) -> Result<Preview, String>;
    fn apply(&self, id: &str) -> Result<Option<String>, String>;
    fn preview_hosted(&self, request: HostedRequest<'_>) -> Result<HostedClientReview, String>;
    fn apply_hosted(&self, owner: &str, id: &str) -> Result<String, String>;
    fn session_current(&self, owner: &str) -> Result<Option<SessionMoveView>, String>;
    fn session_preview(&self, owner: &str, alias: &str) -> Result<SessionMoveView, String>;
    fn session_apply(&self, owner: &str, id: &str) -> Result<SessionMoveView, String>;
    fn session_recover(
        &self,
        owner: &str,
        id: &str,
        cancel: bool,
    ) -> Result<SessionMoveView, String>;
    fn session_dismiss(&self, owner: &str, id: &str) -> Result<(), String>;
}

fn clients(ctx: &AppContext) -> &dyn ClientConfigurator {
    ctx.services().clients.as_ref()
}

pub fn preview_grok(ctx: &AppContext, alias: &str, model: Option<&str>) -> Result<Preview, String> {
    clients(ctx).preview_grok(alias, model)
}

pub fn preview_opencode(
    ctx: &AppContext,
    provider: &str,
    alias: &str,
    model: &str,
) -> Result<Preview, String> {
    clients(ctx).preview_opencode(provider, alias, model)
}

pub fn preview_opencode_update(
    ctx: &AppContext,
    provider: &str,
    alias: &str,
    model: &str,
) -> Result<Preview, String> {
    clients(ctx).preview_opencode_update(provider, alias, model)
}

pub fn preview_opencode_remove(
    ctx: &AppContext,
    provider: &str,
    alias: &str,
) -> Result<Preview, String> {
    clients(ctx).preview_opencode_remove(provider, alias)
}

pub fn apply(ctx: &AppContext, id: &str) -> Result<Option<String>, String> {
    clients(ctx).apply(id)
}

pub fn preview_hosted(
    ctx: &AppContext,
    request: HostedRequest<'_>,
) -> Result<HostedClientReview, String> {
    clients(ctx).preview_hosted(request)
}

pub fn apply_hosted(ctx: &AppContext, owner: &str, id: &str) -> Result<String, String> {
    clients(ctx).apply_hosted(owner, id)
}

/// Codex session move operations; `None` when there is nothing to show.
pub fn session_move(
    ctx: &AppContext,
    owner: &str,
    operation: &str,
    id: Option<&str>,
    alias: Option<&str>,
) -> Result<Option<SessionMoveView>, String> {
    let session = clients(ctx);
    let selected = || id.ok_or_else(|| "Select a saved session move".to_string());
    Ok(match operation {
        "current" => session.session_current(owner)?,
        "preview" => Some(session.session_preview(owner, alias.ok_or("Choose an account name")?)?),
        "apply" => Some(session.session_apply(owner, selected()?)?),
        "recover" | "cancel" => {
            Some(session.session_recover(owner, selected()?, operation == "cancel")?)
        }
        "dismiss" => {
            session.session_dismiss(owner, selected()?)?;
            None
        }
        _ => return Err("Unknown session move operation".into()),
    })
}
