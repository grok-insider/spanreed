//! Deepgram project usage breakdown.
//!
//! Auth header is `Token`, not Bearer.

use crate::model::ProviderOutput;
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;

const ID: &str = "deepgram";
const NAME: &str = "Deepgram";

pub struct Deepgram;

pub(super) fn parse_projects(body: &serde_json::Value) -> Vec<String> {
    body.get("projects")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    json_api::text_field(row, "project_id")
                        .or_else(|| json_api::text_field(row, "id"))
                })
                .filter_map(|id| json_api::safe_segment(&id, 128).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn parse_breakdown(body: &serde_json::Value) -> Vec<(String, f64)> {
    let rows = body
        .get("results")
        .and_then(|v| v.as_array())
        .or_else(|| body.as_array());
    let Some(rows) = rows else {
        return Vec::new();
    };
    let mut requests = 0.0;
    let mut hours = 0.0;
    let mut tokens = 0.0;
    let mut characters = 0.0;
    for row in rows {
        requests += field(row, "requests").unwrap_or(0.0);
        hours += field(row, "total_hours")
            .or_else(|| field(row, "hours"))
            .unwrap_or(0.0);
        tokens += field(row, "tokens_in").unwrap_or(0.0) + field(row, "tokens_out").unwrap_or(0.0);
        characters += field(row, "tts_characters").unwrap_or(0.0);
    }
    let mut lines = Vec::new();
    if requests > 0.0 {
        lines.push(("Requests".into(), requests));
    }
    if hours > 0.0 {
        lines.push(("Audio hours".into(), hours));
    }
    if tokens > 0.0 {
        lines.push(("Tokens".into(), tokens));
    }
    if characters > 0.0 {
        lines.push(("TTS characters".into(), characters));
    }
    lines
}

impl Provider for Deepgram {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["DEEPGRAM_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["DEEPGRAM_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Deepgram API key found. Set DEEPGRAM_API_KEY.",
            );
        };
        let base = match env_any(&["DEEPGRAM_API_URL"]) {
            Some(raw) => match json_api::allowed_base(&raw, false) {
                Ok(base) => base,
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            },
            None => "https://api.deepgram.com/v1".into(),
        };
        let projects = if let Some(id) = env_any(&["DEEPGRAM_PROJECT_ID"]) {
            let Some(id) = json_api::safe_segment(&id, 128) else {
                return ProviderOutput::error(ID, NAME, "DEEPGRAM_PROJECT_ID is invalid.");
            };
            vec![id.to_string()]
        } else {
            match json_api::get_headers(
                &json_api::join_url(&base, "projects"),
                &[("Authorization", &format!("Token {key}"))],
            ) {
                Ok(data) => parse_projects(&data),
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            }
        };
        if projects.is_empty() {
            return ProviderOutput::error(ID, NAME, "Deepgram returned no projects for this key.");
        }
        let start = json_api::utc_ymd(30);
        let end = json_api::utc_ymd(0);
        let mut totals: Vec<(String, f64)> = Vec::new();
        for project in projects.iter().take(8) {
            let url = format!(
                "{}/projects/{project}/usage/breakdown?start={start}&end={end}",
                base.trim_end_matches('/')
            );
            match json_api::get_headers(&url, &[("Authorization", &format!("Token {key}"))]) {
                Ok(data) => {
                    for (label, value) in parse_breakdown(&data) {
                        if let Some(slot) = totals.iter_mut().find(|(name, _)| name == &label) {
                            slot.1 += value;
                        } else {
                            totals.push((label, value));
                        }
                    }
                }
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            }
        }
        if totals.is_empty() {
            return ProviderOutput::error(ID, NAME, "Deepgram usage breakdown was empty.");
        }
        let lines = totals
            .into_iter()
            .map(|(label, value)| json_api::text_line(&label, format!("{value:.2}")))
            .collect();
        ProviderOutput::new(ID, NAME, lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_breakdown_rows() {
        let rows = parse_breakdown(&serde_json::json!({
            "results": [
                {"requests": 4, "total_hours": 1.5, "tokens_in": 10, "tokens_out": 2, "tts_characters": 8},
                {"requests": 1, "hours": 0.5}
            ]
        }));
        assert!(rows
            .iter()
            .any(|(label, value)| label == "Requests" && *value == 5.0));
    }
}
