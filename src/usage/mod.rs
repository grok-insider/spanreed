//! Host composition for local consumption. Quota probes do not gate imports.
pub mod connections;
pub mod discovery;
mod importer;

use fabrials_core::usage::{CostOrigin, SourceStatus, UsageCost, UsageFilter, UsageRecord};
use fabrials_runtime::local_usage::UsageStore;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::process::ExitCode;

pub use fabrials_core::usage::{ConsumptionReport as UsageReport, ConsumptionTotal as UsageTotal};

pub fn refresh(client: Option<&str>, force: bool) -> Result<Vec<SourceStatus>, String> {
    importer::refresh(client, force)
}

pub fn records(filter: &UsageFilter) -> Result<Vec<UsageRecord>, String> {
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
        if let Some((usd, rates)) = crate::pricing::table().exact_cost(model, usage) {
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

fn accumulate(total: &mut UsageTotal, record: &UsageRecord) {
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

fn group(records: &[UsageRecord], key: impl Fn(&UsageRecord) -> String) -> Vec<UsageTotal> {
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

pub fn report(mut filter: UsageFilter, force: bool) -> Result<UsageReport, String> {
    let sources = refresh(filter.client.as_deref(), force)?;
    if filter.since_ms.is_none() {
        filter.since_ms = Some(crate::util::now_ms() - 31 * 86_400_000);
    }
    let records = records(&filter)?;
    let mut total = UsageTotal {
        key: "total".into(),
        ..UsageTotal::default()
    };
    for record in &records {
        accumulate(&mut total, record);
    }
    total.partial |= sources.iter().any(|source| {
        matches!(
            source.state,
            fabrials_core::usage::ImportState::Error
                | fabrials_core::usage::ImportState::Partial
                | fabrials_core::usage::ImportState::UnsupportedFormat
        )
    });
    // Session/account summaries cannot be allocated to individual days faithfully.
    let (daily_records, period_records): (Vec<_>, Vec<_>) =
        records.iter().cloned().partition(|record| {
            record.granularity == fabrials_core::usage::Granularity::Request
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

pub fn cmd(args: &[String]) -> ExitCode {
    let run = || -> Result<serde_json::Value, String> {
        if args.first().is_some_and(|s| s == "connect") {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::io::stdin()
                .take(65537)
                .read_to_end(&mut bytes)
                .map_err(|_| "Cannot read usage credential")?;
            if bytes.len() > 65536 {
                return Err("Usage connection exceeds size limit".into());
            }
            let connection =
                serde_json::from_slice::<connections::Connection>(&bytes).map_err(|_| {
                    "Expected JSON containing client, account and credential on standard input"
                })?;
            connections::save(connection)?;
            return Ok(serde_json::json!({"saved":true}));
        }
        if args.first().is_some_and(|s| s == "disconnect") {
            connections::disconnect(args.get(1).ok_or("Missing client")?)?;
            return Ok(serde_json::json!({"disconnected":true}));
        }
        if args.first().is_some_and(|s| s == "sources") {
            return Ok(
                serde_json::json!({"clients":catalog_view(),"settings":discovery::settings()?,"connections":connections::status()?}),
            );
        }
        let mut filter = UsageFilter::default();
        let mut force = false;
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--refresh"=>force=true,
                "--json"=>{},
                "--client"=>filter.client=Some(iter.next().ok_or("Missing client")?.clone()),
                "--model"=>filter.model=Some(iter.next().ok_or("Missing model")?.clone()),
                "--account"=>filter.account=Some(iter.next().ok_or("Missing account")?.clone()),
                "--provider"=>filter.provider=Some(iter.next().ok_or("Missing provider")?.clone()),
                "--session"=>filter.session=Some(iter.next().ok_or("Missing session")?.clone()),
                "--project"=>filter.project=Some(iter.next().ok_or("Missing project")?.clone()),
                "--days"=>{
                    let days:i64=iter.next().ok_or("Missing days")?.parse().map_err(|_|"Invalid days")?;
                    if !(1..=36500).contains(&days) {return Err("Days must be between 1 and 36500".into());}
                    filter.since_ms=Some(crate::util::now_ms()-days*86_400_000);
                }
                _=>return Err("Usage: spanreed usage [sources] [--client ID] [--model ID] [--provider ID] [--session ID] [--project PATH] [--days N] [--refresh] [--json]".into()),
            }
        }
        serde_json::to_value(report(filter, force)?).map_err(|e| e.to_string())
    };
    match run() {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("usage: {error}");
            ExitCode::FAILURE
        }
    }
}

pub fn catalog_view() -> serde_json::Value {
    serde_json::Value::Array(fabrials_providers::usage::catalog::clients().iter().map(|client|serde_json::json!({
        "id":client.id,"name":client.name,"remote_collection":client.remote_collection(),"aggregate_only":client.aggregate_only()
    })).collect())
}
