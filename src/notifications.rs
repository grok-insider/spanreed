//! Opt-in local reset-expiry delivery, independent of sharing preferences.
use fabrials_core::{Availability, Freshness, Observation, ResetInventory};
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[cfg_attr(feature = "contracts", ts(rename = "ResetNotificationSettings"))]
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub reset_expiry: bool,
}

pub fn settings() -> Result<Settings, String> {
    use std::io::Read;
    let file = match std::fs::File::open(crate::app::config_dir().join("notifications.json")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default())
        }
        Err(_) => return Err("Notification settings unavailable".into()),
    };
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "Notification settings unavailable")?;
    if bytes.len() > 4096 {
        return Err("Notification settings too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid notification settings".into())
}
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let bytes = serde_json::to_vec(&Settings {
        reset_expiry: enabled,
    })
    .map_err(|_| "Invalid notification settings")?;
    fabrials_runtime::files::atomic_write_private(
        &crate::app::config_dir().join("notifications.json"),
        &bytes,
    )
    .map_err(|_| "Could not save notification settings".into())
}

fn expiring(observation: &Observation<ResetInventory>, now: i64) -> Option<(u32, i64)> {
    if observation.availability != Availability::Available
        || observation.freshness != Freshness::Fresh
    {
        return None;
    }
    let observed = observation.observed_at_ms?;
    if now.saturating_sub(observed) > 900_000 || observed.saturating_sub(now) > 60_000 {
        return None;
    }
    let inventory = observation.value.as_ref()?;
    if inventory.available == 0 {
        return None;
    }
    let expiries: Vec<i64> = inventory
        .credits
        .iter()
        .filter(|credit| {
            credit
                .valid_from_ms
                .is_none_or(|start| start.unsigned_abs() <= 8_640_000_000_000_000 && start <= now)
        })
        .filter_map(|credit| credit.expires_at_ms)
        .filter(|expiry| *expiry > now && expiry.saturating_sub(now) <= 86_400_000)
        .collect();
    Some((
        (expiries.len() as u32).min(inventory.available),
        *expiries.iter().min()?,
    ))
}

pub fn deliver(send: impl FnMut(&str, &str) -> Result<(), String>) -> Result<u32, String> {
    if !settings()?.reset_expiry {
        return Ok(0);
    }
    deliver_outputs(&crate::desktop::snapshot(false), send)
}

