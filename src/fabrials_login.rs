//! Fabrials linking state. Device proofs and session tokens stay in the native host.
use crate::share_session::{self, PendingLogin, ShareSession};
use fabrials_runtime::credential_journal::{Rotation, Scope};
use std::path::PathBuf;

#[derive(serde::Serialize, serde::Deserialize)]
struct Pending {
    id: String,
    #[serde(default)]
    api_base: Option<String>,
    authorization: PendingLogin,
    expires_at_ms: i64,
    next_poll_ms: i64,
    authorized: Option<ShareSession>,
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
#[derive(serde::Serialize, serde::Deserialize)]
pub struct FabrialsIdentity {
    pub id: String,
    pub username: String,
}

fn linked_view() -> Result<LinkView, String> {
    let base = crate::share::api_base();
    let token = share_session::ensure_access(&base)?;
    let response =
        crate::http::Request::get(format!("{}/auth/session", base.trim_end_matches('/')))
            .bearer(&token)
            .send()?;
    if response.status != 200 {
        return Err("Fabrials session could not be verified. Reconnect if it has expired.".into());
    }
    let identity = response
        .json()
        .ok_or("Invalid Fabrials identity response")?;
    if identity
        .get("installation")
        .and_then(serde_json::Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(
            "This session predates installation linking. Connect this installation again.".into(),
        );
    }
    let value = Some(identity)
        .and_then(|value| value.get("user").cloned())
        .ok_or("Invalid Fabrials identity response")?;
    let user: FabrialsIdentity =
        serde_json::from_value(value).map_err(|_| "Invalid Fabrials identity")?;
    Ok(LinkView::Linked { user })
}

fn path() -> PathBuf {
    crate::app::config_dir().join("fabrials-link.json")
}
fn lock() -> Result<Rotation, String> {
    let environment = crate::app::config_dir().to_string_lossy().into_owned();
    Rotation::acquire(
        &crate::app::data_dir().join("credential-recovery"),
        Scope {
            environment: &environment,
            owner: "local",
            provider: "fabrials-device",
            alias: "link",
        },
    )
}
fn load() -> Result<Option<Pending>, String> {
    match std::fs::read(path()) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "Invalid saved Fabrials connection".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Could not read Fabrials connection".into()),
    }
}
fn save(pending: &Pending) -> Result<(), String> {
    let bytes = serde_json::to_vec(pending).map_err(|_| "Invalid Fabrials connection")?;
    fabrials_runtime::files::atomic_write_private(&path(), &bytes)
        .map_err(|_| "Could not save Fabrials connection".into())
}
fn remove() -> Result<(), String> {
    match std::fs::remove_file(path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Could not clear Fabrials connection".into()),
    }
}
fn validate_pending_base(pending: &Pending, base: &str) -> Result<(), String> {
    let bound = pending
        .api_base
        .as_deref()
        .unwrap_or("https://fabrials.com/api/spanreed");
    if bound.trim_end_matches('/') != base.trim_end_matches('/') {
        return Err("Fabrials origin changed. Cancel this connection and start again.".into());
    }
    Ok(())
}
fn view(pending: &Pending, now: i64) -> LinkView {
    LinkView::Pending {
        id: pending.id.clone(),
        user_code: pending.authorization.user_code.clone(),
        verification_uri: pending.authorization.verification_uri.clone(),
        expires_at_ms: pending.expires_at_ms,
        retry_after_secs: ((pending.next_poll_ms - now).max(0) as u64).div_ceil(1000),
    }
}

pub fn status() -> Result<LinkView, String> {
    let _lock = lock()?;
    if let Some(pending) = load()? {
        if pending.authorized.is_some() || crate::util::now_ms() < pending.expires_at_ms {
            return Ok(view(&pending, crate::util::now_ms()));
        }
        remove()?;
        return Ok(LinkView::Expired);
    }
    if share_session::is_logged_in() {
        linked_view()
    } else {
        Ok(LinkView::Disconnected)
    }
}

