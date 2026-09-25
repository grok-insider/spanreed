//! The Fabrials account link and the hosted workspace it authorizes.
use serde::{Deserialize, Serialize};

use crate::context::AppContext;

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct FabrialsIdentity {
    pub id: String,
    pub username: String,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum LinkView {
    Disconnected,
    Declined,
    Expired,
    Pending {
        id: String,
        #[serde(rename = "userCode")]
        user_code: String,
        #[serde(rename = "verificationUri")]
        verification_uri: String,
        #[serde(rename = "expiresAtMs")]
        expires_at_ms: i64,
        #[serde(rename = "retryAfterSecs")]
        retry_after_secs: u64,
    },
    Linked {
        user: FabrialsIdentity,
    },
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteOperation {
    #[serde(skip_deserializing)]
    ImportCodexSession,
    CodexSessionStatus,
    CancelCodexSession,
    LinkCodexSource,
    SyncCapabilities,
    PutLocalUsage,
    PutLocalUsageV2,
    Consumption,
    PushSync,
    SetInstallationHostname,
    PullSync,
    RecentSync,
    Dashboard,
    AddAccount,
    ProbeAccount,
    DeleteAccount,
    ActivateAccount,
    SetQuota,
    SetAutosteer,
    CreateKey,
    RevokeKey,
    UpdateKey,
    RotateKey,
    BeginGrok,
    PollGrok,
    CancelGrok,
    BeginCodex,
    ReauthorizeCodex,
    PollCodex,
    CancelCodex,
    BeginNous,
    ReauthorizeNous,
    PollNous,
    CancelNous,
    MigrationSessions,
    MigrationCandidates,
    BeginMigration,
    MigrationStatus,
    ApproveMigration,
    CancelMigration,
    ForgetMigration,
    MigrationAuthorizations,
}

impl RemoteOperation {
    pub fn route(&self) -> (&'static str, &'static str) {
        match self {
            Self::ImportCodexSession => ("POST", "codex/session/import"),
            Self::CodexSessionStatus => ("POST", "codex/session/status"),
            Self::CancelCodexSession => ("POST", "codex/session/cancel"),
            Self::LinkCodexSource => ("POST", "sync/link-codex"),
            Self::SyncCapabilities => ("GET", "sync/capabilities"),
            Self::Consumption => ("GET", "sync/consumption"),
            Self::PutLocalUsageV2 => ("POST", "sync/local-usage-v2"),
            Self::PutLocalUsage => ("POST", "sync/local-usage"),
            Self::PushSync => ("POST", "sync/push"),
            Self::SetInstallationHostname => ("POST", "sync/installation"),
            Self::RecentSync => ("POST", "sync/recent"),
            Self::PullSync => ("POST", "sync/pull"),
            Self::Dashboard => ("GET", "dashboard"),
            Self::AddAccount => ("POST", "accounts"),
            Self::ProbeAccount => ("POST", "accounts/probe"),
            Self::DeleteAccount => ("POST", "accounts/delete"),
            Self::ActivateAccount => ("POST", "accounts/active"),
            Self::SetQuota => ("POST", "accounts/quota"),
            Self::SetAutosteer => ("POST", "autosteer"),
            Self::CreateKey => ("POST", "keys/create"),
            Self::RevokeKey => ("POST", "keys/revoke"),
            Self::UpdateKey => ("POST", "keys/update"),
            Self::RotateKey => ("POST", "keys/rotate"),
            Self::BeginGrok => ("POST", "grok/device"),
            Self::PollGrok => ("POST", "grok/device/wait"),
            Self::CancelGrok => ("POST", "grok/device/cancel"),
            Self::BeginCodex => ("POST", "codex/device"),
            Self::ReauthorizeCodex => ("POST", "codex/device/reauthorize"),
            Self::PollCodex => ("POST", "codex/device/wait"),
            Self::CancelCodex => ("POST", "codex/device/cancel"),
            Self::BeginNous => ("POST", "nous/device"),
            Self::ReauthorizeNous => ("POST", "nous/device/reauthorize"),
            Self::PollNous => ("POST", "nous/device/wait"),
            Self::CancelNous => ("POST", "nous/device/cancel"),
            Self::MigrationSessions => ("GET", "migration/sessions"),
            Self::MigrationCandidates => ("GET", "migration/candidates"),
            Self::BeginMigration => ("POST", "migration/session"),
            Self::MigrationStatus => ("POST", "migration/session/status"),
            Self::ApproveMigration => ("POST", "migration/session/approve"),
            Self::CancelMigration => ("POST", "migration/session/cancel"),
            Self::ForgetMigration => ("POST", "migration/session/forget"),
            Self::MigrationAuthorizations => ("POST", "migration/session/authorizations"),
        }
    }
}

/// Port: the Fabrials device link and the hosted workspace API (HTTP).
pub trait FabrialsLink: Send + Sync {
    fn status(&self) -> Result<LinkView, String>;
    fn begin(&self) -> Result<LinkView, String>;
    fn poll(&self, id: &str) -> Result<LinkView, String>;
    fn cancel(&self, id: &str) -> Result<(), String>;
    fn disconnect(&self) -> Result<(), String>;
    fn verification_url(&self, id: &str) -> Result<String, String>;
    fn remote(
        &self,
        operation: RemoteOperation,
        body: serde_json::Value,
        days: Option<u32>,
        owner: Option<&str>,
    ) -> Result<serde_json::Value, String>;
    /// Validated URL for a hosted authorization page.
    fn authorization_url(&self, raw: &str) -> Result<String, String>;
}

fn link(ctx: &AppContext) -> &dyn FabrialsLink {
    ctx.services().fabrials.as_ref()
}

pub fn status(ctx: &AppContext) -> Result<LinkView, String> {
    link(ctx).status()
}

pub fn begin(ctx: &AppContext) -> Result<LinkView, String> {
    link(ctx).begin()
}

pub fn poll(ctx: &AppContext, id: &str) -> Result<LinkView, String> {
    link(ctx).poll(id)
}

pub fn cancel(ctx: &AppContext, id: &str) -> Result<(), String> {
    link(ctx).cancel(id)
}

pub fn disconnect(ctx: &AppContext) -> Result<(), String> {
    link(ctx).disconnect()
}

pub fn verification_url(ctx: &AppContext, id: &str) -> Result<String, String> {
    link(ctx).verification_url(id)
}

pub fn remote(
    ctx: &AppContext,
    operation: RemoteOperation,
    body: serde_json::Value,
    days: Option<u32>,
    owner: Option<&str>,
) -> Result<serde_json::Value, String> {
    link(ctx).remote(operation, body, days, owner)
}

pub fn authorization_url(ctx: &AppContext, raw: &str) -> Result<String, String> {
    link(ctx).authorization_url(raw)
}
