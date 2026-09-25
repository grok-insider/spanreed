//! Reviewed client configuration: local proxy wiring, hosted relay keys and
//! Codex session moves.
use crate::context::AppContext;

pub use crate::client_configuration::Preview;
pub use crate::codex_session_move::SessionMoveView;
pub use crate::hosted_client_configuration::HostedClientReview;

pub fn preview_grok(ctx: &AppContext, alias: &str, model: Option<&str>) -> Result<Preview, String> {
    ctx.reviews()
        .preview_grok_with_model(ctx.proxy(), alias, model)
}

pub fn preview_opencode(
    ctx: &AppContext,
    provider: &str,
    alias: &str,
    model: &str,
) -> Result<Preview, String> {
    ctx.reviews()
        .preview_opencode(ctx.proxy(), provider, alias, model)
}

pub fn preview_opencode_update(
    ctx: &AppContext,
    provider: &str,
    alias: &str,
    model: &str,
) -> Result<Preview, String> {
    ctx.reviews()
        .preview_opencode_change(ctx.proxy(), provider, alias, model, true)
}

pub fn preview_opencode_remove(
    ctx: &AppContext,
    provider: &str,
    alias: &str,
) -> Result<Preview, String> {
    ctx.reviews()
        .preview_opencode_remove(ctx.proxy(), provider, alias)
}

pub fn apply(ctx: &AppContext, id: &str) -> Result<Option<String>, String> {
    ctx.reviews().apply_configuration(ctx.proxy(), id)
}

/// A hosted relay key for a local client (`codex` or `opencode`).
pub struct HostedRequest<'a> {
    pub owner: &'a str,
    pub client: &'a str,
    pub alias: &'a str,
    pub key: &'a str,
    pub model: &'a str,
}

pub fn preview_hosted(
    ctx: &AppContext,
    request: HostedRequest<'_>,
) -> Result<HostedClientReview, String> {
    ctx.hosted_reviews().preview(
        request.owner,
        request.client,
        request.alias,
        request.key,
        request.model,
    )
}

pub fn apply_hosted(ctx: &AppContext, owner: &str, id: &str) -> Result<String, String> {
    ctx.hosted_reviews().apply(owner, id)
}

/// Codex session move operations; `None` when there is nothing to show.
pub fn session_move(
    owner: &str,
    operation: &str,
    id: Option<&str>,
    alias: Option<&str>,
) -> Result<Option<SessionMoveView>, String> {
    use crate::codex_session_move as session;
    let selected = || id.ok_or_else(|| "Select a saved session move".to_string());
    Ok(match operation {
        "current" => session::current(owner)?,
        "preview" => Some(session::preview(
            owner,
            alias.ok_or("Choose an account name")?,
        )?),
        "apply" => Some(session::apply(owner, selected()?)?),
        "recover" | "cancel" => Some(session::recover(owner, selected()?, operation == "cancel")?),
        "dismiss" => {
            session::dismiss(owner, selected()?)?;
            None
        }
        _ => return Err("Unknown session move operation".into()),
    })
}
