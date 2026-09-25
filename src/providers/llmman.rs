//! llmman daemon memory.
//!
//! Reads `GET /llmman/node`. Detects when `LLMMAN_HOST` or `LLMMAN_API_KEY`
//! is set. A force probe uses `http://127.0.0.1:17434`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};

const ID: &str = "llmman";
const NAME: &str = "llmman";

pub struct Llmman;

fn bytes_label(value: f64) -> String {
    if value < 1_000.0 {
        format!("{value:.0} B")
    } else if value >= 1_000_000_000.0 {
        format!("{:.1} GB", value / 1_000_000_000.0)
    } else if value >= 1_000_000.0 {
        format!("{:.1} MB", value / 1_000_000.0)
    } else {
        format!("{:.1} kB", value / 1_000.0)
    }
}

pub(super) fn parse_node(body: &serde_json::Value) -> Vec<MetricLine> {
    let memory = field(body, "memory").unwrap_or(0.0);
    let loaded = body
        .get("loaded")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut used = 0.0;
    for model in &loaded {
        used += field(model, "size")
            .or_else(|| field(model, "bytes"))
            .unwrap_or(0.0);
    }
    let mut lines = Vec::new();
    if memory > 0.0 {
        lines.push(json_api::count_line("Memory", used, memory, "bytes", None));
        lines.push(json_api::text_line(
            "Loaded",
            format!("{} · {}", loaded.len(), bytes_label(used)),
        ));
    } else {
        lines.push(json_api::text_line(
            "Loaded models",
            loaded.len().to_string(),
        ));
    }
    lines
}

impl Provider for Llmman {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["LLMMAN_HOST"]).is_some() || env_any(&["LLMMAN_API_KEY"]).is_some()
    }

    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        let raw = env_any(&["LLMMAN_HOST"]).unwrap_or_else(|| "http://127.0.0.1:17434".into());
        let raw = if raw.contains("://") {
            raw
        } else if raw.contains(':') {
            format!("http://{raw}")
        } else {
            format!("http://{raw}:17434")
        };
        let base = match json_api::allowed_base(&raw, true) {
            Ok(base) => base
                .trim_end_matches('/')
                .trim_end_matches("/v1")
                .to_string(),
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        let mut headers: Vec<(&str, String)> = Vec::new();
        if let Some(key) = env_any(&["LLMMAN_API_KEY"]) {
            headers.push(("Authorization", format!("Bearer {key}")));
        }
        let header_refs: Vec<(&str, &str)> =
            headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        match json_api::get_headers(&json_api::join_url(&base, "llmman/node"), &header_refs) {
            Ok(data) => {
                let lines = parse_node(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "llmman node response was empty.")
                } else {
                    ProviderOutput::new(ID, NAME, lines)
                }
            }
            Err(err) => ProviderOutput::error(
                ID,
                NAME,
                format!("{err} Start the daemon with `llmman serve`."),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_loaded_bytes() {
        let lines = parse_node(&serde_json::json!({
            "memory": 40_000_000_000u64,
            "loaded": [{"name": "local", "size": 10_000_000_000u64}],
            "stored": []
        }));
        assert_eq!(lines.len(), 2);
    }
}
