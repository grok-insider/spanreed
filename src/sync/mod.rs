//! Opt-in, selected-source private synchronization across compatible providers.
mod run;

use crate::remote_workspace::{RemoteOperation, request_for_subject};
use run::run_outputs;
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSettings {
    pub sources: Vec<String>,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub last_success_ms: Option<i64>,
    pub uploaded: usize,
    pub downloaded: usize,
    pub error: Option<String>,
}

fn cached_owner() -> Option<String> {
    let session = crate::share_session::load()?;
    crate::util::jwt_payload(&session.access_token)?
        .get("sub")?
        .as_str()
        .filter(|owner| !owner.is_empty())
        .map(str::to_owned)
}
pub fn recent(
    before: Option<i64>,
) -> Result<fabrials_types::private_sync::PrivateRecentPage, String> {
    if before.is_some_and(|value| value <= 0) {
        return Err("Invalid private history cursor".into());
    }
    let owner = cached_owner().ok_or("Connect this installation to view its downloaded history")?;
    crate::sync_store::Store::open()?.recent(&owner, before)
}

#[derive(Serialize, Deserialize)]
struct OwnedSelection {
    #[serde(default)]
    identities: std::collections::BTreeMap<String, String>,
    owner: String,
    settings: SyncSettings,
}
#[derive(Serialize, Deserialize)]
struct OwnedStatus {
    owner: String,
    status: SyncStatus,
}

