//! Moving accounts between installations.
use serde::Serialize;

use crate::app::accounts::LoginView;
use crate::context::AppContext;

pub use fabrials_types::migration::{MigrationCandidate, MigrationSelection, MigrationSessionView};

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSession {
    pub id: String,
    pub origin: String,
    pub view: Option<MigrationSessionView>,
    pub imported: Option<Vec<String>>,
}

/// Port: migration sessions (paired origins, reviewed transfers).
pub trait Migrations: Send + Sync {
    fn saved(&self) -> Result<Vec<SavedSession>, String>;
    fn candidates(&self) -> Result<Vec<MigrationCandidate>, String>;
    fn forget(&self, id: &str) -> Result<(), String>;
    fn begin_authorization(&self, id: &str, source_id: &str) -> Result<LoginView, String>;
    fn authorizations(&self, id: &str) -> Result<Vec<String>, String>;
    fn inventory(&self, id: &str) -> Result<Vec<MigrationCandidate>, String>;
    fn propose(
        &self,
        id: &str,
        selection: &[MigrationSelection],
    ) -> Result<MigrationSessionView, String>;
    fn execute(&self, id: &str, revision: &str) -> Result<Vec<String>, String>;
    fn pair(
        &self,
        origin: &str,
        id: &str,
        invitation: &str,
    ) -> Result<MigrationSessionView, String>;
    fn status(&self, id: &str) -> Result<MigrationSessionView, String>;
    fn cancel(&self, id: &str) -> Result<(), String>;
}

fn migrations(ctx: &AppContext) -> &dyn Migrations {
    ctx.services().migrations.as_ref()
}

pub fn saved(ctx: &AppContext) -> Result<Vec<SavedSession>, String> {
    migrations(ctx).saved()
}

pub fn candidates(ctx: &AppContext) -> Result<Vec<MigrationCandidate>, String> {
    migrations(ctx).candidates()
}

pub fn forget(ctx: &AppContext, id: &str) -> Result<(), String> {
    migrations(ctx).forget(id)
}

pub fn begin_authorization(
    ctx: &AppContext,
    id: &str,
    source_id: &str,
) -> Result<LoginView, String> {
    migrations(ctx).begin_authorization(id, source_id)
}

pub fn authorizations(ctx: &AppContext, id: &str) -> Result<Vec<String>, String> {
    migrations(ctx).authorizations(id)
}

pub fn inventory(ctx: &AppContext, id: &str) -> Result<Vec<MigrationCandidate>, String> {
    migrations(ctx).inventory(id)
}

pub fn propose(
    ctx: &AppContext,
    id: &str,
    selection: &[MigrationSelection],
) -> Result<MigrationSessionView, String> {
    migrations(ctx).propose(id, selection)
}

pub fn execute(ctx: &AppContext, id: &str, revision: &str) -> Result<Vec<String>, String> {
    migrations(ctx).execute(id, revision)
}

pub fn pair(
    ctx: &AppContext,
    origin: &str,
    id: &str,
    invitation: &str,
) -> Result<MigrationSessionView, String> {
    migrations(ctx).pair(origin, id, invitation)
}

pub fn status(ctx: &AppContext, id: &str) -> Result<MigrationSessionView, String> {
    migrations(ctx).status(id)
}

pub fn cancel(ctx: &AppContext, id: &str) -> Result<(), String> {
    migrations(ctx).cancel(id)
}
