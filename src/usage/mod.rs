//! Host composition for local consumption. Quota probes do not gate imports.
pub mod connections;
pub mod discovery;
mod importer;

use fabrials_runtime::local_usage::UsageStore;
use fabrials_types::consumption::{
    ConsumptionRecord, CostOrigin, SourceStatus, UsageCost, UsageFilter,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub use fabrials_types::consumption::{
    ConsumptionReport as UsageReport, ConsumptionTotal as UsageTotal,
};

pub fn refresh(client: Option<&str>, force: bool) -> Result<Vec<SourceStatus>, String> {
    importer::refresh(client, force)
}

pub fn records(
    filter: &UsageFilter,
    pricing: &crate::pricing::PricingMap,
) -> Result<Vec<ConsumptionRecord>, String> {
    let store = UsageStore::open(&crate::history::history_path())?;
    let mut records = store.records(filter)?;
    for record in &mut records {
        if record.cost.is_some() {
            continue;
        }
        let (Some(model), Some(tokens)) = (&record.model, &record.tokens) else {
            continue;
        };
        // Do not guess a Fast multiplier when the rate is not known.
        if record
            .service_tier
            .as_deref()
            .is_some_and(|tier| matches!(tier, "priority" | "fast"))
        {
            continue;
        }
        let usage = crate::pricing::Usage {
            input: tokens
                .input
                .saturating_sub(tokens.cache_read)
                .saturating_sub(tokens.cache_write),
            output: tokens.output,
            cache_create: tokens.cache_write,
            cache_read: tokens.cache_read,
        };
        if let Some((usd, rates)) = pricing.exact_cost(model, usage) {
            let price_identity = format!("{model}:{rates:?}");
            record.cost = Some(UsageCost {
                usd,
                origin: CostOrigin::Estimated,
                pricing_revision: Some(format!("{:x}", Sha256::digest(price_identity.as_bytes()))),
            });
        }
    }
    Ok(records)
}

fn accumulate(total: &mut UsageTotal, record: &ConsumptionRecord) {
    total.records += 1;
    if record.tokens.is_none() {
        total.unknown_token_records += 1;
    }
    total.tokens = total
        .tokens
        .saturating_add(record.tokens.as_ref().map_or(0, |t| t.total()));
    if let Some(cost) = &record.cost {
        total.known_usd += cost.usd;
    } else {
        total.partial = true;
    }
}

fn group(
    records: &[ConsumptionRecord],
    key: impl Fn(&ConsumptionRecord) -> String,
) -> Vec<UsageTotal> {
    let mut groups = BTreeMap::<String, UsageTotal>::new();
    for record in records {
        let key = key(record);
        accumulate(
            groups.entry(key.clone()).or_insert_with(|| UsageTotal {
                key,
                ..UsageTotal::default()
            }),
            record,
        );
    }
    groups.into_values().collect()
}

pub fn report(
    mut filter: UsageFilter,
    force: bool,
    pricing: &crate::pricing::PricingMap,
) -> Result<UsageReport, String> {
    let sources = refresh(filter.client.as_deref(), force)?;
    if filter.since_ms.is_none() {
        filter.since_ms = Some(crate::util::now_ms() - 31 * 86_400_000);
    }
    let records = records(&filter, pricing)?;
    let mut total = UsageTotal {
        key: "total".into(),
        ..UsageTotal::default()
    };
    for record in &records {
        accumulate(&mut total, record);
    }
    let explicitly_selected = filter.client.is_some();
    total.partial |= sources.iter().any(|source| {
        matches!(
            source.state,
            fabrials_types::consumption::ImportState::Error
                | fabrials_types::consumption::ImportState::Partial
                | fabrials_types::consumption::ImportState::UnsupportedFormat
        ) || (explicitly_selected
            && matches!(
                source.state,
                fabrials_types::consumption::ImportState::NoData
                    | fabrials_types::consumption::ImportState::NeedsConnection
            ))
    });
    // Session/account summaries cannot be allocated to individual days faithfully.
    let (daily_records, period_records): (Vec<_>, Vec<_>) =
        records.iter().cloned().partition(|record| {
            record.granularity == fabrials_types::consumption::Granularity::Request
                && !record.timestamp_inferred
        });
    let unknown = || "Unknown".to_string();
    let report = UsageReport {
        revision: UsageStore::open(&crate::history::history_path())?.revision()?,
        sources,
        total,
        daily: group(&daily_records, |r| crate::util::local_date_ymd(r.at_ms)),
        period_totals: group(&period_records, |r| {
            format!(
                "{} · {}",
                r.client,
                r.session.as_deref().unwrap_or("Account period")
            )
        }),
        clients: group(&records, |r| r.client.clone()),
        models: group(&records, |r| r.model.clone().unwrap_or_else(unknown)),
        sessions: group(&records, |r| r.session.clone().unwrap_or_else(unknown)),
        projects: group(&records, |r| r.project.clone().unwrap_or_else(unknown)),
        records_truncated: records.len() > 500,
        records: records.into_iter().rev().take(500).collect(),
    };
    Ok(report)
}

pub fn catalog_view() -> serde_json::Value {
    serde_json::Value::Array(fabrials_providers::usage::catalog::clients().iter().map(|client|serde_json::json!({
        "id":client.id,"name":client.name,"remote_collection":client.remote_collection(),"aggregate_only":client.aggregate_only()
    })).collect())
}
