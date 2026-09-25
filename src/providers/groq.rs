//! Groq Enterprise Prometheus rates.
//!
//! Console cookie sessions are not imported. A standard key that cannot read
//! Prometheus is reported as such.

use crate::model::ProviderOutput;
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, number};

const ID: &str = "groq";
const NAME: &str = "Groq";

pub struct Groq;

pub(super) fn parse_scalar(body: &serde_json::Value) -> Option<f64> {
    let value = body.pointer("/data/result/0/value/1")?;
    number(value)
}

fn query(base: &str, key: &str, promql: &str) -> Result<f64, String> {
    let url = format!(
        "{base}/metrics/prometheus/api/v1/query?query={}",
        json_api::query_escape(promql)
    );
    let data = json_api::get_bearer(&url, key)?;
    parse_scalar(&data).ok_or_else(|| "Groq Prometheus response had no sample.".into())
}

impl Provider for Groq {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["GROQ_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["GROQ_API_KEY"]) else {
            return ProviderOutput::error(ID, NAME, "No Groq API key found. Set GROQ_API_KEY.");
        };
        let base = match env_any(&["GROQ_API_URL"]) {
            Some(raw) => match json_api::allowed_base(&raw, false) {
                Ok(base) => base,
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            },
            None => "https://api.groq.com/v1".into(),
        };
        let requests = match query(
            &base,
            &key,
            "sum(model_project_id_status_code:requests:rate5m)",
        ) {
            Ok(value) => value,
            Err(err) => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    format!("{err} Enterprise Prometheus is unavailable for this key."),
                );
            }
        };
        let mut lines = vec![json_api::text_line(
            "Requests",
            format!("{requests:.2} / 5m"),
        )];
        if let Ok(tokens_in) = query(&base, &key, "sum(model_project_id:tokens_in:rate5m)") {
            lines.push(json_api::text_line(
                "Tokens in",
                format!("{tokens_in:.2} / 5m"),
            ));
        }
        if let Ok(tokens_out) = query(&base, &key, "sum(model_project_id:tokens_out:rate5m)") {
            lines.push(json_api::text_line(
                "Tokens out",
                format!("{tokens_out:.2} / 5m"),
            ));
        }
        if let Ok(cache) = query(
            &base,
            &key,
            "sum(model_project_id:prompt_cache_hits:rate5m)",
        ) {
            lines.push(json_api::text_line(
                "Cache hits",
                format!("{cache:.2} / 5m"),
            ));
        }
        ProviderOutput::new(ID, NAME, lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_prometheus_sample() {
        let value = parse_scalar(&serde_json::json!({
            "status": "success",
            "data": {"resultType": "vector", "result": [{"value": [1700000000, "3.5"]}]}
        }));
        assert_eq!(value, Some(3.5));
    }
}
