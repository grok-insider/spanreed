//! Model limits (context window, max output) and channel prices.
//!
//! Limits are keyed by model id and resolved the way pricing is: an embedded
//! snapshot, an optional runtime cache, then the user's override. The snapshot
//! is generated from models.dev for the channels a product routes to, because
//! LiteLLM does not cover every channel (OpenCode Go among them). The same
//! source also supplies prices for those channels, in the shape the LiteLLM
//! table already uses.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::pricing::{normalize, Pricing, PricingMap};

/// Context window and output cap for one model, in tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Limits {
    pub context_window: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output: Option<u64>,
}

/// Price and limits for one model id, each when known.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModelRecord {
    pub pricing: Option<Pricing>,
    pub limits: Option<Limits>,
}

/// Resolved limits, keyed by normalized model id.
#[derive(Debug, Clone, Default)]
pub struct LimitsMap {
    table: HashMap<String, Limits>,
}

impl LimitsMap {
    pub fn new(table: HashMap<String, Limits>) -> Self {
        Self {
            table: table
                .into_iter()
                .map(|(id, limits)| (normalize(&id), limits))
                .collect(),
        }
    }

    pub fn get(&self, model: &str) -> Option<&Limits> {
        self.table.get(&normalize(model.trim()))
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

/// Parse a limits table: either a snapshot (`{"models": {...}}`) or a bare
/// `{"<id>": {context_window, max_output}}` map, as the pricing layers allow.
pub fn parse_limits(json: &str) -> LimitsMap {
    let Ok(raw) = serde_json::from_str::<serde_json::Value>(json) else {
        return LimitsMap::default();
    };
    let Some(object) = raw.as_object().filter(|object| !object.is_empty()) else {
        return LimitsMap::default();
    };
    let models = match object.get("models") {
        Some(serde_json::Value::Object(models)) => models.clone(),
        _ => object.clone(),
    };
    let Ok(models) =
        serde_json::from_value::<HashMap<String, Limits>>(serde_json::Value::Object(models))
    else {
        return LimitsMap::default();
    };
    LimitsMap::new(models)
}

/// Layer limits: embedded, then an optional runtime cache, then the user's
/// override. Later layers win per model and never drop earlier rows.
pub fn build_limits(embedded: &str, remote: Option<&str>, user: Option<&str>) -> LimitsMap {
    let mut table: HashMap<String, Limits> = parse_limits(embedded).table;
    for layer in [remote, user].into_iter().flatten() {
        table.extend(parse_limits(layer).table);
    }
    LimitsMap::new(table)
}

#[derive(Deserialize)]
struct ModelsDevRoot {
    #[serde(flatten)]
    providers: HashMap<String, ModelsDevProvider>,
}

#[derive(Deserialize)]
struct ModelsDevProvider {
    #[serde(default)]
    models: HashMap<String, ModelsDevModel>,
}

#[derive(Deserialize)]
struct ModelsDevModel {
    #[serde(default)]
    limit: Option<ModelsDevLimit>,
    #[serde(default)]
    cost: Option<ModelsDevCost>,
}

#[derive(Deserialize)]
struct ModelsDevLimit {
    #[serde(default)]
    context: Option<u64>,
    #[serde(default)]
    output: Option<u64>,
}

/// models.dev rates, in USD per million tokens.
#[derive(Deserialize, Default)]
struct ModelsDevRates {
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
    #[serde(default)]
    cache_read: Option<f64>,
    #[serde(default)]
    cache_write: Option<f64>,
}

#[derive(Deserialize, Default)]
struct ModelsDevCost {
    #[serde(flatten)]
    base: ModelsDevRates,
    /// Rates past the provider's context threshold. models.dev publishes the
    /// same numbers under an explicit `tiers` list with named sizes; the flat
    /// field is what a caller with a fixed 200k boundary wants.
    #[serde(default)]
    context_over_200k: Option<ModelsDevRates>,
}

/// Walk the requested providers in precedence order, handing each model to
/// `accept` until it takes the id. Returns how many ids the leading provider
/// contributed, which is what the callers gate on.
fn visit_models<'a>(
    root: &'a ModelsDevRoot,
    providers: &[&str],
    mut accept: impl FnMut(&'a str, &'a ModelsDevModel) -> bool,
) -> Result<usize, String> {
    let Some(leading) = providers.first() else {
        return Err("no providers requested".into());
    };
    let mut claimed: HashSet<&'a str> = HashSet::new();
    let mut from_leading = 0usize;
    for provider in providers {
        let Some(catalog) = root.providers.get(*provider) else {
            continue;
        };
        for (id, model) in &catalog.models {
            if claimed.contains(id.as_str()) {
                continue;
            }
            if accept(id, model) {
                claimed.insert(id);
                if *provider == *leading {
                    from_leading += 1;
                }
            }
        }
    }
    Ok(from_leading)
}

fn parse_models_dev(json: &str) -> Result<ModelsDevRoot, String> {
    serde_json::from_str(json).map_err(|e| format!("invalid models.dev JSON: {e}"))
}

/// Reduce a models.dev `api.json` to a limits table.
///
/// `providers` is precedence order: the first provider that publishes a model
/// supplies its limits, so a channel with its own caps (OpenCode Go) is listed
/// before the upstream vendor. `aliases` copies a canonical id's limits onto a
/// local id, e.g. (`deepseek-flash`, `deepseek-v4.1-flash`).
///
/// Fails when the source does not look like models.dev or when the leading
/// provider contributes nothing, matching the pricing filter's refusal to
/// replace a good table with a degenerate one.
pub fn filter_models_dev(
    json: &str,
    providers: &[&str],
    aliases: &[(&str, &str)],
) -> Result<String, String> {
    let root = parse_models_dev(json)?;
    let mut table: BTreeMap<String, Limits> = BTreeMap::new();
    let from_leading = visit_models(&root, providers, |id, model| {
        let Some(limit) = model.limit.as_ref() else {
            return false;
        };
        let Some(context_window) = limit.context.filter(|context| *context > 0) else {
            return false;
        };
        table.insert(
            id.to_string(),
            Limits {
                context_window,
                max_output: limit.output.filter(|output| *output > 0),
            },
        );
        true
    })?;
    if from_leading == 0 {
        let leading = providers.first().copied().unwrap_or_default();
        return Err(format!("models.dev has no models for {leading}; refusing"));
    }
    for (alias, canonical) in aliases {
        if let Some(limits) = table.get(*canonical).copied() {
            table.insert((*alias).to_string(), limits);
        }
    }
    serde_json::to_string(&table).map_err(|e| e.to_string())
}

/// Reduce a models.dev `api.json` to a LiteLLM-shaped price table in USD per
/// token, so it layers with the LiteLLM snapshot through the same parser.
///
/// A row without both an input and an output rate is skipped; a cache rate the
/// source omits stays absent, so a strict caller keeps refusing to price usage
/// it cannot account for.
pub fn models_dev_prices(
    json: &str,
    providers: &[&str],
    aliases: &[(&str, &str)],
) -> Result<String, String> {
    let root = parse_models_dev(json)?;
    let mut table: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let from_leading = visit_models(&root, providers, |id, model| {
        let Some(row) = price_row(model.cost.as_ref()) else {
            return false;
        };
        table.insert(id.to_string(), row);
        true
    })?;
    if from_leading == 0 {
        let leading = providers.first().copied().unwrap_or_default();
        return Err(format!(
            "models.dev has no priced models for {leading}; refusing"
        ));
    }
    for (alias, canonical) in aliases {
        if let Some(row) = table.get(*canonical).cloned() {
            table.insert((*alias).to_string(), row);
        }
    }
    serde_json::to_string(&table).map_err(|e| e.to_string())
}

fn price_row(cost: Option<&ModelsDevCost>) -> Option<serde_json::Value> {
    let cost = cost?;
    let (Some(input), Some(output)) = (cost.base.input, cost.base.output) else {
        return None;
    };
    let mut row = serde_json::Map::new();
    insert_rate(&mut row, "input_cost_per_token", Some(input));
    insert_rate(&mut row, "output_cost_per_token", Some(output));
    insert_rate(
        &mut row,
        "cache_read_input_token_cost",
        cost.base.cache_read,
    );
    insert_rate(
        &mut row,
        "cache_creation_input_token_cost",
        cost.base.cache_write,
    );
    if let Some(tier) = cost.context_over_200k.as_ref() {
        insert_rate(
            &mut row,
            "input_cost_per_token_above_200k_tokens",
            tier.input,
        );
        insert_rate(
            &mut row,
            "output_cost_per_token_above_200k_tokens",
            tier.output,
        );
        insert_rate(
            &mut row,
            "cache_read_input_token_cost_above_200k_tokens",
            tier.cache_read,
        );
        insert_rate(
            &mut row,
            "cache_creation_input_token_cost_above_200k_tokens",
            tier.cache_write,
        );
    }
    Some(serde_json::Value::Object(row))
}

fn insert_rate(
    row: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    per_mtok: Option<f64>,
) {
    let Some(rate) = per_mtok.filter(|rate| rate.is_finite() && *rate >= 0.0) else {
        return;
    };
    row.insert(key.to_string(), serde_json::json!(rate / 1_000_000.0));
}

/// Compose LiteLLM-shaped price tables, later layers winning per model id, so
/// two upstreams can be written as one cache file and read as a single layer.
pub fn merge_price_tables(layers: &[&str]) -> Result<String, String> {
    let mut merged: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for layer in layers {
        let parsed: BTreeMap<String, serde_json::Value> =
            serde_json::from_str(layer).map_err(|e| format!("invalid price table: {e}"))?;
        merged.extend(parsed);
    }
    serde_json::to_string(&merged).map_err(|e| e.to_string())
}

/// Pricing and limits resolved from one pair of tables.
pub struct ModelData {
    pricing: PricingMap,
    limits: LimitsMap,
}

impl ModelData {
    pub fn new(pricing: PricingMap, limits: LimitsMap) -> Self {
        Self { pricing, limits }
    }

