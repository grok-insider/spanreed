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

pub fn deliver(mut send: impl FnMut(&str, &str) -> Result<(), String>) -> Result<u32, String> {
    if !settings()?.reset_expiry {
        return Ok(0);
    }
    let outputs = crate::desktop::snapshot(false);
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

#[cfg(test)]
mod tests {
    use super::*;
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
