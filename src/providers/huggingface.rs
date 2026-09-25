//! Hugging Face Inference Providers charges and ZeroGPU quota.
//!
//! Token order: `HF_TOKEN`, `HUGGING_FACE_HUB_TOKEN`, then the CLI token file.
//! Browser credit wallets are not imported.

use crate::creds;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;
use crate::util;

const ID: &str = "huggingface";
const NAME: &str = "Hugging Face";

pub struct HuggingFace;

fn token() -> Option<String> {
    if let Some(token) = env_any(&["HF_TOKEN", "HUGGING_FACE_HUB_TOKEN"]) {
        return Some(token);
    }
    let path = env_any(&["HF_TOKEN_PATH"])
        .map(std::path::PathBuf::from)
        .or_else(|| env_any(&["HF_HOME"]).map(|home| std::path::PathBuf::from(home).join("token")))
        .unwrap_or_else(|| creds::cache_home().join("huggingface/token"));
    creds::read_file(&path)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

pub(super) fn parse_whoami(body: &serde_json::Value) -> Option<String> {
    match body.get("isPro").and_then(|v| v.as_bool()) {
        Some(true) => Some("PRO".into()),
        Some(false) => Some("Free".into()),
        None => None,
    }
}

pub(super) fn parse_billing(body: &serde_json::Value) -> Vec<MetricLine> {
    let Some(inference) = body.pointer("/inferenceProviders") else {
        return Vec::new();
    };
    let gross = field(inference, "usedNanoUsd").unwrap_or(0.0) / 1e9;
    let included = field(inference, "includedNanoUsd").unwrap_or(0.0) / 1e9;
    let billable = (gross - included).max(0.0);
    let mut lines = vec![json_api::text_line("This month", json_api::usd(billable))];
    if let Some(limit) = field(inference, "limitNanoUsd") {
        if limit > 0.0 {
            lines.push(json_api::text_line("Limit", json_api::usd(limit / 1e9)));
        }
    }
    if let Some(requests) = field(inference, "numRequests") {
        lines.push(json_api::text_line("Requests", format!("{requests:.0}")));
    }
    lines
}

pub(super) fn parse_zerogpu(body: &serde_json::Value) -> Option<MetricLine> {
    let total = field(body, "base")?;
    let remaining = field(body, "current")?;
    let used = (total - remaining).max(0.0);
    let pct = json_api::used_percent(used, total)?;
    let resets = body.get("resetsAt").and_then(util::to_iso);
    Some(MetricLine::percent("ZeroGPU", pct, resets))
}

impl Provider for HuggingFace {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        token().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(token) = token() else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Hugging Face token found. Set HF_TOKEN or run `hf auth login`.",
            );
        };
        let end = json_api::utc_ymd(0);
        let start = format!("{}-01", &end[..7]);
        let url = format!(
            "https://huggingface.co/api/settings/billing/usage-v2?startDate={start}&endDate={end}"
        );
        let billing = match json_api::get_bearer(&url, &token) {
            Ok(billing) => billing,
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        let mut lines = parse_billing(&billing);
        let mut plan = None;
        if let Ok(who) = json_api::get_bearer("https://huggingface.co/api/whoami-v2", &token) {
            plan = parse_whoami(&who);
        }
        if let Ok(gpu) =
            json_api::get_bearer("https://huggingface.co/api/spaces/zero-gpu/quota", &token)
        {
            if let Some(line) = parse_zerogpu(&gpu) {
                lines.push(line);
            }
        }
        if lines.is_empty() {
            ProviderOutput::error(
                ID,
                NAME,
                "Hugging Face billing response had no inference charges.",
            )
        } else {
            ProviderOutput::new(ID, NAME, lines).with_plan(plan)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtracts_included_nano_usd() {
        let lines = parse_billing(&serde_json::json!({
            "inferenceProviders": {
                "usedNanoUsd": 2_500_000_000u64,
                "includedNanoUsd": 1_000_000_000u64,
                "limitNanoUsd": 5_000_000_000u64,
                "numRequests": 3
            }
        }));
        assert!(lines.len() >= 2);
        assert_eq!(
            parse_whoami(&serde_json::json!({"name": "ada", "isPro": true})).as_deref(),
            Some("PRO")
        );
    }
}
