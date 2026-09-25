//! Opt-in, selected-source private synchronization across compatible providers.
use crate::remote_workspace::{RemoteOperation, request_for_subject};
use fabrials_runtime::credential_journal::{Rotation, Scope};
use fabrials_types::private_sync::{PrivateEvent, PrivateObservation, PrivatePage, PrivatePush};
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

fn remember_installation_label(owner: &str, device: &str) -> Result<(), String> {
    let Some(hostname) = crate::machine_name::profile_label() else {
        return Ok(());
    };
    if hostname == device {
        return Ok(());
    }
    let response = request_for_subject(
        RemoteOperation::SetInstallationHostname,
        serde_json::json!({ "device": device, "hostname": hostname }),
        owner,
    )?;
    if response["accepted"] != true {
        return Err("Could not save this machine name".into());
    }
    Ok(())
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
fn run_outputs(
    outputs: &[crate::model::ProviderOutput],
    pricing: &crate::pricing::PricingMap,
) -> Result<String, String> {
    if crate::product::env_offline() {
        return Err("Offline mode is enabled".into());
    }
    if !crate::privacy::load().sync_history {
        return Err("Private history synchronization is off".into());
    }
    let selection = settings()?;
    if selection.sources.is_empty() {
        return Err("Select the providers and accounts to synchronize in Settings".into());
    }
    let environment = crate::product::config_dir().to_string_lossy().into_owned();
    let _lock = Rotation::acquire(
        &crate::product::data_dir().join("credential-recovery"),
        Scope {
            environment: &environment,
            owner: "local",
            provider: "private-sync",
            alias: "worker",
        },
    )?;
    let owner = match crate::fabrials_login::status()? {
        crate::fabrials_login::LinkView::Linked { user } => user.id,
        _ => return Err("Connect this installation to Fabrials first".into()),
    };
    let mut store = crate::sync_store::Store::open()?;
    let now = crate::util::now_ms();
    for output in outputs {
        if selection.sources.contains(&output.provider_id) {
            store.enqueue(
                &owner,
                &PrivateObservation {
                    source: output.provider_id.clone(),
                    event: PrivateEvent::Quota {
                        at_ms: now,
                        output: output.clone(),
                    },
                },
            )?;
        }
    }
    while store.scan_page(&owner, &selection.sources)? {
        if !crate::privacy::load().sync_history {
            return Err("Private synchronization was disabled".into());
        }
    }
    let device = crate::client_id::ensure()?;
    let capabilities = request_for_subject(
        RemoteOperation::SyncCapabilities,
        serde_json::json!({}),
        &owner,
    );
    if let Ok(value) = capabilities.as_ref()
        && value["installation_hostname_v1"] == true
    {
        remember_installation_label(&owner, &device)?;
    }
    let mut uploaded = 0;
    let mut downloaded = 0;
    loop {
        if !crate::privacy::load().sync_history {
            return Err("Private synchronization was disabled".into());
        }
        let selection = settings()?;
        let batch = store.pending(&owner, &selection.sources)?;
        if batch.is_empty() {
            break;
        }
        if !batch
            .iter()
            .all(|(_, item)| allowed(&item.source).unwrap_or(false))
        {
            continue;
        }
        let mut source_identities = std::collections::BTreeMap::new();
        if capabilities
            .as_ref()
            .is_ok_and(|value| value["source_identity_v1"] == true)
            && allowed("codex")?
            && let Some(identity) = crate::providers::codex::local_identity()
        {
            source_identities.insert("codex".into(), identity);
        }
        let push = PrivatePush {
            source_identities,
            device: device.clone(),
            observations: batch.iter().map(|(_, item)| item.clone()).collect(),
        };
        let response = request_for_subject(
            RemoteOperation::PushSync,
            serde_json::to_value(push).map_err(|_| "Invalid sync batch")?,
            &owner,
        )?;
        if response["accepted"].as_u64() != Some(batch.len() as u64) {
            return Err("ai-relay did not acknowledge the entire sync batch".into());
        }
        store.acknowledge(
            &owner,
            &batch.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
        )?;
        uploaded += batch.len();
    }
    loop {
        if !crate::privacy::load().sync_history {
            return Err("Private synchronization was disabled".into());
        }
        let value = request_for_subject(
            RemoteOperation::PullSync,
            serde_json::json!({"cursor":store.cursor(&owner)?}),
            &owner,
        )?;
        let page: PrivatePage =
            serde_json::from_value(value).map_err(|_| "Invalid private history response")?;
        downloaded += page.observations.len();
        store.import(&owner, &page)?;
        if !page.has_more {
            break;
        }
    }
    if capabilities
        .as_ref()
        .is_ok_and(|value| value["local_usage_snapshot_v2"] == true)
    {
        let remote =
            request_for_subject(RemoteOperation::Consumption, serde_json::json!({}), &owner)?;
        let snapshots: Vec<fabrials_types::private_sync::LocalUsageSnapshotV2> =
            serde_json::from_value(remote["snapshots"].clone())
                .map_err(|_| "Invalid synchronized consumption response")?;
        for client in fabrials_providers::usage::catalog::clients() {
            if !allowed(&client.id)? {
                continue;
            }
            let report = crate::usage::report(
                fabrials_types::consumption::UsageFilter {
                    client: Some(client.id.clone()),
                    ..Default::default()
                },
                false,
                pricing,
            )?;
            let days: Vec<_> = report
                .daily
                .into_iter()
                .map(|day| fabrials_types::private_sync::LocalUsageDay {
                    date: day.key,
                    tokens: day.tokens,
                    estimated_usd: day.known_usd,
                })
                .collect();
            let period_totals = (!report.period_totals.is_empty()).then(|| {
                fabrials_types::private_sync::LocalUsageAggregate {
                    tokens: report
                        .period_totals
                        .iter()
                        .all(|row| row.unknown_token_records == 0)
                        .then(|| {
                            report
                                .period_totals
                                .iter()
                                .fold(0u64, |sum, row| sum.saturating_add(row.tokens))
                        }),
                    known_usd: report.period_totals.iter().map(|row| row.known_usd).sum(),
                    partial: report.period_totals.iter().any(|row| row.partial),
                }
            });
            let send_models = capabilities
                .as_ref()
                .is_ok_and(|value| value["local_usage_models_v1"] == true);
            let models = model_totals(&report.models);
            let projection = serde_json::to_vec(&(
                report.total.partial,
                &days,
                &period_totals,
                &models,
                send_models,
            ))
            .map_err(|_| "Invalid usage projection")?;
            use sha2::{Digest, Sha256};
            let digest = format!("{:x}", Sha256::digest(projection));
            let revision =
                fabrials_runtime::local_usage::UsageStore::open(&crate::history::history_path())?
                    .export_revision_after(
                    &format!("{}:{}:{}", owner, device, client.id),
                    &digest,
                    snapshots
                        .iter()
                        .filter(|s| s.device == device && s.source == client.id)
                        .map(|s| s.revision)
                        .max()
                        .unwrap_or(0),
                )?;
            let snapshot = fabrials_types::private_sync::LocalUsageSnapshotV2 {
                device: device.clone(),
                source: client.id.clone(),
                revision,
                period_totals,
                observed_at_ms: crate::util::now_ms(),
                partial: report.total.partial,
                days,
                models: if send_models { models } else { Vec::new() },
            };
            if !allowed(&client.id)? {
                return Err("Private synchronization selection changed".into());
            }
            let response = request_for_subject(
                RemoteOperation::PutLocalUsageV2,
                serde_json::to_value(snapshot).map_err(|_| "Invalid local usage")?,
                &owner,
            )?;
            if response["accepted"] != true {
                return Err("Local usage revision was not acknowledged".into());
            }
        }
    } else if capabilities
        .as_ref()
        .is_ok_and(|value| value["local_usage_snapshot_v1"] == true)
    {
        for (source, kind) in [
            ("codex", crate::cost::Source::Codex),
            ("claude", crate::cost::Source::Claude),
        ] {
            if !allowed(source)? {
                continue;
            }
            if let Some(summary) = crate::cost::estimate(kind, pricing) {
                let snapshot = fabrials_types::private_sync::LocalUsageSnapshot {
                    device: device.clone(),
                    source: source.into(),
                    observed_at_ms: crate::util::now_ms(),
                    partial: summary.partial,
                    days: summary
                        .daily
                        .into_iter()
                        .map(|day| fabrials_types::private_sync::LocalUsageDay {
                            date: day.date,
                            tokens: day.tokens,
                            estimated_usd: day.cost,
                        })
                        .collect(),
                };
                if !allowed(source)? {
                    return Err("Private synchronization selection changed".into());
                }
                let response = request_for_subject(
                    RemoteOperation::PutLocalUsage,
                    serde_json::to_value(snapshot).map_err(|_| "Invalid local usage")?,
                    &owner,
                )?;
                if response["accepted"] != true {
                    return Err("Local usage was not acknowledged".into());
                }
            }
        }
    }
    save_status(
        &owner,
        SyncStatus {
            last_success_ms: Some(crate::util::now_ms()),
            uploaded,
            downloaded,
            error: None,
        },
    );
    Ok(format!(
        "Synchronized {uploaded} private observations; downloaded {downloaded}."
    ))
}

/// At most this many model rows travel with one snapshot. The rest fold into
/// `other` so a noisy client cannot blow the sync body limit.
fn model_totals(
    models: &[fabrials_types::consumption::ConsumptionTotal],
) -> Vec<fabrials_types::private_sync::LocalUsageModel> {
    const LIMIT: usize = 32;
    let mut rows: Vec<_> = models
        .iter()
        .filter(|row| model_id(&row.key))
        .map(|row| fabrials_types::private_sync::LocalUsageModel {
            model: row.key.clone(),
            requests: row.records,
            tokens: row.tokens,
            estimated_usd: row.known_usd,
        })
        .filter(|row| {
            row.estimated_usd.is_finite()
                && (0.0..=1_000_000.0).contains(&row.estimated_usd)
                && row.requests <= 1_000_000_000
                && row.tokens <= 1_000_000_000_000
        })
        .collect();
    rows.sort_by(|left, right| {
        right
            .tokens
            .cmp(&left.tokens)
            .then_with(|| left.model.cmp(&right.model))
    });
    if rows.len() <= LIMIT {
        return rows;
    }
    let rest = rows.split_off(LIMIT - 1);
    let mut folded = fabrials_types::private_sync::LocalUsageModel {
        model: "other".into(),
        requests: 0,
        tokens: 0,
        estimated_usd: 0.0,
    };
    for row in rest {
        folded.requests = folded.requests.saturating_add(row.requests);
        folded.tokens = folded.tokens.saturating_add(row.tokens);
        folded.estimated_usd += row.estimated_usd;
    }
    if let Some(existing) = rows.iter_mut().find(|row| row.model == "other") {
        existing.requests = existing.requests.saturating_add(folded.requests);
        existing.tokens = existing.tokens.saturating_add(folded.tokens);
        existing.estimated_usd += folded.estimated_usd;
    } else if folded.requests > 0 || folded.tokens > 0 {
        rows.push(folded);
    }
    rows
}

fn model_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_/:.".contains(&byte))
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
