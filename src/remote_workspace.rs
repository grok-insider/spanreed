//! Native transport for the hosted workspace. Renderer input cannot select an origin.
use serde::{Deserialize, Serialize};

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
    PushSync,
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
    fn route(&self) -> (&'static str, &'static str) {
        match self {
            Self::ImportCodexSession => ("POST", "codex/session/import"),
            Self::CodexSessionStatus => ("POST", "codex/session/status"),
            Self::CancelCodexSession => ("POST", "codex/session/cancel"),
            Self::LinkCodexSource => ("POST", "sync/link-codex"),
            Self::SyncCapabilities => ("GET", "sync/capabilities"),
            Self::PutLocalUsage => ("POST", "sync/local-usage"),
            Self::PushSync => ("POST", "sync/push"),
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

enum Principal<'a> {
    Owner(&'a str),
    Subject(&'a str),
    Discovery,
}

pub fn request(
    operation: RemoteOperation,
    body: serde_json::Value,
    days: Option<u32>,
    expected_owner: Option<&str>,
) -> Result<serde_json::Value, String> {
    let principal = match expected_owner {
        Some(owner) => Principal::Owner(owner),
        None if matches!(operation, RemoteOperation::Dashboard) => Principal::Discovery,
        None => return Err("Refresh the hosted workspace before performing this operation".into()),
    };
    execute(operation, body, days, principal)
}
pub fn request_for_subject(
    operation: RemoteOperation,
    body: serde_json::Value,
    subject: &str,
) -> Result<serde_json::Value, String> {
    execute(operation, body, None, Principal::Subject(subject))
}
fn check_principal(token: &str, principal: Principal<'_>) -> Result<(), String> {
    if matches!(principal, Principal::Discovery) {
        return Ok(());
    }
    // This is a local stale-session guard, not authentication. ai-relay verifies
    // the signature and live installation grant for this exact token.
    let claims = crate::util::jwt_payload(token).ok_or("Invalid Fabrials session")?;
    let subject = claims["sub"].as_str().ok_or("Invalid Fabrials session")?;
    let owner = claims["x_user_id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or(subject);
    let matches = match principal {
        Principal::Owner(expected) => owner == expected,
        Principal::Subject(expected) => subject == expected,
        Principal::Discovery => true,
    };
    if matches {
        Ok(())
    } else {
        Err("Fabrials account changed. Refresh before continuing this operation.".into())
    }
}
fn execute(
    operation: RemoteOperation,
    body: serde_json::Value,
    days: Option<u32>,
    principal: Principal<'_>,
) -> Result<serde_json::Value, String> {
    let (method, path) = operation.route();
    let mut url = format!("https://ai.fabrials.com/ui/native/v1/{path}");
    if matches!(operation, RemoteOperation::Dashboard) {
        let days = days.unwrap_or(7);
        if ![1, 7, 30].contains(&days) {
            return Err("Choose a 1, 7 or 30 day window".into());
        }
        url.push_str(&format!("?days={days}"));
    }
    if crate::share::api_base().trim_end_matches('/') != "https://fabrials.com/api/spanreed" {
        return Err("The hosted workspace requires a session linked through fabrials.com".into());
    }
    let token = crate::share_session::ensure_access(&crate::share::api_base())?;
    check_principal(&token, principal)?;
    let subject = crate::util::jwt_payload(&token)
        .and_then(|claims| claims["sub"].as_str().map(str::to_owned))
        .ok_or("Invalid Fabrials session")?;
    let request = if method == "GET" {
        crate::http::Request::get(url)
    } else {
        if !body.is_object() {
            return Err("Invalid remote request".into());
        }
        let body = serde_json::to_string(&body).map_err(|_| "Invalid remote request")?;
        if body.len() > 65_536 {
            return Err("Remote request is too large".into());
        }
        crate::http::Request::post(url)
            .header("Content-Type", "application/json")
            .body(body)
    };
    let response = request
        .bearer(&token)
        .header("Accept", "application/json")
        .send_limited(8 * 1024 * 1024)?;
    let current = crate::share_session::load()
        .ok_or("Fabrials installation disconnected during the operation")?;
    check_principal(&current.access_token, Principal::Subject(&subject))?;
    if response.status == 401 {
        return Err("Reconnect this installation to Fabrials to open ai-relay.".into());
    }
    if response.status == 403 {
        return Err(
            "This Fabrials account does not have access to this ai-relay operation.".into(),
        );
    }
    if !(200..300).contains(&response.status) {
        if let Some(message) = response.json().as_ref().and_then(public_remote_error) {
            return Err(message.into());
        }
        return Err(format!("ai-relay could not complete the operation (HTTP {}). Refresh before retrying a change.", response.status));
    }
    response
        .json()
        .filter(|value| {
            value.is_object()
                || (matches!(operation, RemoteOperation::MigrationAuthorizations)
                    && value.is_array())
        })
        .ok_or_else(|| "Invalid ai-relay response".into())
}

fn public_remote_error(body: &serde_json::Value) -> Option<&'static str> {
    match body.get("error")?.as_str()? {
        "provider_identity_already_linked" => Some("This provider identity is already connected. Renew authorization on its existing account."),
        "account_name_already_exists" => Some("This account name already exists. Choose another name or renew the existing account."),
        _ => None,
    }
}

pub fn verification_url(raw: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "Invalid provider authorization URL")?;
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("auth.x.ai" | "accounts.x.ai" | "portal.nousresearch.com" | "auth.openai.com")
        )
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Unexpected provider authorization origin".into());
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_known_public_error_codes_reach_the_renderer() {
        assert!(public_remote_error(
            &serde_json::json!({"error":"provider_identity_already_linked"})
        )
        .unwrap()
        .contains("existing account"));
        assert!(public_remote_error(&serde_json::json!({"error":"secret-token-value"})).is_none());
        assert!(public_remote_error(&serde_json::json!({"detail":"secret-token-value"})).is_none());
    }
    #[test]
    fn stale_owner_and_subject_cannot_use_replacement_session() {
        let token = "e30.eyJzdWIiOiJhbGljZSIsInhfdXNlcl9pZCI6IngtYWxpY2UifQ.signature";
        assert!(check_principal(token, Principal::Owner("x-alice")).is_ok());
        assert!(check_principal(token, Principal::Subject("alice")).is_ok());
        assert!(check_principal(token, Principal::Owner("bob")).is_err());
        assert!(check_principal(token, Principal::Owner("alice")).is_err());
        assert!(check_principal(token, Principal::Subject("x-alice")).is_err());
        assert!(check_principal("invalid", Principal::Subject("alice")).is_err());
        assert!(request(
            RemoteOperation::CreateKey,
            serde_json::json!({}),
            None,
            None
        )
        .is_err());
    }
    #[test]
    fn renderer_cannot_supply_proxy_routes_or_arbitrary_origins() {
        for operation in [
            "https://evil.example",
            "/v1/responses",
            "admin/pool",
            "../dashboard",
        ] {
            assert!(
                serde_json::from_value::<RemoteOperation>(serde_json::json!(operation)).is_err()
            );
        }
        assert_eq!(RemoteOperation::CreateKey.route(), ("POST", "keys/create"));
        assert_eq!(RemoteOperation::Dashboard.route(), ("GET", "dashboard"));
    }
}
