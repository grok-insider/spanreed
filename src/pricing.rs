//! Model pricing for local-log cost estimation.
//!
//! A filtered snapshot of LiteLLM's `model_prices_and_context_window.json` is
//! embedded at compile time (no build-time network — Nix-sandbox friendly).
//! Users can override or extend it with `~/.config/spanreed/pricing.json`
//! (same shape: `{ "<model>": { input_cost_per_token, ... } }`).
//!
//! Prices are USD per token. Cache-write/read and a >200k-context tier are
//! supported, mirroring how the upstream pricing data is structured.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::creds;

const EMBEDDED: &str = include_str!("pricing-data.json");

/// Raw LiteLLM-shaped entry (only the fields we use).
#[derive(Debug, Clone, Deserialize)]
struct RawPricing {
    input_cost_per_token: Option<f64>,
    output_cost_per_token: Option<f64>,
    cache_creation_input_token_cost: Option<f64>,
    cache_read_input_token_cost: Option<f64>,
    input_cost_per_token_above_200k_tokens: Option<f64>,
    output_cost_per_token_above_200k_tokens: Option<f64>,
    cache_creation_input_token_cost_above_200k_tokens: Option<f64>,
    cache_read_input_token_cost_above_200k_tokens: Option<f64>,
}

/// Resolved per-token prices for a model.
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
}

impl Pricing {
    fn from_raw(r: &RawPricing) -> Option<Self> {
        let input = r.input_cost_per_token?;
        let output = r.output_cost_per_token?;
        Some(Pricing {
            input,
            output,
            // Anthropic-style defaults when the snapshot omits cache rates:
            // cache write ≈ 1.25× input, cache read ≈ 0.1× input.
            cache_create: r.cache_creation_input_token_cost.unwrap_or(input * 1.25),
            cache_read: r.cache_read_input_token_cost.unwrap_or(input * 0.1),
            input_above_200k: r.input_cost_per_token_above_200k_tokens,
            output_above_200k: r.output_cost_per_token_above_200k_tokens,
            cache_create_above_200k: r.cache_creation_input_token_cost_above_200k_tokens,
            cache_read_above_200k: r.cache_read_input_token_cost_above_200k_tokens,
        })
    }
}

/// Token usage for a single message (raw counts).
#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_create: u64,
    pub cache_read: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_create + self.cache_read
    }
}

/// The resolved pricing table, built once from embedded + user override.
pub struct PricingMap {
    table: HashMap<String, Pricing>,
}

impl PricingMap {
    /// Look up a model, trying exact, then normalized, then prefix matching.
    pub fn find(&self, model: &str) -> Option<&Pricing> {
        if let Some(p) = self.table.get(model) {
            return Some(p);
        }
        let norm = normalize(model);
        if let Some(p) = self.table.get(&norm) {
            return Some(p);
        }
        // Prefix fallback: a dated/suffixed query (claude-opus-4-8-20260601)
        // matches the longest base key that is a prefix of it (claude-opus-4-8),
        // or vice versa. Compare on normalized keys.
        let mut best: Option<(&Pricing, usize)> = None;
        for (key, pricing) in &self.table {
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

    /// Compute the USD cost for a usage record under a model.
    /// Returns None when the model has no known pricing.
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

/// Tiered pricing: tokens beyond 200k use the higher rate when present.
fn tiered(tokens: u64, base: f64, above_200k: Option<f64>) -> f64 {
    const TIER: u64 = 200_000;
    match above_200k {
        Some(high) if tokens > TIER => (TIER as f64) * base + ((tokens - TIER) as f64) * high,
        _ => (tokens as f64) * base,
    }
}

/// Normalize a model name for matching: lowercase, drop provider routing
/// prefix (`azure/`, `openai/`, ...), normalize `@`/`:` to `-`.
fn normalize(model: &str) -> String {
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

/// The process-wide pricing table (embedded snapshot, overlaid with the user's
/// optional `~/.config/spanreed/pricing.json`).
pub fn table() -> &'static PricingMap {
    static TABLE: OnceLock<PricingMap> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = parse_table(EMBEDDED);
        let override_path = creds::config_home().join("spanreed").join("pricing.json");
        if let Some(text) = creds::read_file(&override_path) {
            for (k, p) in parse_table(&text) {
                table.insert(k, p);
            }
        }
        PricingMap { table }
    })
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
    }

    #[test]
    fn prefix_match_handles_dated_suffix() {
        let t = table();
        // A dated variant should fall back to the base model's pricing.
        assert!(t.find("claude-opus-4-8-20260601").is_some());
    }

    #[test]
    fn unknown_model_has_no_price() {
        let t = table();
        assert!(t.find("totally-made-up-model-xyz").is_none());
        assert!(t
            .cost(
                "totally-made-up-model-xyz",
                Usage {
                    input: 10,
                    ..Default::default()
                }
            )
            .is_none());
    }

    #[test]
    fn cost_math_is_linear_in_tokens() {
        let map = PricingMap {
            table: HashMap::from([(
                "m".to_string(),
                Pricing {
                    input: 1e-6,
                    output: 2e-6,
                    cache_create: 5e-7,
                    cache_read: 1e-7,
                    input_above_200k: None,
                    output_above_200k: None,
                    cache_create_above_200k: None,
                    cache_read_above_200k: None,
                },
            )]),
        };
        let usage = Usage {
            input: 1_000_000,
            output: 1_000_000,
            cache_create: 1_000_000,
            cache_read: 1_000_000,
        };
        // 1e6*(1e-6 + 2e-6 + 5e-7 + 1e-7) = 1.0 + 2.0 + 0.5 + 0.1 = 3.6
        let cost = map.cost("m", usage).unwrap();
        assert!((cost - 3.6).abs() < 1e-9, "got {cost}");
    }

    #[test]
    fn tiered_pricing_applies_above_200k() {
        // 300k input tokens: 200k @ 1e-6 + 100k @ 2e-6 = 0.2 + 0.2 = 0.4
        let v = tiered(300_000, 1e-6, Some(2e-6));
        assert!((v - 0.4).abs() < 1e-9, "got {v}");
        // Without a tier, linear: 300k @ 1e-6 = 0.3
        let v2 = tiered(300_000, 1e-6, None);
        assert!((v2 - 0.3).abs() < 1e-9, "got {v2}");
    }

    #[test]
    fn normalize_strips_provider_prefix() {
        assert_eq!(normalize("openai/gpt-5-codex"), "gpt-5-codex");
        assert_eq!(normalize("claude-opus-4-8"), "claude-opus-4-8");
        assert_eq!(normalize("gpt-5@2025-08-07"), "gpt-5-2025-08-07");
    }
}
