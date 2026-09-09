//! Opt-in, selected-source private synchronization across compatible providers.
use crate::remote_workspace::{request_for_subject, RemoteOperation};
use fabrials_model::private_sync::{PrivateEvent, PrivateObservation, PrivatePage, PrivatePush};
use fabrials_runtime::credential_journal::{Rotation, Scope};
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
) -> Result<fabrials_model::private_sync::PrivateRecentPage, String> {
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
    match std::fs::read(crate::app::config_dir().join("sync-selection.json")) {
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
        &crate::app::config_dir().join("sync-selection.json"),
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
    std::fs::read(crate::app::data_dir().join("sync-status.json"))
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
            &crate::app::data_dir().join("sync-status.json"),
            &bytes,
        );
    }
}
pub fn after_probe(outputs: &[crate::model::ProviderOutput]) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if !crate::privacy::load().sync_history
        || crate::app::env_offline()
        || !crate::share_session::is_logged_in()
    {
        return;
    }
    if RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let outputs = outputs.to_vec();
    let owner = cached_owner();
    std::thread::spawn(move || {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                RUNNING.store(false, Ordering::Release);
            }
        }
        let _guard = Guard;
        if let Err(error) = run_outputs(&outputs) {
            if let Some(owner) = owner {
                let mut status = status_for(&owner);
                status.error = Some(error);
                save_status(&owner, status);
            }
        }
    });
}

pub fn cmd(args: &[String]) -> std::process::ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("spanreed sync — synchronize selected private usage sources with Fabrials.\nSelect sources in Spanreed Settings and explicitly enable private history synchronization.\nNo provider credentials or request bodies are uploaded.");
        return std::process::ExitCode::SUCCESS;
    }
    match if args.first().is_some_and(|arg| arg == "link-codex") {
        link_codex()
    } else {
        run(true)
    } {
        Ok(message) => {
            println!("{message}");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
pub fn run(_verbose: bool) -> Result<String, String> {
    run_outputs(&crate::api::fetch_cached().unwrap_or_default())
}
fn allowed(source: &str) -> Result<bool, String> {
    Ok(crate::privacy::load().sync_history
        && settings()?
            .sources
            .iter()
            .any(|selected| selected == source))
}
fn run_outputs(outputs: &[crate::model::ProviderOutput]) -> Result<String, String> {
    if crate::app::env_offline() {
        return Err("Offline mode is enabled".into());
    }
    if !crate::privacy::load().sync_history {
        return Err("Private history synchronization is off".into());
    }
    let selection = settings()?;
    if selection.sources.is_empty() {
        return Err("Select the providers and accounts to synchronize in Settings".into());
    }
    let environment = crate::app::config_dir().to_string_lossy().into_owned();
    let _lock = Rotation::acquire(
        &crate::app::data_dir().join("credential-recovery"),
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
        {
            if let Some(identity) = crate::providers::codex::local_identity() {
                source_identities.insert("codex".into(), identity);
            }
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
        let snapshots: Vec<fabrials_model::private_sync::LocalUsageSnapshotV2> =
            serde_json::from_value(remote["snapshots"].clone())
                .map_err(|_| "Invalid synchronized consumption response")?;
        for client in fabrials_providers::usage::catalog::clients() {
            if !allowed(&client.id)? {
                continue;
            }
            let report = crate::usage::report(
                fabrials_core::usage::UsageFilter {
                    client: Some(client.id.clone()),
                    ..Default::default()
                },
                false,
            )?;
            let days: Vec<_> = report
                .daily
                .into_iter()
                .map(|day| fabrials_model::private_sync::LocalUsageDay {
                    date: day.key,
                    tokens: day.tokens,
                    estimated_usd: day.known_usd,
                })
                .collect();
            let period_totals = (!report.period_totals.is_empty()).then(|| {
                fabrials_model::private_sync::LocalUsageAggregate {
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
            let projection = serde_json::to_vec(&(report.total.partial, &days, &period_totals))
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
            let snapshot = fabrials_model::private_sync::LocalUsageSnapshotV2 {
                device: device.clone(),
                source: client.id.clone(),
                revision,
                period_totals,
                observed_at_ms: crate::util::now_ms(),
                partial: report.total.partial,
                days,
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
            if let Some(summary) = crate::cost::estimate(kind) {
                let snapshot = fabrials_model::private_sync::LocalUsageSnapshot {
                    device: device.clone(),
                    source: source.into(),
                    observed_at_ms: crate::util::now_ms(),
                    partial: summary.partial,
                    days: summary
                        .daily
                        .into_iter()
                        .map(|day| fabrials_model::private_sync::LocalUsageDay {
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
    Ok(format!("Verified identity match with hosted {account}. Local log totals remain separate from relay traffic."))
}
