//! One synchronization run: enqueue fresh observations, push pending ones,
//! pull remote history, then publish local consumption snapshots.

use fabrials_fabric::ports::Scope;
use fabrials_store_sqlite::credential_journal::Rotation;
use fabrials_types::private_sync::{
    LocalUsageAggregate, LocalUsageDay, LocalUsageSnapshot, LocalUsageSnapshotV2, PrivateEvent,
    PrivateObservation, PrivatePage, PrivatePush,
};
use serde_json::Value;

use super::{SyncSettings, SyncStatus, allowed, save_status, settings};
use crate::model::ProviderOutput;
use crate::pricing::PricingMap;
use crate::remote_workspace::{RemoteOperation, request_for_subject};

/// State shared by the steps of one run.
struct Run<'a> {
    owner: String,
    device: String,
    capabilities: Result<Value, String>,
    store: crate::sync_store::Store,
    pricing: &'a PricingMap,
}

impl Run<'_> {
    fn supports(&self, capability: &str) -> bool {
        self.capabilities
            .as_ref()
            .is_ok_and(|value| value[capability] == true)
    }

    fn request(&self, operation: RemoteOperation, body: Value) -> Result<Value, String> {
        request_for_subject(operation, body, &self.owner)
    }
}

pub(super) fn run_outputs(
    outputs: &[ProviderOutput],
    pricing: &PricingMap,
) -> Result<String, String> {
    let selection = preflight()?;
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
    enqueue(&mut store, &owner, &selection, outputs)?;
    while store.scan_page(&owner, &selection.sources)? {
        ensure_enabled()?;
    }
    let device = crate::client_id::ensure()?;
    let capabilities = request_for_subject(
        RemoteOperation::SyncCapabilities,
        serde_json::json!({}),
        &owner,
    );
    let mut run = Run {
        owner,
        device,
        capabilities,
        store,
        pricing,
    };
    if run.supports("installation_hostname_v1") {
        remember_installation_label(&run.owner, &run.device)?;
    }
    let uploaded = push(&mut run)?;
    let downloaded = pull(&mut run)?;
    if run.supports("local_usage_snapshot_v2") {
        publish_usage_v2(&run)?;
    } else if run.supports("local_usage_snapshot_v1") {
        publish_usage_v1(&run)?;
    }
    save_status(
        &run.owner,
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

/// Offline mode, consent and a source selection are required.
fn preflight() -> Result<SyncSettings, String> {
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
    Ok(selection)
}

/// Consent can be withdrawn while a run is in progress.
fn ensure_enabled() -> Result<(), String> {
    if crate::privacy::load().sync_history {
        Ok(())
    } else {
        Err("Private synchronization was disabled".into())
    }
}

fn enqueue(
    store: &mut crate::sync_store::Store,
    owner: &str,
    selection: &SyncSettings,
    outputs: &[ProviderOutput],
) -> Result<(), String> {
    let now = crate::util::now_ms();
    for output in outputs {
        if selection.sources.contains(&output.provider_id) {
            store.enqueue(
                owner,
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
    Ok(())
}

/// Push pending observations batch by batch; returns how many were accepted.
fn push(run: &mut Run<'_>) -> Result<usize, String> {
    let mut uploaded = 0;
    loop {
        ensure_enabled()?;
        let selection = settings()?;
        let batch = run.store.pending(&run.owner, &selection.sources)?;
        if batch.is_empty() {
            return Ok(uploaded);
        }
        if !batch
            .iter()
            .all(|(_, item)| allowed(&item.source).unwrap_or(false))
        {
            continue;
        }
        let mut source_identities = std::collections::BTreeMap::new();
        if run.supports("source_identity_v1")
            && allowed("codex")?
            && let Some(identity) = crate::providers::codex::local_identity()
        {
            source_identities.insert("codex".into(), identity);
        }
        let push = PrivatePush {
            source_identities,
            device: run.device.clone(),
            observations: batch.iter().map(|(_, item)| item.clone()).collect(),
        };
        let response = run.request(
            RemoteOperation::PushSync,
            serde_json::to_value(push).map_err(|_| "Invalid sync batch")?,
        )?;
        if response["accepted"].as_u64() != Some(batch.len() as u64) {
            return Err("ai-relay did not acknowledge the entire sync batch".into());
        }
        run.store.acknowledge(
            &run.owner,
            &batch.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
        )?;
        uploaded += batch.len();
    }
}

/// Import remote history pages; returns how many observations arrived.
fn pull(run: &mut Run<'_>) -> Result<usize, String> {
    let mut downloaded = 0;
    loop {
        ensure_enabled()?;
        let value = run.request(
            RemoteOperation::PullSync,
            serde_json::json!({"cursor": run.store.cursor(&run.owner)?}),
        )?;
        let page: PrivatePage =
            serde_json::from_value(value).map_err(|_| "Invalid private history response")?;
        downloaded += page.observations.len();
        run.store.import(&run.owner, &page)?;
        if !page.has_more {
            return Ok(downloaded);
        }
    }
}

/// Per-client consumption snapshots with revisions (relay v2).
fn publish_usage_v2(run: &Run<'_>) -> Result<(), String> {
    let remote = run.request(RemoteOperation::Consumption, serde_json::json!({}))?;
    let snapshots: Vec<LocalUsageSnapshotV2> = serde_json::from_value(remote["snapshots"].clone())
        .map_err(|_| "Invalid synchronized consumption response")?;
    for client in fabrials_usage_import::catalog::clients() {
        if !allowed(&client.id)? {
            continue;
        }
        let snapshot = usage_snapshot_v2(run, &client.id, &snapshots)?;
        if !allowed(&client.id)? {
            return Err("Private synchronization selection changed".into());
        }
        let response = run.request(
            RemoteOperation::PutLocalUsageV2,
            serde_json::to_value(snapshot).map_err(|_| "Invalid local usage")?,
        )?;
        if response["accepted"] != true {
            return Err("Local usage revision was not acknowledged".into());
        }
    }
    Ok(())
}

fn usage_snapshot_v2(
    run: &Run<'_>,
    client: &str,
    published: &[LocalUsageSnapshotV2],
) -> Result<LocalUsageSnapshotV2, String> {
    let report = crate::usage::report(
        fabrials_types::consumption::UsageFilter {
            client: Some(client.to_string()),
            ..Default::default()
        },
        false,
        run.pricing,
    )?;
    let days: Vec<_> = report
        .daily
        .into_iter()
        .map(|day| LocalUsageDay {
            date: day.key,
            tokens: day.tokens,
            estimated_usd: day.known_usd,
        })
        .collect();
    let period_totals = period_totals(&report.period_totals);
    let send_models = run.supports("local_usage_models_v1");
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
    let last_published = published
        .iter()
        .filter(|s| s.device == run.device && s.source == client)
        .map(|s| s.revision)
        .max()
        .unwrap_or(0);
    let revision = fabrials_store_sqlite::SqliteUsageStore::open(&crate::history::history_path())?
        .export_revision_after(
            &format!("{}:{}:{}", run.owner, run.device, client),
            &digest,
            last_published,
        )?;
    Ok(LocalUsageSnapshotV2 {
        device: run.device.clone(),
        source: client.to_string(),
        revision,
        period_totals,
        observed_at_ms: crate::util::now_ms(),
        partial: report.total.partial,
        days,
        models: if send_models { models } else { Vec::new() },
    })
}

fn period_totals(
    rows: &[fabrials_types::consumption::ConsumptionTotal],
) -> Option<LocalUsageAggregate> {
    (!rows.is_empty()).then(|| LocalUsageAggregate {
        tokens: rows
            .iter()
            .all(|row| row.unknown_token_records == 0)
            .then(|| {
                rows.iter()
                    .fold(0u64, |sum, row| sum.saturating_add(row.tokens))
            }),
        known_usd: rows.iter().map(|row| row.known_usd).sum(),
        partial: rows.iter().any(|row| row.partial),
    })
}

/// Daily Claude/Codex cost snapshots (relay v1).
fn publish_usage_v1(run: &Run<'_>) -> Result<(), String> {
    for (source, kind) in [
        ("codex", crate::cost::Source::Codex),
        ("claude", crate::cost::Source::Claude),
    ] {
        if !allowed(source)? {
            continue;
        }
        let Some(summary) = crate::cost::estimate(kind, run.pricing) else {
            continue;
        };
        let snapshot = LocalUsageSnapshot {
            device: run.device.clone(),
            source: source.into(),
            observed_at_ms: crate::util::now_ms(),
            partial: summary.partial,
            days: summary
                .daily
                .into_iter()
                .map(|day| LocalUsageDay {
                    date: day.date,
                    tokens: day.tokens,
                    estimated_usd: day.cost,
                })
                .collect(),
        };
        if !allowed(source)? {
            return Err("Private synchronization selection changed".into());
        }
        let response = run.request(
            RemoteOperation::PutLocalUsage,
            serde_json::to_value(snapshot).map_err(|_| "Invalid local usage")?,
        )?;
        if response["accepted"] != true {
            return Err("Local usage was not acknowledged".into());
        }
    }
    Ok(())
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

/// At most this many model rows travel with one snapshot. The rest fold into
/// `other` so a noisy client cannot blow the sync body limit.
pub(super) fn model_totals(
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
