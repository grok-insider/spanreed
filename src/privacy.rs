//! Independent consent for aggregate publication and private history synchronization.
use fabrials_core::SharingConsent;
use std::process::ExitCode;

pub fn load() -> SharingConsent {
    std::fs::read(crate::app::config_dir().join("sharing.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(consent: &SharingConsent) -> Result<(), String> {
    let directory = crate::app::config_dir();
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(consent).map_err(|e| e.to_string())?;
    fabrials_runtime::files::atomic_write_private(&directory.join("sharing.json"), &bytes)
        .map_err(|e| e.to_string())
}

pub fn cmd(args: &[String]) -> ExitCode {
    let mut consent = load();
    match args {
        [] => {
            println!(
                "{}",
                serde_json::to_string_pretty(&consent).unwrap_or_default()
            );
            return ExitCode::SUCCESS;
        }
        [kind, value]
            if matches!(kind.as_str(), "metrics" | "sync")
                && matches!(value.as_str(), "on" | "off") =>
        {
            if kind == "metrics" {
                consent.share_metrics = value == "on";
            } else {
                consent.sync_history = value == "on";
            }
        }
        _ => {
            eprintln!("Usage: spanreed privacy [metrics|sync on|off]\nMetrics publishes aggregates; sync transfers private request history. Both are off by default.");
            return ExitCode::FAILURE;
        }
    }
    match save(&consent) {
        Ok(()) => {
            println!("Sharing preference saved.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Could not save sharing preference: {error}");
            ExitCode::FAILURE
        }
    }
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
