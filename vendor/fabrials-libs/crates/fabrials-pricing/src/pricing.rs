//! Model pricing (embedded LiteLLM snapshot plus data overlays). No HTTP.

use crate::provider_rates::ProviderRate;

use fabrials_types::HopRecord;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const EMBEDDED: &str = include_str!("pricing-data.json");
const EMBEDDED_OVERLAYS: &str = include_str!("pricing-overlays.json");

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
    /// The source published both the input and the output rate.
    pub token_rates_known: bool,
    /// The source published a cache-write rate (otherwise `cache_create` is estimated).
    pub cache_create_known: bool,
    /// The source published a cache-read rate (otherwise `cache_read` is estimated).
    pub cache_read_known: bool,
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
            token_rates_known: r.input_cost_per_token.is_some()
                && r.output_cost_per_token.is_some(),
            cache_create_known: r.cache_creation_input_token_cost.is_some(),
            cache_read_known: r.cache_read_input_token_cost.is_some(),
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

impl Usage {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_create + self.cache_read
    }
}

/// Upper bound on remembered `find` results. Live traffic uses a few dozen
/// distinct model names, so this only guards against an adversarial caller.
const MEMO_CAP: usize = 4096;

pub struct PricingMap {
    table: HashMap<String, Pricing>,
    /// Normalized key to list price. The table keys are provider-qualified
    /// (`xai/grok-4.3`) while usage records carry the bare name (`grok-4.3`),
    /// so this index answers those without touching `prefix`.
    normalized: HashMap<String, Pricing>,
    /// Normalized keys in a stable order, for the dated-variant fallback.
    prefix: Vec<(String, Pricing)>,
    /// `find` answers by model name. A fold revisits the same handful of
    /// models tens of thousands of times.
    memo: Mutex<HashMap<String, Option<Pricing>>>,
    provider_rates: Vec<ProviderRate>,
}

/// Prefer the least-qualified key when several normalize to the same name.
fn better_key(candidate: &str, current: &str) -> bool {
    (candidate.len(), candidate) < (current.len(), current)
}

impl PricingMap {
    fn new(table: HashMap<String, Pricing>) -> Self {
        let mut normalized: HashMap<String, (String, Pricing)> = HashMap::new();
        let mut keys: Vec<&String> = table.keys().collect();
        keys.sort();
        let mut prefix = Vec::with_capacity(keys.len());
        for key in keys {
            let pricing = table[key];
            let nkey = normalize(key);
            match normalized.get(&nkey) {
                Some((current, _)) if !better_key(key, current) => {}
                _ => {
                    normalized.insert(nkey.clone(), (key.clone(), pricing));
                }
            }
            prefix.push((nkey, pricing));
        }
        Self {
            table,
            normalized: normalized
                .into_iter()
                .map(|(nkey, (_, pricing))| (nkey, pricing))
                .collect(),
            prefix,
            memo: Mutex::new(HashMap::new()),
            provider_rates: Vec::new(),
        }
    }

    pub fn find(&self, model: &str) -> Option<Pricing> {
        if let Some(pricing) = self.table.get(model) {
            return Some(*pricing);
        }
        let norm = normalize(model);
        if let Some(pricing) = self.normalized.get(&norm) {
            return Some(*pricing);
        }
        let mut memo = self.memo.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(hit) = memo.get(model) {
            return *hit;
        }
        let found = self.scan_dated_variant(&norm);
        if memo.len() < MEMO_CAP {
            memo.insert(model.to_string(), found);
        }
        found
    }

    /// Longest shared prefix against the precomputed normalized keys.
    fn scan_dated_variant(&self, norm: &str) -> Option<Pricing> {
        let mut best: Option<(Pricing, usize)> = None;
        for (nkey, pricing) in &self.prefix {
            if pricing.is_voice_only() {
                continue;
            }
            let matched = if norm.starts_with(nkey.as_str()) || nkey.starts_with(norm) {
                nkey.len().min(norm.len())
            } else {
                0
            };
            if matched > 0 && best.map(|(_, l)| matched > l).unwrap_or(true) {
                best = Some((*pricing, matched));
            }
        }
        best.map(|(pricing, _)| pricing)
    }

    pub fn get(&self, model: &str) -> Option<&Pricing> {
        self.table
            .get(model)
            .or_else(|| self.table.get(&normalize(model)))
    }

    /// Strict lookup: the exact or normalized id only, no dated-variant guess.
    pub fn exact(&self, model: &str) -> Option<Pricing> {
        self.table
            .get(model)
            .or_else(|| self.normalized.get(&normalize(model)))
            .copied()
            .filter(|pricing| pricing.token_rates_known)
    }

