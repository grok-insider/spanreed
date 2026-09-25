//! The Fabrials account link and the hosted workspace it authorizes.

pub use crate::fabrials_login::{
    FabrialsIdentity, LinkView, begin, cancel, disconnect, poll, status, verification_url,
};
pub use crate::remote_workspace::RemoteOperation;

pub fn remote(
    operation: RemoteOperation,
    body: serde_json::Value,
    days: Option<u32>,
    owner: Option<&str>,
) -> Result<serde_json::Value, String> {
    crate::remote_workspace::request(operation, body, days, owner)
}

/// Validated URL for a hosted authorization page.
pub fn authorization_url(raw: &str) -> Result<String, String> {
    crate::remote_workspace::verification_url(raw)
}