pub fn begin() -> Result<LinkView, String> {
    let _lock = lock()?;
    let now = crate::util::now_ms();
    if let Some(pending) = load()? {
        validate_pending_base(&pending, &crate::share::api_base())?;
        if pending.authorized.is_some() || now < pending.expires_at_ms {
            return Ok(view(&pending, now));
        }
    }
    let authorization = share_session::start_device_login()?;
    let pending = Pending {
        id: format!("{}-{now}", authorization.user_code),
        api_base: Some(crate::share::api_base()),
        expires_at_ms: now.saturating_add(authorization.expires_in.min(3600) as i64 * 1000),
        next_poll_ms: now.saturating_add(authorization.interval_secs.clamp(2, 60) as i64 * 1000),
        authorization,
        authorized: None,
    };
    save(&pending)?;
    Ok(view(&pending, now))
}

pub fn poll(id: &str) -> Result<LinkView, String> {
    let _lock = lock()?;
    let mut pending = load()?.ok_or("Fabrials connection no longer exists")?;
    if pending.id != id {
        return Err("Fabrials connection changed; reopen the current connection".into());
    }
    validate_pending_base(&pending, &crate::share::api_base())?;
    let now = crate::util::now_ms();
    if pending.authorized.is_none() {
        if now >= pending.expires_at_ms {
            remove()?;
            return Ok(LinkView::Expired);
        }
        if now < pending.next_poll_ms {
            return Ok(view(&pending, now));
        }
        pending.next_poll_ms =
            now.saturating_add(pending.authorization.interval_secs.clamp(2, 60) as i64 * 1000);
        save(&pending)?;
        match share_session::check_device_login(&pending.authorization)? {
            share_session::DevicePoll::Pending => {}
            share_session::DevicePoll::Approved(session) => pending.authorized = Some(session),
            share_session::DevicePoll::Declined => {
                remove()?;
                return Ok(LinkView::Declined);
            }
            share_session::DevicePoll::Expired => {
                remove()?;
                return Ok(LinkView::Expired);
            }
        }
        save(&pending)?;
    }
    if let Some(ref session) = pending.authorized {
        share_session::save(session)?;
        remove()?;
        return linked_view();
    }
    Ok(view(&pending, now))
}

pub fn cancel(id: &str) -> Result<(), String> {
    let _lock = lock()?;
    if let Some(pending) = load()? {
        if pending.id != id {
            return Err("Fabrials connection changed".into());
        }
        remove()?;
    }
    Ok(())
}

pub fn disconnect() -> Result<(), String> {
    let _lock = lock()?;
    if share_session::is_logged_in() {
        let base = crate::share::api_base();
        let token = share_session::ensure_access(&base)?;
        let response =
            crate::http::Request::delete(format!("{}/auth/session", base.trim_end_matches('/')))
                .bearer(&token)
                .send()?;
        if response.status != 200 && response.status != 401 {
            return Err(
                "Could not revoke this installation. Check your connection and retry.".into(),
            );
        }
        share_session::clear()?;
    }
    remove()
}

pub fn verification_url(id: &str) -> Result<String, String> {
    let _lock = lock()?;
    let pending = load()?.ok_or("No pending Fabrials connection")?;
    if pending.id != id || crate::util::now_ms() >= pending.expires_at_ms {
        return Err("Fabrials connection expired or changed".into());
    }
    validate_pending_base(&pending, &crate::share::api_base())?;
    let url = reqwest::Url::parse(&pending.authorization.verification_uri)
        .map_err(|_| "Invalid verification URL")?;
    let base =
        reqwest::Url::parse(&crate::share::api_base()).map_err(|_| "Invalid Fabrials origin")?;
    if url.origin() != base.origin()
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Fabrials verification URL must use the configured HTTPS origin".into());
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_view_never_contains_device_or_session_secrets() {
        let pending = Pending {
            id: "flow".into(),
            api_base: Some("https://fabrials.com/api/spanreed".into()),
            authorization: PendingLogin {
                device_code: "secret-proof".into(),
                user_code: "ABCD-EFGH".into(),
                verification_uri: "https://fabrials.com/link".into(),
                interval_secs: 5,
                expires_in: 600,
            },
            expires_at_ms: 600_000,
            next_poll_ms: 5000,
            authorized: None,
        };
        assert!(validate_pending_base(&pending, "https://fabrials.com/api/spanreed/").is_ok());
        assert!(validate_pending_base(&pending, "https://other.example/api/spanreed").is_err());
        let encoded = serde_json::to_string(&view(&pending, 1000)).unwrap();
        assert!(!encoded.contains("secret-proof"));
        assert!(!encoded.contains("device_code"));
        assert!(encoded.contains("ABCD-EFGH"));
        assert!(encoded.contains("\"retryAfterSecs\":4"));
    }
}
