//! Independent consent for aggregate publication and private history synchronization.
use fabrials_types::SharingConsent;

pub fn load() -> SharingConsent {
    std::fs::read(crate::product::config_dir().join("sharing.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(consent: &SharingConsent) -> Result<(), String> {
    let directory = crate::product::config_dir();
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(consent).map_err(|e| e.to_string())?;
    fabrials_store_sqlite::files::atomic_write_private(&directory.join("sharing.json"), &bytes)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn consent_is_separate_and_defaults_to_private() {
        let defaults = SharingConsent::default();
        assert!(!defaults.share_metrics && !defaults.sync_history);
        let metrics = SharingConsent {
            share_metrics: true,
            ..Default::default()
        };
        assert!(!metrics.sync_history);
    }
}
