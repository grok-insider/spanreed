//! Moving accounts between installations.
use crate::context::AppContext;

pub use crate::migration::session::{
    SavedSession, authorizations, cancel, execute, forget, inventory, pair, propose, saved, status,
};
pub use crate::migration::{MigrationCandidate, MigrationSelection, candidates};
pub use fabrials_types::migration::MigrationSessionView;

pub fn begin_authorization(
    ctx: &AppContext,
    id: &str,
    source_id: &str,
) -> Result<crate::account_login::LoginView, String> {
    crate::migration::session::begin_authorization(ctx.logins(), id, source_id)
}