    pub fn record(&self, model: &str) -> ModelRecord {
        ModelRecord {
            pricing: self.pricing.find(model),
            limits: self.limits.get(model).copied(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOT: &str = r#"{
        "source": "https://models.dev/api.json",
        "generated": "2026-09-22",
        "models": {
            "deepseek-v4.1-flash": {"context_window": 1000000, "max_output": 384000},
            "glm-5.1": {"context_window": 202752}
        }
    }"#;

    #[test]
    fn snapshot_and_bare_maps_both_parse() {
        let snapshot = parse_limits(SNAPSHOT);
        assert_eq!(snapshot.len(), 2);
        assert_eq!(
            snapshot
                .get("deepseek-v4.1-flash")
                .map(|l| l.context_window),
            Some(1_000_000)
        );
        assert_eq!(
            snapshot
                .get("deepseek-v4.1-flash")
                .and_then(|l| l.max_output),
            Some(384_000)
        );
        assert_eq!(
            parse_limits(r#"{"grok-4.7": {"context_window": 500000}}"#)
                .get("grok-4.7")
                .map(|l| l.context_window),
            Some(500_000)
        );
    }

    #[test]
    fn lookup_ignores_case_and_provider_prefixes() {
        let map = parse_limits(SNAPSHOT);
        assert_eq!(
            map.get("openrouter/deepseek-v4.1-flash")
                .map(|l| l.context_window),
            Some(1_000_000)
        );
        assert_eq!(
            map.get(" DeepSeek-V4.1-Flash ").map(|l| l.context_window),
            Some(1_000_000)
        );
        assert!(map.get("no-such-model").is_none());
    }

    #[test]
    fn layers_override_per_model_without_dropping_rows() {
        let remote = r#"{"glm-5.1": {"context_window": 1000000}}"#;
        let user = r#"{"models": {"deepseek-v4.1-flash": {"context_window": 128000, "max_output": 8192}}}"#;
        let map = build_limits(SNAPSHOT, Some(remote), Some(user));
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("glm-5.1").map(|l| l.context_window),
            Some(1_000_000)
        );
        assert_eq!(
            map.get("deepseek-v4.1-flash").and_then(|l| l.max_output),
            Some(8_192)
        );
    }

    #[test]
    fn malformed_input_yields_an_empty_table() {
        assert!(parse_limits("not json").is_empty());
        assert!(parse_limits("[1,2,3]").is_empty());
        assert!(parse_limits(r#"{"models": "nope"}"#).is_empty());
    }

    #[test]
    fn filter_prefers_the_leading_provider_and_copies_aliases() {
        let upstream = serde_json::json!({
            "opencode-go": {
                "models": {
                    "deepseek-v4.1-flash": {"limit": {"context": 1000000, "output": 384000}},
                    "glm-5.1": {"limit": {"context": 202752}}
                }
            },
            "openrouter": {
                "models": {
                    "deepseek-v4.1-flash": {"limit": {"context": 65536}},
                    "mistral-large": {"limit": {"context": 131072}}
                }
            }
        })
        .to_string();
        let filtered = filter_models_dev(
            &upstream,
            &["opencode-go", "openrouter"],
            &[("deepseek-flash", "deepseek-v4.1-flash")],
        )
        .expect("filter ok");
        let map = parse_limits(&filtered);
        assert_eq!(
            map.get("deepseek-flash").map(|l| l.context_window),
            Some(1_000_000)
        );
        assert_eq!(map.get("glm-5.1").map(|l| l.context_window), Some(202_752));
        assert_eq!(
            map.get("deepseek-v4.1-flash").and_then(|l| l.max_output),
            Some(384_000)
        );
    }

    #[test]
    fn filter_rejects_a_source_without_the_leading_provider() {
        let upstream = serde_json::json!({
            "openrouter": {"models": {"glm-5.1": {"limit": {"context": 202752}}}}
        })
        .to_string();
        assert!(filter_models_dev(&upstream, &["opencode-go"], &[]).is_err());
        assert!(filter_models_dev("nope", &["opencode-go"], &[]).is_err());
        assert!(filter_models_dev(&upstream, &[], &[]).is_err());
    }

    #[test]
    fn record_joins_price_and_limits() {
        let pricing = crate::pricing::build_table(
            r#"{"deepseek-flash": {"input_cost_per_token": 1.5e-7, "output_cost_per_token": 6e-7}}"#,
            None,
            None,
        );
        let data = ModelData::new(pricing, parse_limits(SNAPSHOT));
        let known = data.record("deepseek-flash");
        assert!(known.pricing.is_some());
        assert!(known.limits.is_none(), "alias is not in the fixture");
        let both = data.record("deepseek-v4.1-flash");
        assert!(both.pricing.is_none(), "only the alias is priced");
        assert_eq!(both.limits.map(|l| l.context_window), Some(1_000_000));
    }

    const PRICED: &str = r#"{
        "opencode-go": {
            "models": {
                "deepseek-v4.1-flash": {
                    "limit": {"context": 1000000, "output": 384000},
                    "cost": {"input": 0.15, "output": 0.6, "cache_read": 0.003}
                },
                "grok-4.6": {
                    "limit": {"context": 500000},
                    "cost": {
                        "input": 2, "output": 6, "cache_read": 0.5,
                        "context_over_200k": {"input": 4, "output": 12, "cache_read": 1}
                    }
                },
                "ox-alpha-free": {"limit": {"context": 1000000}, "cost": {}},
                "no-cost": {"limit": {"context": 1000}}
            }
        },
        "openrouter": {
            "models": {
                "deepseek-v4.1-flash": {
                    "cost": {"input": 9, "output": 9}
                }
            }
        }
    }"#;

    #[test]
    fn prices_convert_to_per_token_and_keep_the_threshold_tier() {
        let filtered = models_dev_prices(PRICED, &["opencode-go"], &[]).expect("filter ok");
        let table: HashMap<String, serde_json::Value> =
            serde_json::from_str(&filtered).expect("json");
        assert!(
            !table.contains_key("ox-alpha-free"),
            "a row without rates goes"
        );
        assert!(!table.contains_key("no-cost"), "cost is required");
        let flash = table["deepseek-v4.1-flash"].as_object().expect("row");
        assert!((flash["input_cost_per_token"].as_f64().unwrap() - 1.5e-7).abs() < 1e-15);
        assert!((flash["output_cost_per_token"].as_f64().unwrap() - 6e-7).abs() < 1e-15);
        assert!((flash["cache_read_input_token_cost"].as_f64().unwrap() - 3e-9).abs() < 1e-15);
        assert!(
            !flash.contains_key("cache_creation_input_token_cost"),
            "a rate the source omits stays unknown"
        );
        let grok = table["grok-4.6"].as_object().expect("row");
        assert!(
            (grok["input_cost_per_token_above_200k_tokens"]
                .as_f64()
                .unwrap()
                - 4e-6)
                .abs()
                < 1e-15
        );
        assert!(
            (grok["cache_read_input_token_cost_above_200k_tokens"]
                .as_f64()
                .unwrap()
                - 1e-6)
                .abs()
                < 1e-15
        );
    }

    #[test]
    fn prices_prefer_the_leading_provider_and_copy_aliases() {
        let filtered = models_dev_prices(
            PRICED,
            &["opencode-go", "openrouter"],
            &[("deepseek-flash", "deepseek-v4.1-flash")],
        )
        .expect("filter ok");
        let table: HashMap<String, serde_json::Value> =
            serde_json::from_str(&filtered).expect("json");
        let canonical = table["deepseek-v4.1-flash"].as_object().expect("row");
        assert!((canonical["input_cost_per_token"].as_f64().unwrap() - 1.5e-7).abs() < 1e-15);
        assert_eq!(table["deepseek-flash"], table["deepseek-v4.1-flash"]);
    }

    #[test]
    fn prices_layer_over_the_litellm_table_through_the_same_parser() {
        let embedded =
            r#"{"xai/grok-4.6": {"input_cost_per_token": 3e-6, "output_cost_per_token": 9e-6}}"#;
        let channel = models_dev_prices(PRICED, &["opencode-go"], &[]).expect("filter ok");
        let litellm = r#"{
            "xai/grok-4.6": {"input_cost_per_token": 3e-6, "output_cost_per_token": 9e-6},
            "xai/grok-4.3": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6}
        }"#;
        let remote = merge_price_tables(&[&channel, litellm]).expect("merge ok");
        let table = crate::pricing::build_table(embedded, Some(&remote), None);
        assert!(
            (table.get("xai/grok-4.6").expect("litellm row").input - 3e-6).abs() < 1e-12,
            "the LiteLLM layer wins the ids both carry"
        );
        assert!(
            (table.get("deepseek-v4.1-flash").expect("channel row").input - 1.5e-7).abs() < 1e-15,
            "the channel supplies what LiteLLM lacks"
        );
        assert!(
            table.get("xai/grok-4.3").is_some(),
            "unrelated rows survive"
        );
    }

    #[test]
    fn merge_refuses_a_layer_that_is_not_a_table() {
        assert!(merge_price_tables(&[r#"{"a": {}}"#, "nope"]).is_err());
    }

    #[test]
    fn merge_keeps_both_spellings_of_one_model() {
        let channel = r#"{"grok-4.6": {"input_cost_per_token": 2e-6}}"#;
        let litellm = r#"{"xai/grok-4.6": {"input_cost_per_token": 3e-6}}"#;
        let merged = merge_price_tables(&[channel, litellm]).expect("merge ok");
        let table: BTreeMap<String, serde_json::Value> = serde_json::from_str(&merged).unwrap();
        assert_eq!(table.len(), 2);
        assert_eq!(
            table["grok-4.6"]["input_cost_per_token"].as_f64(),
            Some(2e-6),
            "a lookup by the channel's own id keeps its rate"
        );
    }

    #[test]
    fn prices_refuse_a_source_without_the_leading_provider() {
        let upstream = serde_json::json!({
            "openrouter": {"models": {"glm-5.1": {"cost": {"input": 1.4, "output": 4.4}}}}
        })
        .to_string();
        assert!(models_dev_prices(&upstream, &["opencode-go"], &[]).is_err());
        assert!(models_dev_prices("nope", &["opencode-go"], &[]).is_err());
        assert!(models_dev_prices(&upstream, &[], &[]).is_err());
    }
}
