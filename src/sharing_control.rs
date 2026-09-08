//! Desktop publication controls; consent and scheduling remain independent.
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
        last_shared_day: crate::share::last_shared_day(),
        due: crate::share::is_due_today(),
        schedule: crate::share_schedule::status(),
    }
}
pub fn publish() -> Result<String, String> {
    crate::share::share_once(false)
}
pub fn schedule(enabled: bool) -> Result<String, String> {
    if enabled {
        if !crate::privacy::load().share_metrics {
            return Err("Enable and save metrics publication before scheduling uploads".into());
        }
        if !crate::share_session::is_logged_in() {
            return Err("Connect this installation to Fabrials first".into());
        }
        crate::share_schedule::enable(false)
    } else {
        crate::share_schedule::disable(false)
    }
}
