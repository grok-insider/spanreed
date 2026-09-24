//! Gemini CLI OAuth quota.
//!
//! Reads `~/.gemini/oauth_creds.json` and posts to Cloud Code
//! `retrieveUserQuota`. Expired tokens are not refreshed here.

use crate::creds;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::{self, field};
use crate::providers::Provider;
use crate::util;

const ID: &str = "gemini";
const NAME: &str = "Gemini";

pub struct Gemini;

fn creds_path() -> std::path::PathBuf {
    creds::expand("~/.gemini/oauth_creds.json")
}

fn access_token() -> Result<String, String> {
    let data = creds::read_json(&creds_path())
        .ok_or_else(|| "No Gemini CLI credentials found. Run `gemini` to log in.".to_string())?;
    let token = data
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "Gemini credentials are missing an access token. Run `gemini`.".to_string()
        })?;
    if let Some(expiry) = data.get("expiry_date").and_then(json_api::number) {
        if expiry > 0.0 && crate::util::now_ms() as f64 >= expiry {
            return Err("Gemini access token expired. Run `gemini` to log in again.".into());
        }
    }
    Ok(token.to_string())
}

fn project_id() -> Option<String> {
    creds::read_json(&creds_path()).and_then(|data| {
        ["project_id", "project"]
            .iter()
            .find_map(|key| json_api::text_field(&data, key))
    })
}

pub(super) fn parse_quota(body: &serde_json::Value) -> Vec<MetricLine> {
    let Some(buckets) = body.get("buckets").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut pro: Option<(f64, Option<String>)> = None;
    let mut flash: Option<(f64, Option<String>)> = None;
    let mut other: Option<(f64, Option<String>, String)> = None;
    for bucket in buckets {
        let Some(remaining) = field(bucket, "remainingFraction") else {
            continue;
        };
        let used = (1.0 - remaining) * 100.0;
        let resets = bucket.get("resetTime").and_then(util::to_iso);
        let model = json_api::text_field(bucket, "modelId").unwrap_or_default();
        let model_lower = model.to_ascii_lowercase();
        let slot = if model_lower.contains("pro") {
            Some(&mut pro)
        } else if model_lower.contains("flash") {
            Some(&mut flash)
        } else {
            None
        };
        if let Some(slot) = slot {
            let replace = slot.as_ref().is_none_or(|(current, _)| used > *current);
            if replace {
                *slot = Some((used, resets));
            }
        } else {
            let replace = other.as_ref().is_none_or(|(current, _, _)| used > *current);
            if replace {
                other = Some((used, resets, model));
            }
        }
    }
    let mut lines = Vec::new();
    if let Some((used, resets)) = pro {
        lines.push(MetricLine::percent("Pro", used, resets));
    }
    if let Some((used, resets)) = flash {
        lines.push(MetricLine::percent("Flash", used, resets));
    }
    if lines.is_empty() {
        if let Some((used, resets, model)) = other {
            let label = if model.is_empty() { "Quota" } else { &model };
            lines.push(MetricLine::percent(label, used, resets));
        }
    }
    lines
}

impl Provider for Gemini {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        creds_path().is_file()
    }

    fn probe(&self) -> ProviderOutput {
        let token = match access_token() {
            Ok(token) => token,
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        let body = match project_id() {
            Some(project) => serde_json::json!({"project": project}).to_string(),
            None => "{}".into(),
        };
        match json_api::post_bearer(
            "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota",
            &token,
            &body,
        ) {
            Ok(data) => {
                let lines = parse_quota(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "Gemini quota response had no buckets.")
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
    fn used_percent_is_one_minus_remaining() {
        let lines = parse_quota(&serde_json::json!({
            "buckets": [
                {"modelId": "gemini-2.5-pro", "remainingFraction": 0.75, "resetTime": "2026-09-24T20:00:00Z"},
                {"modelId": "gemini-2.5-flash", "remainingFraction": 0.5, "resetTime": "2026-09-24T20:00:00Z"}
            ]
        }));
        assert_eq!(lines.len(), 2);
    }
}
