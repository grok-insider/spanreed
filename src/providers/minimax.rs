//! MiniMax (Token Plan remains) provider.
//!
//! Auth: API key by region.
//!   CN:     MINIMAX_CN_API_KEY -> MINIMAX_API_KEY -> MINIMAX_API_TOKEN
//!   GLOBAL: MINIMAX_API_KEY -> MINIMAX_API_TOKEN
//! Region auto-select: if MINIMAX_CN_API_KEY is set, try CN first else GLOBAL.
//! Usage: `GET /v1/token_plan/remains` (host differs by region).

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProgressFormat, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "minimax";
const NAME: &str = "MiniMax";
const GLOBAL_URL: &str = "https://www.minimax.io/v1/token_plan/remains";
const CN_URL: &str = "https://api.minimaxi.com/v1/token_plan/remains";

pub struct MiniMax;

#[derive(Clone, Copy)]
enum Region {
    Global,
    Cn,
}

impl Region {
    fn url(self) -> &'static str {
        match self {
            Region::Global => GLOBAL_URL,
            Region::Cn => CN_URL,
        }
    }
    fn suffix(self) -> &'static str {
        match self {
            Region::Global => " (GLOBAL)",
            Region::Cn => " (CN)",
        }
    }
    fn key(self) -> Option<String> {
        match self {
            Region::Cn => creds::env("MINIMAX_CN_API_KEY")
                .or_else(|| creds::env("MINIMAX_API_KEY"))
                .or_else(|| creds::env("MINIMAX_API_TOKEN")),
            Region::Global => {
                creds::env("MINIMAX_API_KEY").or_else(|| creds::env("MINIMAX_API_TOKEN"))
            }
        }
    }
}

fn region_order() -> [Region; 2] {
    if creds::env("MINIMAX_CN_API_KEY").is_some() {
        [Region::Cn, Region::Global]
    } else {
        [Region::Global, Region::Cn]
    }
}

fn any_key() -> bool {
    creds::env("MINIMAX_API_KEY").is_some()
        || creds::env("MINIMAX_CN_API_KEY").is_some()
        || creds::env("MINIMAX_API_TOKEN").is_some()
}

fn num(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64())
}

fn try_region(region: Region) -> Option<Result<ProviderOutput, String>> {
    let key = region.key()?;
    let resp = match Request::get(region.url())
        .bearer(&key)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .send()
    {
        Ok(r) => r,
        Err(_) => return Some(Err("Request failed. Check your connection.".into())),
    };
    if resp.is_auth_error() {
        return Some(Err("Session expired. Check your MiniMax API key.".into()));
    }
    if !(200..300).contains(&resp.status) {
        return Some(Err(format!(
            "Request failed (HTTP {}). Try again later.",
            resp.status
        )));
    }
    let data = match resp.json() {
        Some(d) => d,
        None => return Some(Err("Could not parse usage data.".into())),
    };

    // base_resp status check
    if let Some(code) = data
        .get("base_resp")
        .and_then(|b| b.get("status_code"))
        .and_then(|v| v.as_i64())
    {
        if code != 0 {
            let msg = data
                .get("base_resp")
                .and_then(|b| b.get("status_msg"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            // Treat auth-like errors so the next region is tried.
            if msg.to_lowercase().contains("auth") || msg.to_lowercase().contains("key") {
                return None;
            }
            return Some(Err(format!("MiniMax API error: {msg}")));
        }
    }

    let model = data
        .get("model_remains")
        .and_then(|m| m.as_array())
        .and_then(|arr| arr.first());
    let model = match model {
        Some(m) => m,
        None => return None, // try next region
    };

    let total = num(model.get("current_interval_total_count"));
    let used_count = num(model.get("current_interval_usage_count"));
    let resets = model
        .get("end_time")
        .and_then(util::to_iso)
        .or_else(|| model.get("remains_time").and_then(util::to_iso));

    let plan_base = model
        .get("current_subscribe_title")
        .or_else(|| model.get("plan_name"))
        .or_else(|| model.get("plan"))
        .and_then(|v| v.as_str())
        .map(util::plan_label);
    let plan = plan_base.map(|p| format!("{p}{}", region.suffix()));

    let line = match (total, used_count) {
        (Some(total), Some(used)) if total > 0.0 => MetricLine::Progress {
            label: "Session".into(),
            used,
            limit: total,
            format: ProgressFormat::Count {
                suffix: "prompts".into(),
            },
            resets_at: resets,
            color: None,
        },
        _ => {
            // Fall back to remaining percent.
            let rem_pct = num(model.get("current_interval_remaining_percent"))?;
            MetricLine::percent("Session", 100.0 - rem_pct, resets)
        }
    };

    Some(Ok(ProviderOutput::new(ID, NAME, vec![line]).with_plan(plan)))
}

impl Provider for MiniMax {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        any_key()
    }

    fn probe(&self) -> ProviderOutput {
        if !any_key() {
            return ProviderOutput::error(
                ID,
                NAME,
                "MiniMax API key missing. Set MINIMAX_API_KEY or MINIMAX_CN_API_KEY.",
            );
        }
        let mut last_err: Option<String> = None;
        for region in region_order() {
            match try_region(region) {
                Some(Ok(out)) => return out,
                Some(Err(e)) => last_err = Some(e),
                None => continue,
            }
        }
        ProviderOutput::error(ID, NAME, last_err.unwrap_or_else(|| "Could not parse usage data.".into()))
    }
}
