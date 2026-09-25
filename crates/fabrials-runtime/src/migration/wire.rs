//! Native migration wire contracts shared by the client and hosted boundary.
use fabrials_accounts::transfer::ApiKeyTransfer;
use fabrials_core::migration::{MigrationCandidate, MigrationReview};

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_environment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<MigrationReview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<ApiKeyTransfer>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Inventory {
    pub accounts: Vec<MigrationCandidate>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Revision {
    pub revision: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    pub account_ids: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Exported {
    pub entries: Vec<ApiKeyTransfer>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct OkResponse {
    pub ok: bool,
}
