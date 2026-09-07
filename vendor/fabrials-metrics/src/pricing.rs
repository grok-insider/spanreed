//! Model pricing (embedded LiteLLM snapshot). No HTTP.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

const EMBEDDED: &str = include_str!("pricing-data.json");

const FAMILIES: &[&str] = &["claude", "gpt", "codex", "gemini", "grok", "minimax"];

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RawPricing {
    #[serde(skip_serializing_if = "Option::is_none")]
    input_cost_per_token: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_cost_per_token: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_creation_input_token_cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_read_input_token_cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_cost_per_token_above_200k_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_cost_per_token_above_200k_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_creation_input_token_cost_above_200k_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_read_input_token_cost_above_200k_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_per_character: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_per_second: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_per_image: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct Pricing {
    pub input: f64,
    pub output: f64,
    pub cache_create: f64,
    pub cache_read: f64,
    pub input_above_200k: Option<f64>,
    pub output_above_200k: Option<f64>,
    pub cache_create_above_200k: Option<f64>,
    pub cache_read_above_200k: Option<f64>,
    pub cost_per_character: Option<f64>,
    pub cost_per_second: Option<f64>,
    pub cost_per_image: Option<f64>,
}

impl Pricing {
    fn from_raw(r: &RawPricing) -> Option<Self> {
        let voice = r
            .cost_per_character
            .or(r.cost_per_second)
            .or(r.cost_per_image)
            .is_some();
        if r.input_cost_per_token.is_none() && r.output_cost_per_token.is_none() && !voice {
            return None;
        }
        let input = r.input_cost_per_token.unwrap_or(0.0);
        let output = r.output_cost_per_token.unwrap_or(0.0);
        Some(Pricing {
            input,
            output,
            cache_create: r.cache_creation_input_token_cost.unwrap_or(input * 1.25),
            cache_read: r.cache_read_input_token_cost.unwrap_or(input * 0.1),
            input_above_200k: r.input_cost_per_token_above_200k_tokens,
            output_above_200k: r.output_cost_per_token_above_200k_tokens,
            cache_create_above_200k: r.cache_creation_input_token_cost_above_200k_tokens,
            cache_read_above_200k: r.cache_read_input_token_cost_above_200k_tokens,
            cost_per_character: r.cost_per_character,
            cost_per_second: r.cost_per_second,
            cost_per_image: r.cost_per_image,
        })
    }

    fn is_voice_only(self) -> bool {
        (self.cost_per_character.is_some()
            || self.cost_per_second.is_some()
            || self.cost_per_image.is_some())
            && self.input == 0.0
            && self.output == 0.0
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_create: u64,
    pub cache_read: u64,
}

pub struct PricingMap {
    table: HashMap<String, Pricing>,
}

impl PricingMap {
    pub fn find(&self, model: &str) -> Option<&Pricing> {
        if let Some(p) = self.table.get(model) {
            return Some(p);
        }
        let norm = normalize(model);
        if let Some(p) = self.table.get(&norm) {
            return Some(p);
        }
        let mut best: Option<(&Pricing, usize)> = None;
        for (key, pricing) in &self.table {
            if pricing.is_voice_only() {
                continue;
            }
            let nkey = normalize(key);
            let matched = if norm.starts_with(&nkey) || nkey.starts_with(&norm) {
                nkey.len().min(norm.len())
            } else {
                0
            };
            if matched > 0 && best.map(|(_, l)| matched > l).unwrap_or(true) {
                best = Some((pricing, matched));
            }
        }
        best.map(|(p, _)| p)
    }

    pub fn get(&self, model: &str) -> Option<&Pricing> {
        self.table
            .get(model)
            .or_else(|| self.table.get(&normalize(model)))
    }

    pub fn cost(&self, model: &str, usage: Usage) -> Option<f64> {
        let p = self.find(model)?;
        Some(
            tiered(usage.input, p.input, p.input_above_200k)
                + tiered(usage.output, p.output, p.output_above_200k)
                + tiered(
                    usage.cache_create,
                    p.cache_create,
                    p.cache_create_above_200k,
                )
                + tiered(usage.cache_read, p.cache_read, p.cache_read_above_200k),
        )
    }
}

fn tiered(tokens: u64, base: f64, above_200k: Option<f64>) -> f64 {
    const TIER: u64 = 200_000;
    match above_200k {
        Some(high) if tokens > TIER => (TIER as f64) * base + ((tokens - TIER) as f64) * high,
        _ => (tokens as f64) * base,
    }
}

pub fn normalize(model: &str) -> String {
    let mut m = model.to_lowercase();
    if let Some(i) = m.rfind('/') {
        m = m[i + 1..].to_string();
    }
    m.replace([':', '@'], "-")
}

fn parse_table(json: &str) -> HashMap<String, Pricing> {
    let raw: HashMap<String, RawPricing> = serde_json::from_str(json).unwrap_or_default();
    raw.iter()
        .filter_map(|(k, v)| Pricing::from_raw(v).map(|p| (k.clone(), p)))
        .collect()
}

pub fn build_table(embedded: &str, remote: Option<&str>, user: Option<&str>) -> PricingMap {
    let mut table = parse_table(embedded);
    for layer in [remote, user].into_iter().flatten() {
        for (k, p) in parse_table(layer) {
            table.insert(k, p);
        }
    }
    overlay_xai_media_prices(&mut table);
    PricingMap { table }
}

/// Official xAI Voice + Imagine list prices (not LiteLLM).
fn overlay_xai_media_prices(table: &mut HashMap<String, Pricing>) {
    for (k, p) in xai_media_list_prices() {
        table.insert(k, p);
    }
}

fn xai_media_list_prices() -> Vec<(String, Pricing)> {
    const TTS_CHAR: f64 = 15.0 / 1_000_000.0;
    const STT_STREAM: f64 = 0.20 / 3600.0;
    const STT_BATCH: f64 = 0.10 / 3600.0;
    const REALTIME: f64 = 0.05 / 60.0;
    /// grok-imagine-image-quality 1K output (docs.x.ai).
    const IMG_QUALITY_1K: f64 = 0.05;
    const IMG_STANDARD: f64 = 0.02;
    /// grok-imagine-video 720p; grok-imagine-video-1.5 720p.
    const VIDEO_720P: f64 = 0.07;
    const VIDEO_15_720P: f64 = 0.14;
    fn row(
        cost_per_character: Option<f64>,
        cost_per_second: Option<f64>,
        cost_per_image: Option<f64>,
    ) -> Pricing {
        Pricing {
            input: 0.0,
            output: 0.0,
            cache_create: 0.0,
            cache_read: 0.0,
            input_above_200k: None,
            output_above_200k: None,
            cache_create_above_200k: None,
            cache_read_above_200k: None,
            cost_per_character,
            cost_per_second,
            cost_per_image,
        }
    }
    vec![
        ("tts".into(), row(Some(TTS_CHAR), None, None)),
        ("grok-tts".into(), row(Some(TTS_CHAR), None, None)),
        ("stt".into(), row(None, Some(STT_STREAM), None)),
        ("grok-stt".into(), row(None, Some(STT_STREAM), None)),
        ("stt-batch".into(), row(None, Some(STT_BATCH), None)),
        ("realtime".into(), row(None, Some(REALTIME), None)),
        ("grok-realtime".into(), row(None, Some(REALTIME), None)),
        (
            "grok-imagine-image-quality".into(),
            row(None, None, Some(IMG_QUALITY_1K)),
        ),
        (
            "grok-imagine-image".into(),
            row(None, None, Some(IMG_STANDARD)),
        ),
        (
            "grok-imagine-video".into(),
            row(None, Some(VIDEO_720P), None),
        ),
        (
            "grok-imagine-video-1.5".into(),
            row(None, Some(VIDEO_15_720P), None),
        ),
    ]
}

pub fn embedded_json() -> &'static str {
    EMBEDDED
}

pub fn table() -> &'static PricingMap {
    static TABLE: OnceLock<PricingMap> = OnceLock::new();
    TABLE.get_or_init(|| build_table(EMBEDDED, None, None))
}