    /// Strict cost for local logs: `None` when the model is not priced exactly
    /// or the usage needs a cache rate the source did not publish.
    pub fn exact_cost(&self, model: &str, usage: Usage) -> Option<(f64, Pricing)> {
        let p = self.exact(model)?;
        if usage.cache_create > 0 && !p.cache_create_known
            || usage.cache_read > 0 && !p.cache_read_known
        {
            return None;
        }
        let cost = tiered_cost(&p, usage);
        (cost.is_finite() && cost >= 0.0).then_some((cost, p))
    }

    pub fn cost(&self, model: &str, usage: Usage) -> Option<f64> {
        let p = self.find(model)?;
        Some(tiered_cost(&p, usage))
    }
}

fn tiered_cost(p: &Pricing, usage: Usage) -> f64 {
    tiered(usage.input, p.input, p.input_above_200k)
        + tiered(usage.output, p.output, p.output_above_200k)
        + tiered(
            usage.cache_create,
            p.cache_create,
            p.cache_create_above_200k,
        )
        + tiered(usage.cache_read, p.cache_read, p.cache_read_above_200k)
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

/// Where a host's price layers come from. Later layers win: the embedded
/// snapshot, the remote refresh, the published overlays upstream lacks, then
/// the user's override. Hosts own fetching and files; this crate only parses.
pub trait PricingSource {
    fn embedded(&self) -> Cow<'_, str>;
    fn remote(&self) -> Option<Cow<'_, str>> {
        None
    }
    /// Published list prices as an overlay document (`override`,
    /// `fill_missing`, `provider_rates`).
    fn overlays(&self) -> Cow<'_, str> {
        Cow::Borrowed(EMBEDDED_OVERLAYS)
    }
    fn user(&self) -> Option<Cow<'_, str>> {
        None
    }
}

/// Borrowed price layers; the plain [`PricingSource`] for hosts that already
/// hold the documents in memory.
#[derive(Debug, Clone, Copy)]
pub struct PricingLayers<'a> {
    pub embedded: &'a str,
    pub remote: Option<&'a str>,
    pub overlays: &'a str,
    pub user: Option<&'a str>,
}

impl<'a> PricingLayers<'a> {
    pub fn new(embedded: &'a str) -> Self {
        Self {
            embedded,
            remote: None,
            overlays: EMBEDDED_OVERLAYS,
            user: None,
        }
    }
}

impl PricingSource for PricingLayers<'_> {
    fn embedded(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.embedded)
    }
    fn remote(&self) -> Option<Cow<'_, str>> {
        self.remote.map(Cow::Borrowed)
    }
    fn overlays(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.overlays)
    }
    fn user(&self) -> Option<Cow<'_, str>> {
        self.user.map(Cow::Borrowed)
    }
}

/// The snapshot and overlays compiled into this crate.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmbeddedPricing;

impl PricingSource for EmbeddedPricing {
    fn embedded(&self) -> Cow<'_, str> {
        Cow::Borrowed(EMBEDDED)
    }
}

#[derive(Debug, Default, Deserialize)]
struct Overlays {
    #[serde(default)]
    r#override: HashMap<String, RawPricing>,
    #[serde(default)]
    fill_missing: HashMap<String, RawPricing>,
    #[serde(default)]
    provider_rates: Vec<ProviderRate>,
}

fn parse_raw(raw: &HashMap<String, RawPricing>) -> HashMap<String, Pricing> {
    raw.iter()
        .filter_map(|(k, v)| Pricing::from_raw(v).map(|p| (k.clone(), p)))
        .collect()
}

impl PricingMap {
    /// Build the table from host-supplied layers.
    pub fn from_source(source: &dyn PricingSource) -> Self {
        let mut table = parse_table(&source.embedded());
        if let Some(remote) = source.remote() {
            table.extend(parse_table(&remote));
        }
        let overlays: Overlays = serde_json::from_str(&source.overlays()).unwrap_or_default();
        table.extend(parse_raw(&overlays.r#override));
        for (model, pricing) in parse_raw(&overlays.fill_missing) {
            table.entry(model).or_insert(pricing);
        }
        if let Some(user) = source.user() {
            table.extend(parse_table(&user));
        }
        let mut map = PricingMap::new(table);
        map.provider_rates = overlays.provider_rates;
        map
    }

    /// Cost from a provider-specific rate card, when one matches the record.
    pub fn provider_rate_cost(&self, record: &HopRecord) -> Option<f64> {
        self.provider_rates
            .iter()
            .find_map(|rate| rate.cost(record))
    }
}

/// Thin wrapper over [`PricingMap::from_source`] with the embedded overlays.
pub fn build_table(embedded: &str, remote: Option<&str>, user: Option<&str>) -> PricingMap {
    PricingMap::from_source(&PricingLayers {
        embedded,
        remote,
        overlays: EMBEDDED_OVERLAYS,
        user,
    })
}

pub fn embedded_overlays_json() -> &'static str {
    EMBEDDED_OVERLAYS
}

pub fn embedded_json() -> &'static str {
    EMBEDDED
}

