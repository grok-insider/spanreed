//! Kilo credit blocks.
//!
//! Bearer `KILO_API_KEY`, or `kilo.access` from the CLI auth file.

use crate::creds;
use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;

const ID: &str = "kilo";
const NAME: &str = "Kilo";

pub struct Kilo;

fn cli_token() -> Option<String> {
    let path = creds::data_home().join("kilo/auth.json");
    creds::read_json(&path)
        .as_ref()
        .and_then(|data| data.pointer("/kilo/access"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn blocks(body: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    if let Some(rows) = body.as_array() {
        for row in rows {
            if let Some(blocks) = row
                .pointer("/result/data/json/creditBlocks")
                .or_else(|| row.pointer("/result/data/creditBlocks"))
                .and_then(|v| v.as_array())
            {
                return Some(blocks);
            }
        }
    }
    body.get("creditBlocks").and_then(|v| v.as_array())
}

pub(super) fn parse_credits(body: &serde_json::Value) -> Vec<MetricLine> {
    let Some(blocks) = blocks(body) else {
        return Vec::new();
    };
    let mut total = 0.0;
    let mut remaining = 0.0;
    let mut saw_total = false;
    let mut saw_remaining = false;
    for block in blocks {
        if let Some(amount) = field(block, "amount_mUsd") {
            total += amount / 1_000_000.0;
            saw_total = true;
        }
        if let Some(balance) = field(block, "balance_mUsd") {
            remaining += balance / 1_000_000.0;
            saw_remaining = true;
        }
    }
    if !saw_total && !saw_remaining {
        return Vec::new();
    }
    let total = if saw_total {
        total.max(0.0)
    } else {
        remaining.max(0.0)
    };
    let remaining = if saw_remaining {
        remaining.max(0.0)
    } else {
        total
    };
    let used = (total - remaining).max(0.0);
    vec![MetricLine::dollars(
        MetricKind::Quota,
        "Credits",
        used,
        total.max(used),
        None,
    )]
}

impl Provider for Kilo {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["KILO_API_KEY"]).is_some() || cli_token().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["KILO_API_KEY"]).or_else(cli_token) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Kilo token found. Set KILO_API_KEY or run `kilo auth login`.",
            );
        };
        let input =
            json_api::query_escape(r#"{"0":{"json":null},"1":{"json":null},"2":{"json":null}}"#);
        let url = format!(
            "https://app.kilo.ai/api/trpc/user.getCreditBlocks,kiloPass.getState,user.getAutoTopUpPaymentMethod?batch=1&input={input}"
        );
        match json_api::get_bearer(&url, &key) {
            Ok(data) => {
                let lines = parse_credits(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "Kilo response had no credit blocks.")
                } else {
                    ProviderOutput::new(ID, NAME, lines)
                }
            }
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_micro_usd_blocks() {
        let lines = parse_credits(&serde_json::json!([
            {"result": {"data": {"json": {"creditBlocks": [
                {"amount_mUsd": 2_000_000, "balance_mUsd": 500_000}
            ]}}}}
        ]));
        assert_eq!(lines.len(), 1);
    }
}
