//! Publication of plan metrics, share login and consent. Consent and
//! scheduling remain independent.
use crate::context::AppContext;

pub use fabrials_types::SharingConsent;

pub const DEFAULT_API_BASE: &str = "https://fabrials.com/api/spanreed";
pub const TIME_ZONE_LABEL: &str = crate::util::SHARE_TZ_LABEL;

/// In-flight device authorization (RFC 8628-style).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingLogin {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval_secs: u64,
    pub expires_in: u64,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationStatus {
    pub last_shared_day: Option<String>,
    pub due: bool,
    pub schedule: String,
}

/// Port: sharing consent, the share session (device login) and uploads.
pub trait Sharing: Send + Sync {
    fn consent(&self) -> SharingConsent;
    fn save_consent(&self, consent: &SharingConsent) -> Result<(), String>;
    fn is_linked(&self) -> bool;
    /// A stored share session that can refresh its access token.
    fn has_refresh_session(&self) -> bool;
    fn last_shared_day(&self) -> Option<String>;
    fn due_today(&self) -> bool;
    fn begin_link(&self) -> Result<PendingLogin, String>;
    fn wait_link(&self, pending: &PendingLogin) -> Result<(), String>;
    fn unlink(&self) -> Result<(), String>;
    /// Probe and upload once; `force` bypasses the same-day skip.
    fn share_now(&self, ctx: &AppContext, force: bool) -> Result<String, String>;
}

fn sharing(ctx: &AppContext) -> &dyn Sharing {
    ctx.services().sharing.as_ref()
}

pub fn consent(ctx: &AppContext) -> SharingConsent {
    sharing(ctx).consent()
}

pub fn save_consent(ctx: &AppContext, consent: &SharingConsent) -> Result<(), String> {
    sharing(ctx).save_consent(consent)
}

pub fn is_linked(ctx: &AppContext) -> bool {
    sharing(ctx).is_linked()
}

pub fn last_shared_day(ctx: &AppContext) -> Option<String> {
    sharing(ctx).last_shared_day()
}

/// Start the device login that links this installation for sharing.
pub fn begin_link(ctx: &AppContext) -> Result<PendingLogin, String> {
    sharing(ctx).begin_link()
}

pub fn has_refresh_session(ctx: &AppContext) -> bool {
    sharing(ctx).has_refresh_session()
}

/// Wait for the browser approval of `pending`.
pub fn wait_link(ctx: &AppContext, pending: &PendingLogin) -> Result<(), String> {
    sharing(ctx).wait_link(pending)
}

/// Wait for approval, then enable the daily schedule.
pub fn finish_link(ctx: &AppContext, pending: &PendingLogin) -> Result<(), String> {
    sharing(ctx).wait_link(pending)?;
    let _ = ctx.services().installer.enable_share_schedule(false);
    Ok(())
}

pub fn unlink(ctx: &AppContext) -> Result<(), String> {
    sharing(ctx).unlink()
}

/// Probe and upload once; `force` bypasses the same-day skip.
pub fn share_now(ctx: &AppContext, force: bool) -> Result<String, String> {
    sharing(ctx).share_now(ctx, force)
}

pub fn status(ctx: &AppContext) -> PublicationStatus {
    PublicationStatus {
        last_shared_day: sharing(ctx).last_shared_day(),
        due: sharing(ctx).due_today(),
        schedule: ctx.services().installer.share_schedule_status(),
    }
}

pub fn publish(ctx: &AppContext) -> Result<String, String> {
    share_now(ctx, false)
}

pub fn schedule(ctx: &AppContext, enabled: bool) -> Result<String, String> {
    let installer = ctx.services().installer.as_ref();
    if enabled {
        if !consent(ctx).share_metrics {
            return Err("Enable and save metrics publication before scheduling uploads".into());
        }
        if !is_linked(ctx) {
            return Err("Connect this installation to Fabrials first".into());
        }
        installer.enable_share_schedule(false)
    } else {
        installer.disable_share_schedule(false)
    }
}