pub fn table() -> &'static PricingMap {
    static TABLE: OnceLock<PricingMap> = OnceLock::new();
    TABLE.get_or_init(|| PricingMap::from_source(&EmbeddedPricing))
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
    fn claude_5_models_carry_anthropic_list_prices() {
        let t = table();
        let opus = t.find("claude-opus-5-5").expect("claude-opus-5-5 priced");
        assert!((opus.input - 4e-6).abs() < 1e-15);
        assert!((opus.cache_read - 2e-7).abs() < 1e-15);
        assert!((opus.output - 2e-5).abs() < 1e-15);
        assert!(t.find("claude-opus-5").is_some());
        assert!(t.find("claude-sonnet-5").is_some());
    }

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
        assert!(t.find("grok-4.7").is_some(), "grok-4.7 priced");
        assert!(t.find("grok-4.7-build").is_some(), "grok-4.7-build priced");
        assert!(
            t.find("grok-4.7-build-fast").is_some(),
            "grok-4.7-build-fast priced"
        );
        assert!(t.find("grok-4.7-fast").is_some(), "grok-4.7-fast priced");
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

    #[test]
    fn bare_model_names_resolve_to_their_qualified_key() {
        let map = build_table(
            r#"{"xai/grok-4.3": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6}}"#,
            None,
            None,
        );
        let qualified = map.find("xai/grok-4.3").expect("qualified name priced");
        let bare = map.find("grok-4.3").expect("bare name priced");
        assert_eq!(bare.input, qualified.input);
        assert_eq!(bare.output, qualified.output);
    }

    #[test]
    fn embedded_table_prices_the_bare_grok_43_name() {
        let map = table();
        let qualified = map.find("xai/grok-4.3").expect("qualified grok-4.3");
        let bare = map.find("grok-4.3").expect("bare grok-4.3");
        assert_eq!(bare.input, qualified.input);
    }

    #[test]
    fn gpt_6_astra_uses_the_published_list_price() {
        let price = table().find("gpt-6-astra").expect("gpt-6-astra priced");
        assert!((price.input - 1e-5).abs() < 1e-15);
        assert!((price.output - 5e-5).abs() < 1e-15);
        assert!((price.cache_read - 1e-6).abs() < 1e-15);
        assert!((price.cache_create - 1.25e-5).abs() < 1e-15);
        assert!((price.input_above_200k.unwrap() - 2e-5).abs() < 1e-15);
        assert!((price.output_above_200k.unwrap() - 7.5e-5).abs() < 1e-15);
        assert!((price.cache_read_above_200k.unwrap() - 2e-6).abs() < 1e-15);
    }

    #[test]
    fn dated_variants_fall_back_to_the_shared_prefix() {
        let map = build_table(
            r#"{"xai/grok-4.6": {"input_cost_per_token": 3e-6, "output_cost_per_token": 9e-6}}"#,
            None,
            None,
        );
        let dated = map.find("grok-4.6-0309").expect("dated variant priced");
        assert_eq!(dated.input, 3e-6);
    }

    #[test]
    fn colliding_names_prefer_the_least_qualified_key() {
        let map = build_table(
            r#"{"azure_ai/grok-4": {"input_cost_per_token": 9e-6, "output_cost_per_token": 9e-6},
                "xai/grok-4": {"input_cost_per_token": 3e-6, "output_cost_per_token": 15e-6}}"#,
            None,
            None,
        );
        assert_eq!(map.find("grok-4").expect("priced").input, 3e-6);
    }

    #[test]
    fn unknown_models_are_remembered_instead_of_rescanned() {
        let map = build_table(
            r#"{"xai/grok-4.6": {"input_cost_per_token": 3e-6, "output_cost_per_token": 9e-6}}"#,
            None,
            None,
        );
        assert!(map.find("totally-unknown").is_none());
        assert!(map.find("totally-unknown").is_none());
        let memo = map.memo.lock().unwrap_or_else(|error| error.into_inner());
        assert_eq!(memo.len(), 1);
    }
}
