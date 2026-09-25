//! Publication of plan metrics, share login and consent. Consent and
//! scheduling remain independent.
use crate::context::AppContext;

pub use crate::share_session::PendingLogin;
pub use fabrials_types::SharingConsent;

pub fn consent() -> SharingConsent {
    crate::privacy::load()
}

pub fn save_consent(consent: &SharingConsent) -> Result<(), String> {
    crate::privacy::save(consent)
}

pub fn is_linked() -> bool {
    crate::share_session::is_logged_in()
}

pub fn last_shared_day() -> Option<String> {
    crate::share_state::last_shared_day()
}

/// Start the device login that links this installation for sharing.
pub fn begin_link() -> Result<PendingLogin, String> {
    crate::share_session::start_device_login()
}

pub const DEFAULT_API_BASE: &str = crate::share_state::DEFAULT_API_BASE;
pub const TIME_ZONE_LABEL: &str = crate::util::SHARE_TZ_LABEL;

/// A stored share session that can refresh its access token.
pub fn has_refresh_session() -> bool {
    crate::share_session::load().is_some_and(|session| !session.refresh_token.is_empty())
}

/// Wait for the browser approval of `pending`.
pub fn wait_link(pending: &PendingLogin) -> Result<(), String> {
    crate::share_session::wait_device_login(pending).map(|_| ())
}

/// Wait for approval, then enable the daily schedule.
pub fn finish_link(pending: &PendingLogin) -> Result<(), String> {
    crate::share_session::wait_device_login(pending)?;
    let _ = crate::setup::share_schedule::enable(false);
    Ok(())
}

pub fn unlink() -> Result<(), String> {
    crate::share_session::clear()
}

/// Probe and upload once; `force` bypasses the same-day skip.
pub fn share_now(ctx: &AppContext, force: bool) -> Result<String, String> {
    crate::share::share_once(ctx, force)
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationStatus {
    pub last_shared_day: Option<String>,
    pub due: bool,
    pub schedule: String,
}
pub fn status() -> PublicationStatus {
    PublicationStatus {
        last_shared_day: crate::share_state::last_shared_day(),
        due: crate::share_state::is_due_today(),
        schedule: crate::setup::share_schedule::status(),
    }
}
pub fn publish(ctx: &AppContext) -> Result<String, String> {
    crate::share::share_once(ctx, false)
}
pub fn schedule(enabled: bool) -> Result<String, String> {
    if enabled {
        if !crate::privacy::load().share_metrics {
            return Err("Enable and save metrics publication before scheduling uploads".into());
        }
        if !crate::share_session::is_logged_in() {
            return Err("Connect this installation to Fabrials first".into());
        }
        crate::setup::share_schedule::enable(false)
    } else {
        crate::setup::share_schedule::disable(false)
    }
}
