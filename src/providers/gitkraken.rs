//! GitKraken AI weekly credits.
//!
//! `GET https://api.gitkraken.dev/v1/ai-tasks/usage`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};
use crate::util;

const ID: &str = "gitkraken";
const NAME: &str = "GitKraken AI";

pub struct GitKraken;

fn quota_line(
    label: &str,
    value: &serde_json::Value,
    resets: Option<String>,
) -> Option<MetricLine> {
    let used = field(value, "used")?;
    let limit = field(value, "limit")?;
    if limit > 0.0 {
        json_api::used_percent(used, limit).map(|pct| MetricLine::percent(label, pct, resets))
    } else {
        Some(json_api::text_line(
            label,
            format!("{used:.0} credits used"),
        ))
    }
}

pub(super) fn parse_usage(body: &serde_json::Value) -> Result<Vec<MetricLine>, String> {
    if body.get("error").is_some_and(|value| !value.is_null()) {
        return Err("GitKraken returned an error payload.".into());
    }
    let data = body.get("data").ok_or("GitKraken response missing data")?;
    let resets = data.get("resetsOn").and_then(util::to_iso);
    let mut lines = Vec::new();
    if let Some(line) = quota_line("Personal", data, resets.clone()) {
        lines.push(line);
    }
    if let Some(org) = data.get("organization")
        && let Some(line) = quota_line("Shared pool", org, resets)
    {
        lines.push(line);
    }
    if lines.is_empty() {
        return Err("GitKraken usage response had no quota.".into());
    }
    Ok(lines)
}

impl Provider for GitKraken {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["GITKRAKEN_API_TOKEN"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(token) = env_any(&["GITKRAKEN_API_TOKEN"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No GitKraken token found. Set GITKRAKEN_API_TOKEN.",
            );
        };
        let mut headers = vec![
            ("Authorization", format!("Bearer {token}")),
            ("Client-Name", "spanreed".into()),
            ("Client-Version", env!("CARGO_PKG_VERSION").into()),
        ];
        let org_owned;
        if let Some(org) = env_any(&["GITKRAKEN_ORG_ID"]) {
            org_owned = org;
            headers.push(("gk-org-id", org_owned.clone()));
        }
        let header_refs: Vec<(&str, &str)> =
            headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        match json_api::get_headers("https://api.gitkraken.dev/v1/ai-tasks/usage", &header_refs) {
            Ok(data) => match parse_usage(&data) {
                Ok(lines) => ProviderOutput::new(ID, NAME, lines),
                Err(err) => ProviderOutput::error(ID, NAME, err),
            },
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_personal_and_pool() {
        let lines = parse_usage(&serde_json::json!({
            "data": {
                "used": 20.0,
                "limit": 100.0,
                "resetsOn": "2026-09-28T00:00:00Z",
                "organization": {"used": 5.0, "limit": 50.0}
            }
        }))
        .unwrap();
        assert_eq!(lines.len(), 2);
    }
}