pub fn settings() -> Result<SyncSettings, String> {
    match std::fs::read(crate::product::config_dir().join("sync-selection.json")) {
        Ok(bytes) => {
            // Legacy selections were not bound to an identity; require a new explicit selection.
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| "Invalid sync selection")?;
            if value.get("owner").is_none() {
                return Ok(SyncSettings::default());
            }
            let saved: OwnedSelection =
                serde_json::from_value(value).map_err(|_| "Invalid sync selection")?;
            if cached_owner().as_deref() == Some(saved.owner.as_str())
                && saved
                    .settings
                    .sources
                    .iter()
                    .any(|source| source == "codex")
            {
                let current = crate::providers::codex::local_identity().unwrap_or_default();
                if saved.identities.get("codex") != Some(&current) {
                    return Err("Review and save synchronization sources to confirm the current local Codex account before uploading.".into());
                }
            }
            Ok(if cached_owner().as_deref() == Some(saved.owner.as_str()) {
                saved.settings
            } else {
                SyncSettings::default()
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SyncSettings::default()),
        Err(_) => Err("Could not read sync selection".into()),
    }
}
pub fn save_settings(mut settings: SyncSettings) -> Result<(), String> {
    if settings.sources.len() > 200
        || settings.sources.iter().any(|source| {
            source.is_empty()
                || source.len() > 128
                || !source
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_/:.".contains(&byte))
        })
    {
        return Err("Invalid sync sources".into());
    }
    settings.sources.sort();
    settings.sources.dedup();
    let owner = cached_owner()
        .ok_or("Connect this installation before selecting synchronization sources")?;
    let mut identities = std::collections::BTreeMap::new();
    if settings.sources.iter().any(|source| source == "codex") {
        identities.insert(
            "codex".into(),
            crate::providers::codex::local_identity().unwrap_or_default(),
        );
    }
    let bytes = serde_json::to_vec(&OwnedSelection {
        owner,
        settings,
        identities,
    })
    .map_err(|_| "Invalid sync selection")?;
    fabrials_runtime::files::atomic_write_private(
        &crate::product::config_dir().join("sync-selection.json"),
        &bytes,
    )
    .map_err(|_| "Could not save sync selection".into())
}
pub fn status() -> SyncStatus {
    let Some(owner) = cached_owner() else {
        return SyncStatus::default();
    };
    status_for(&owner)
}
fn status_for(owner: &str) -> SyncStatus {
    std::fs::read(crate::product::data_dir().join("sync-status.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<OwnedStatus>(&bytes).ok())
        .filter(|saved| saved.owner == owner)
        .map(|saved| saved.status)
        .unwrap_or_default()
}
fn save_status(owner: &str, status: SyncStatus) {
    if let Ok(bytes) = serde_json::to_vec(&OwnedStatus {
        owner: owner.to_owned(),
        status,
    }) {
        let _ = fabrials_runtime::files::atomic_write_private(
            &crate::product::data_dir().join("sync-status.json"),
            &bytes,
        );
    }
}
/// `AfterProbe` hook: uploads fresh outputs when private history sync is on.
/// At most one upload runs at a time per hook instance.
pub struct HistorySync {
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pricing: std::sync::Arc<crate::pricing::Catalog>,
}

impl HistorySync {
    pub fn new(pricing: std::sync::Arc<crate::pricing::Catalog>) -> Self {
        Self {
            running: Default::default(),
            pricing,
        }
    }
}

impl crate::ports::AfterProbe for HistorySync {
    fn after_probe(&self, outputs: &[crate::model::ProviderOutput]) {
        use std::sync::atomic::Ordering;
        if !crate::privacy::load().sync_history
            || crate::product::env_offline()
            || !crate::share_session::is_logged_in()
        {
            return;
        }
        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let outputs = outputs.to_vec();
        let owner = cached_owner();
        let running = std::sync::Arc::clone(&self.running);
        let pricing = self.pricing.table();
        std::thread::spawn(move || {
            struct Guard(std::sync::Arc<std::sync::atomic::AtomicBool>);
            impl Drop for Guard {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Release);
                }
            }
            let _guard = Guard(running);
            if let Err(error) = run_outputs(&outputs, &pricing)
                && let Some(owner) = owner
            {
                let mut status = status_for(&owner);
                status.error = Some(error);
                save_status(&owner, status);
            }
        });
    }
}

pub fn run(pricing: &crate::pricing::PricingMap) -> Result<String, String> {
    run_outputs(&crate::api::fetch_cached().unwrap_or_default(), pricing)
}
fn allowed(source: &str) -> Result<bool, String> {
    Ok(crate::privacy::load().sync_history
        && settings()?
            .sources
            .iter()
            .any(|selected| selected == source))
}
pub fn link_codex() -> Result<String, String> {
    if !allowed("codex")? {
        return Err("Enable private synchronization and save the Codex source first".into());
    }
    let owner = cached_owner().ok_or("Connect this installation to Fabrials first")?;
    let proof = crate::providers::codex::identity_proof()?;
    let result = request_for_subject(
        RemoteOperation::LinkCodexSource,
        serde_json::json!({"device":crate::client_id::ensure()?,"id_token":proof}),
        &owner,
    )?;
    let account = result["account_id"]
        .as_str()
        .ok_or("Invalid identity match response")?;
    Ok(format!(
        "Verified identity match with hosted {account}. Local log totals remain separate from relay traffic."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabrials_types::consumption::ConsumptionTotal;
    use run::model_totals;

    fn total(key: &str, tokens: u64) -> ConsumptionTotal {
        ConsumptionTotal {
            key: key.into(),
            tokens,
            unknown_token_records: 0,
            known_usd: 1.0,
            partial: false,
            records: 1,
        }
    }

    #[test]
    fn model_totals_keep_the_largest_and_fold_the_rest() {
        let rows: Vec<_> = (0..40).map(|n| total(&format!("m{n:02}"), n)).collect();
        let models = model_totals(&rows);
        assert_eq!(models.len(), 32);
        assert_eq!(models[0].model, "m39");
        assert_eq!(models.last().unwrap().model, "other");
        let kept: u64 = models.iter().map(|row| row.tokens).sum();
        let source: u64 = rows.iter().map(|row| row.tokens).sum();
        assert_eq!(kept, source);
    }

    #[test]
    fn model_totals_drop_ids_the_relay_would_reject() {
        let models = model_totals(&[total("gpt-6-astra", 10), total("has space", 99)]);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model, "gpt-6-astra");
        assert_eq!(models[0].tokens, 10);
    }
}
