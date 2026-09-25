//! Ollama Cloud API-key check.
//!
//! Quota windows on the settings page need a browser cookie, which Spanreed
//! does not import. An API key is verified with an empty web-search body and
//! the model catalog is counted from `/api/tags`.

use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any};

const ID: &str = "ollama";
const NAME: &str = "Ollama";

pub struct Ollama;

pub(super) fn parse_tags(body: &serde_json::Value) -> Vec<MetricLine> {
    let count = body
        .get("models")
        .and_then(|v| v.as_array())
        .map(|rows| rows.len())
        .unwrap_or(0);
    vec![
        json_api::text_line("Models", count.to_string()),
        MetricLine::badge(
            MetricKind::Quota,
            "Quota",
            "API key verified; plan windows need a browser session",
        ),
    ]
}

impl Provider for Ollama {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["OLLAMA_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["OLLAMA_API_KEY"]) else {
            return ProviderOutput::error(ID, NAME, "No Ollama API key found. Set OLLAMA_API_KEY.");
        };
        let check =
            json_api::post_bearer("https://ollama.com/api/web_search", &key, r#"{"query":""}"#);
        match check {
            Ok(_) => {}
            Err(err) if err.contains("HTTP 400") => {}
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        }
        match json_api::get_bearer("https://ollama.com/api/tags", &key) {
            Ok(data) => ProviderOutput::new(ID, NAME, parse_tags(&data)),
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_models() {
        let lines = parse_tags(&serde_json::json!({"models": [{"name": "llama3"}]}));
        assert_eq!(lines.len(), 2);
    }
}
