//! `spanreed usage [sources|connect|disconnect] [filters]`: local consumption.

use std::process::ExitCode;

use crate::app::usage::{self, connections};
use crate::app::{self, AppContext};
use fabrials_types::consumption::UsageFilter;

pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
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
            let connection = serde_json::from_slice::<connections::Connection>(&bytes).map_err(
                |_| "Expected JSON containing client, account and credential on standard input",
            )?;
            connections::save(connection)?;
            return Ok(serde_json::json!({"saved":true}));
        }
        if args.first().is_some_and(|s| s == "disconnect") {
            connections::disconnect(args.get(1).ok_or("Missing client")?)?;
            return Ok(serde_json::json!({"disconnected":true}));
        }
        if args.first().is_some_and(|s| s == "sources") {
            return usage::sources();
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
                    filter.since_ms=Some(app::usage::now_ms()-days*86_400_000);
                }
                _=>return Err("Usage: spanreed usage [sources] [--client ID] [--model ID] [--provider ID] [--session ID] [--project PATH] [--days N] [--refresh] [--json]".into()),
            }
        }
        serde_json::to_value(usage::report(ctx, filter, force)?).map_err(|e| e.to_string())
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