pub fn deliver_outputs(
    outputs: &[crate::model::ProviderOutput],
    mut send: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<u32, String> {
    if !settings()?.reset_expiry {
        return Ok(0);
    }
    let store = fabrials_runtime::notifications::DeliveryStore::open(
        &crate::app::data_dir().join("runtime.sqlite3"),
    )?;
    let now = crate::util::now_ms();
    let mut delivered = 0;
    for output in outputs {
        let Some((count, expiry)) = output
            .reset_inventory
            .as_ref()
            .and_then(|observation| expiring(observation, now))
        else {
            continue;
        };
        if !settings()?.reset_expiry {
            return Ok(delivered);
        }
        let key = format!("{}:{expiry}", output.provider_id);
        let Some(claim) = store.claim("local-reset-expiry", &key, now, expiry)? else {
            continue;
        };
        let name: String = output
            .display_name
            .chars()
            .filter(|c| !c.is_control())
            .take(120)
            .collect();
        send("Reset credits expire soon", &format!("{name}: {count} reset credit(s) expire within 24 hours. Open Spanreed for expiry dates."))?;
        store.acknowledge(&claim)?;
        delivered += 1;
    }
    Ok(delivered)
}

pub fn deliver_background(outputs: &[crate::model::ProviderOutput]) -> Result<u32, String> {
    deliver_outputs(outputs, system_notification)
}

fn system_notification(title: &str, body: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    #[cfg(target_os = "linux")]
    let status = std::process::Command::new("notify-send")
        .args([
            "--app-name=Spanreed",
            "--icon=com.fabrials.spanreed",
            "--",
            title,
            body,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("powershell.exe")
        .creation_flags(0x08000000)
        .args(["-NoProfile", "-NonInteractive", "-Command", r#"
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null
[Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] > $null
$document = New-Object Windows.Data.Xml.Dom.XmlDocument
$document.LoadXml('<toast><visual><binding template="ToastGeneric"><text/><text/></binding></visual></toast>')
$nodes = $document.GetElementsByTagName('text')
$nodes.Item(0).AppendChild($document.CreateTextNode($env:SPANREED_NOTIFICATION_TITLE)) > $null
$nodes.Item(1).AppendChild($document.CreateTextNode($env:SPANREED_NOTIFICATION_BODY)) > $null
$toast = [Windows.UI.Notifications.ToastNotification]::new($document)
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('com.fabrials.spanreed').Show($toast)
"#])
        .env("SPANREED_NOTIFICATION_TITLE", title).env("SPANREED_NOTIFICATION_BODY", body)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null()).status();
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    return match status {
        Ok(status) if status.success() => Ok(()),
        _ => Err("Could not deliver the reset notification through the operating system".into()),
    };
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = (title, body);
        Err("Background notifications have not been qualified on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires an isolated OS notification session and SPANREED_NOTIFICATION_QA_ROOT"]
    fn background_notification_survives_restart_without_duplicate_delivery() {
        let root = std::path::PathBuf::from(
            std::env::var("SPANREED_NOTIFICATION_QA_ROOT").expect("isolated QA root required"),
        );
        assert!(root.is_absolute());
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("XDG_DATA_HOME", root.join("data"));
        let now = crate::util::now_ms();
        std::fs::create_dir_all(&root).unwrap();
        let restart = std::env::var_os("SPANREED_NOTIFICATION_QA_RESTART").is_some();
        let expiry = if restart {
            std::fs::read_to_string(root.join("expiry"))
                .unwrap()
                .parse()
                .unwrap()
        } else {
            let expiry = now + 3_600_000;
            std::fs::write(root.join("expiry"), expiry.to_string()).unwrap();
            expiry
        };
        let mut output = crate::model::ProviderOutput::error(
            "qa/reset-fixture",
            "Spanreed QA fixture (test)",
            "Test observation",
        );
        output.reset_inventory = Some(Observation {
            availability: Availability::Available,
            freshness: Freshness::Fresh,
            observed_at_ms: Some(now),
            source: "isolated QA".into(),
            error: None,
            value: Some(ResetInventory {
                available: 1,
                details_complete: true,
                credits: vec![fabrials_core::ResetCredit {
                    valid_from_ms: None,
                    expires_at_ms: Some(expiry),
                }],
            }),
        });
        set_enabled(true).unwrap();
        assert_eq!(
            deliver_background(std::slice::from_ref(&output)).unwrap(),
            if restart { 0 } else { 1 }
        );
        assert_eq!(deliver_background(&[output]).unwrap(), 0);
        if !restart {
            assert!(std::process::Command::new(std::env::current_exe().unwrap()).args(["notifications::tests::background_notification_survives_restart_without_duplicate_delivery","--ignored","--exact"]).env("SPANREED_NOTIFICATION_QA_RESTART","1").status().unwrap().success());
            set_enabled(false).unwrap();
        }
    }
    #[test]
    fn notification_policy_requires_recent_current_credits() {
        let mut observed = Observation {
            availability: Availability::Available,
            freshness: Freshness::Fresh,
            observed_at_ms: Some(1_000_000),
            source: "fixture".into(),
            error: None,
            value: Some(ResetInventory {
                available: 1,
                details_complete: true,
                credits: vec![fabrials_core::ResetCredit {
                    valid_from_ms: None,
                    expires_at_ms: Some(2_000_000),
                }],
            }),
        };
        assert_eq!(expiring(&observed, 1_000_000), Some((1, 2_000_000)));
        assert_eq!(expiring(&observed, 2_000_000), None);
        observed.freshness = Freshness::Stale;
        assert_eq!(expiring(&observed, 1_000_000), None);
        observed.freshness = Freshness::Fresh;
        observed.observed_at_ms = None;
        assert_eq!(expiring(&observed, 1_000_000), None);
    }
}