fn relevant_model(key: &str) -> bool {
    let name = normalize(key);
    if FAMILIES.iter().any(|f| name.contains(f)) {
        return true;
    }
    let mut chars = name.chars();
    chars.next() == Some('o') && chars.next().is_some_and(|c| c.is_ascii_digit())
}

pub fn filter_upstream(json: &str) -> Result<String, String> {
    let raw: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid pricing JSON: {e}"))?;
    let obj = raw.as_object().ok_or("pricing JSON is not an object")?;

    let mut filtered = std::collections::BTreeMap::new();
    for (key, value) in obj {
        if !relevant_model(key) {
            continue;
        }
        let Ok(entry) = serde_json::from_value::<RawPricing>(value.clone()) else {
            continue;
        };
        if entry.input_cost_per_token.is_none() || entry.output_cost_per_token.is_none() {
            continue;
        }
        filtered.insert(key.clone(), entry);
    }

    if !filtered.keys().any(|k| normalize(k).contains("claude")) {
        return Err("filtered pricing table has no claude models; refusing".into());
    }
    serde_json::to_string(&filtered).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_table_parses_and_has_claude_and_gpt() {
        let t = table();
        assert!(
            t.find("claude-opus-4-8").is_some(),
            "claude-opus-4-8 priced"
        );
        assert!(t.find("gpt-5-codex").is_some(), "gpt-5-codex priced");
        assert!(t.find("grok-4.5").is_some() || t.find("grok-4").is_some());
        assert!(t.find("grok-4.6").is_some(), "grok-4.6 priced");
        assert!(t.find("grok-4.6-build").is_some(), "grok-4.6-build priced");
        assert!(
            t.find("claude-fable-5-1").is_some(),
            "claude-fable-5-1 priced"
        );
        assert!(t.get("tts").is_some(), "tts list price");
        assert!(t.get("stt").is_some(), "stt list price");
        assert!(t.get("stt-batch").is_some(), "stt-batch list price");
        assert!(t.get("realtime").is_some(), "realtime list price");
    }

    #[test]
    fn filter_upstream_keeps_relevant_families_and_cost_fields() {
        let upstream = serde_json::json!({
            "claude-fable-5": {
                "input_cost_per_token": 6e-6,
                "output_cost_per_token": 3e-5,
                "cache_read_input_token_cost": 6e-7,
                "litellm_provider": "anthropic",
                "max_tokens": 64000
            },
            "mistral-large": {
                "input_cost_per_token": 2e-6,
                "output_cost_per_token": 6e-6
            },
            "o4-mini": {
                "input_cost_per_token": 1e-6,
                "output_cost_per_token": 4e-6
            }
        })
        .to_string();
        let filtered = filter_upstream(&upstream).expect("filter ok");
        let map: HashMap<String, serde_json::Value> = serde_json::from_str(&filtered).unwrap();
        assert!(map.contains_key("claude-fable-5"));
        assert!(map.contains_key("o4-mini"));
        assert!(!map.contains_key("mistral-large"));
    }

    #[test]
    fn filter_upstream_rejects_tables_without_claude() {
        let upstream = serde_json::json!({
            "mistral-large": { "input_cost_per_token": 2e-6, "output_cost_per_token": 6e-6 }
        })
        .to_string();
        assert!(filter_upstream(&upstream).is_err());
    }
}
