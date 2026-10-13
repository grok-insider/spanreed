#![allow(missing_docs)]

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::bounds::MAX_MODELS;
use crate::session_catalog::grok_home;

#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModelState {
    pub revision: u64,
    pub source: &'static str,
    pub current_model: Option<String>,
    pub selectable_models: Vec<ModelProjection>,
    pub current_effort: Option<String>,
    pub effort_options: Vec<EffortOption>,
    pub can_set_model: bool,
    pub can_set_effort: bool,
    pub uncertain: bool,
    #[serde(skip)]
    pub use_model_config: bool,
}

impl Default for SessionModelState {
    fn default() -> Self {
        Self {
            revision: 0,
            source: "unavailable",
            current_model: None,
            selectable_models: Vec::new(),
            current_effort: None,
            effort_options: Vec::new(),
            can_set_model: false,
            can_set_effort: false,
            uncertain: false,
            use_model_config: false,
        }
    }
}

fn live_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
        .map(str::to_owned)
}

fn live_options(value: Option<&Value>, limit: usize) -> Vec<EffortOption> {
    let mut values = Vec::new();
    if let Some(options) = value.and_then(Value::as_array) {
        for option in options.iter().take(limit) {
            let choices = option
                .get("options")
                .and_then(Value::as_array)
                .map_or_else(|| std::slice::from_ref(option), Vec::as_slice);
            for choice in choices.iter().take(limit) {
                let Some(id) = live_string(choice.get("value")) else {
                    continue;
                };
                if values.iter().any(|old: &EffortOption| old.id == id) {
                    continue;
                }
                let label = live_string(choice.get("name")).unwrap_or_else(|| id.clone());
                values.push(EffortOption { id, label });
                if values.len() >= limit {
                    return values;
                }
            }
        }
    }
    values
}

#[allow(missing_docs)]
impl SessionModelState {
    pub fn capture(&mut self, value: &Value) -> bool {
        let previous = self.clone();
        if let Some(available) = value
            .pointer("/models/availableModels")
            .and_then(Value::as_array)
        {
            self.selectable_models.clear();
            for entry in available.iter().take(128) {
                let Some(id) = live_string(entry.get("modelId")) else {
                    continue;
                };
                if self.selectable_models.iter().any(|model| model.id == id) {
                    continue;
                }
                self.selectable_models.push(ModelProjection {
                    name: live_string(entry.get("name")).unwrap_or_else(|| id.clone()),
                    id,
                    supports_reasoning_effort: false,
                    reasoning_efforts: Vec::new(),
                    default_effort: None,
                });
            }
            self.source = "live";
            self.can_set_model = !self.selectable_models.is_empty();
        }
        if let Some(model) = live_string(
            value
                .pointer("/_meta/x.ai~1sessionDetail/currentModelId")
                .or_else(|| value.pointer("/models/currentModelId")),
        ) {
            if self.current_model.as_ref() != Some(&model) {
                self.current_effort = None;
                self.effort_options.clear();
                self.can_set_effort = false;
            }
            self.current_model = Some(model);
        }
        if let Some(options) = value.get("configOptions").and_then(Value::as_array) {
            self.use_model_config = false;
            self.uncertain = false;
            self.current_effort = None;
            self.effort_options.clear();
            self.can_set_effort = false;
            for option in options.iter().take(64) {
                match option.get("id").and_then(Value::as_str) {
                    Some("model") => {
                        self.use_model_config = true;
                        if let Some(model) = live_string(option.get("currentValue")) {
                            self.current_model = Some(model);
                        }
                        let models = live_options(option.get("options"), 128);
                        self.selectable_models = models
                            .into_iter()
                            .map(|option| ModelProjection {
                                id: option.id,
                                name: option.label,
                                supports_reasoning_effort: false,
                                reasoning_efforts: Vec::new(),
                                default_effort: None,
                            })
                            .collect();
                        self.source = "live";
                        self.can_set_model = !self.selectable_models.is_empty();
                    }
                    Some("reasoning_effort") => {
                        self.current_effort = live_string(option.get("currentValue"));
                        self.effort_options = live_options(option.get("options"), 64);
                        self.can_set_effort = !self.effort_options.is_empty();
                    }
                    _ => {}
                }
            }
        }
        for model in &mut self.selectable_models {
            let current = Some(&model.id) == self.current_model.as_ref();
            model.supports_reasoning_effort = current && self.can_set_effort;
            model.reasoning_efforts = if current {
                self.effort_options.clone()
            } else {
                Vec::new()
            };
            model.default_effort = None;
        }
        if *self != previous {
            self.revision = previous.revision.saturating_add(1);
            true
        } else {
            false
        }
    }

    pub fn capture_update(&mut self, value: &Value) -> bool {
        let update = value.get("update").unwrap_or(value);
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("config_option_update") => self.capture(update),
            Some("model_changed") => {
                let Some(model) = live_string(
                    update
                        .get("model_id")
                        .or_else(|| update.get("currentModelId")),
                ) else {
                    return false;
                };
                let previous = self.clone();
                if self.current_model.as_ref() != Some(&model) {
                    self.effort_options.clear();
                    self.can_set_effort = false;
                }
                self.current_model = Some(model);
                self.current_effort = live_string(update.get("reasoning_effort"));
                self.uncertain = false;
                for model in &mut self.selectable_models {
                    let current = Some(&model.id) == self.current_model.as_ref();
                    model.supports_reasoning_effort = current && self.can_set_effort;
                    model.reasoning_efforts = if current {
                        self.effort_options.clone()
                    } else {
                        Vec::new()
                    };
                }
                if *self != previous {
                    self.revision = previous.revision.saturating_add(1);
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

/// One selectable model as the browser may see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProjection {
    /// Catalog / routing id (e.g. `grok-4.5`, `grok-build`).
    pub id: String,
    /// Human label.
    pub name: String,
    /// Whether the model exposes reasoning-effort controls.
    pub supports_reasoning_effort: bool,
    /// Effort options when supported (empty otherwise).
    pub reasoning_efforts: Vec<EffortOption>,
    /// Default effort id when supported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
}

/// One reasoning-effort level for a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffortOption {
    /// Wire / meta value (e.g. `high`, `medium`, `low`).
    pub id: String,
    /// UI label.
    pub label: String,
}

/// Whether a catalog id is a Grok/xAI family model Light may offer.
#[must_use]
pub fn is_grok_model_id(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    if lower.is_empty() || lower.contains('/') && !lower.starts_with("xai/") {
        // Foreign provider paths like `anthropic/claude` are refused.
        if lower.contains("claude")
            || lower.contains("gpt-")
            || lower.contains("openai")
            || lower.contains("gemini")
            || lower.contains("anthropic")
        {
            return false;
        }
    }
    lower.starts_with("grok")
        || lower.contains("grok-build")
        || lower.starts_with("xai/")
        || lower.contains("spacexai")
}

/// List Grok-only models from the user's cache (or empty if missing).
#[must_use]
pub fn list_models() -> Vec<ModelProjection> {
    list_models_in(&grok_home())
}

/// Same as [`list_models`] with an explicit Grok home.
#[must_use]
pub fn list_models_in(home: &Path) -> Vec<ModelProjection> {
    let path = home.join("models_cache.json");
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(root) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let Some(models) = root.get("models").and_then(Value::as_object) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (key, entry) in models {
        if !is_grok_model_id(key) {
            continue;
        }
        let info = entry.get("info").unwrap_or(entry);
        if info.get("hidden").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let id = info
            .get("id")
            .or_else(|| info.get("model"))
            .and_then(Value::as_str)
            .unwrap_or(key)
            .to_owned();
        if !is_grok_model_id(&id) {
            continue;
        }
        let name = info
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&id)
            .to_owned();
        let supports = info
            .get("supports_reasoning_effort")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut efforts = Vec::new();
        let mut default_effort = None;
        if supports {
            if let Some(list) = info.get("reasoning_efforts").and_then(Value::as_array) {
                for item in list {
                    let effort_id = item
                        .get("id")
                        .or_else(|| item.get("value"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if effort_id.is_empty() {
                        continue;
                    }
                    let label = item
                        .get("label")
                        .and_then(Value::as_str)
                        .unwrap_or(effort_id)
                        .to_owned();
                    if item.get("default").and_then(Value::as_bool) == Some(true) {
                        default_effort = Some(effort_id.to_owned());
                    }
                    efforts.push(EffortOption {
                        id: effort_id.to_owned(),
                        label,
                    });
                }
            }
            if default_effort.is_none() {
                default_effort = info
                    .get("reasoning_effort")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| efforts.first().map(|e| e.id.clone()));
            }
        }
        out.push(ModelProjection {
            id,
            name,
            supports_reasoning_effort: supports && !efforts.is_empty(),
            reasoning_efforts: efforts,
            default_effort,
        });
        if out.len() >= MAX_MODELS {
            break;
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Default model id from `config.toml` `[models] default = "…"`, if Grok.
#[must_use]
pub fn default_model_id() -> Option<String> {
    default_model_id_in(&grok_home())
}

/// Same as [`default_model_id`] with an explicit home.
#[must_use]
pub fn default_model_id_in(home: &Path) -> Option<String> {
    let raw = fs::read_to_string(home.join("config.toml")).ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("default") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let value = rest.trim().trim_matches('"').trim_matches('\'');
                if is_grok_model_id(value) {
                    return Some(value.to_owned());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{is_grok_model_id, list_models_in};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn live_catalogue_preserves_custom_models_and_only_current_efforts() {
        let mut state = super::SessionModelState::default();
        let response = serde_json::json!({"models":{"currentModelId":"relay/custom","availableModels":[{"modelId":"grok-live","name":"Grok"},{"modelId":"relay/custom","name":"Custom"},{"modelId":"relay/custom","name":"Duplicate"}]},"configOptions":[{"id":"reasoning_effort","currentValue":"balanced","options":[{"group":"custom","options":[{"value":"balanced","name":"Balanced"},{"value":"high","name":"High"}]}]}]});
        assert!(state.capture(&response));
        assert_eq!(state.selectable_models.len(), 2);
        assert_eq!(state.current_model.as_deref(), Some("relay/custom"));
        assert_eq!(state.effort_options.len(), 2);
        assert!(!state.selectable_models[0].supports_reasoning_effort);
        assert!(state.selectable_models[1].supports_reasoning_effort);
        let revision = state.revision;
        assert!(!state.capture(&response));
        assert_eq!(state.revision, revision);
        assert!(state.capture_update(
            &serde_json::json!({"update":{"sessionUpdate":"model_changed","model_id":"grok-live"}})
        ));
        assert!(state.current_effort.is_none());
        assert!(state.effort_options.is_empty());
        assert!(
            state
                .selectable_models
                .iter()
                .all(|model| model.reasoning_efforts.is_empty())
        );
    }

    #[test]
    fn advertised_empty_model_options_remove_the_previous_selectable_catalogue() {
        let mut state = super::SessionModelState::default();
        state.capture(&serde_json::json!({"configOptions":[{"id":"model","currentValue":"custom/model","options":[{"value":"custom/model","name":"Live custom"}]}]}));
        assert!(state.use_model_config && state.can_set_model);
        state.capture(&serde_json::json!({"configOptions":[{"id":"model","currentValue":"custom/model","options":[]}]}));
        assert!(!state.can_set_model);
        assert!(state.selectable_models.is_empty());
        assert_eq!(state.current_model.as_deref(), Some("custom/model"));
    }

    #[test]
    fn bounded_live_catalogue_never_projects_credentials_or_invalid_identifiers() {
        let mut state = super::SessionModelState::default();
        let mut models: Vec<_> = (0..150).map(|index| serde_json::json!({"modelId":format!("provider/{index}"),"apiKey":"synthetic-secret"})).collect();
        models[0]["modelId"] = serde_json::json!("invalid\nvalue");
        state.capture(&serde_json::json!({"models":{"availableModels":models}}));
        assert_eq!(state.selectable_models.len(), 127);
        let wire = serde_json::to_string(&state).unwrap();
        assert!(!wire.contains("synthetic-secret"));
        assert!(!wire.contains("invalid"));
    }

    #[test]
    fn grok_ids_pass_and_foreign_fail() {
        assert!(is_grok_model_id("grok-4.5"));
        assert!(is_grok_model_id("grok-build"));
        assert!(is_grok_model_id("Grok 4"));
        assert!(!is_grok_model_id("claude-opus"));
        assert!(!is_grok_model_id("gpt-5.6"));
        assert!(!is_grok_model_id("openai/gpt-4"));
    }

    #[test]
    fn list_filters_to_grok_and_exposes_efforts() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("light-models-{stamp}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("mkdir");
        fs::write(
            root.join("models_cache.json"),
            r#"{
              "models": {
                "grok-4.5": {
                  "info": {
                    "id": "grok-4.5",
                    "name": "Grok 4.5",
                    "supports_reasoning_effort": true,
                    "reasoning_effort": "high",
                    "reasoning_efforts": [
                      {"id": "high", "value": "high", "label": "High", "default": true},
                      {"id": "low", "value": "low", "label": "Low"}
                    ]
                  }
                },
                "claude-opus": {
                  "info": { "id": "claude-opus", "name": "Claude" }
                }
              }
            }"#,
        )
        .expect("write");
        let models = list_models_in(&root);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "grok-4.5");
        assert!(models[0].supports_reasoning_effort);
        assert_eq!(models[0].reasoning_efforts.len(), 2);
        assert_eq!(models[0].default_effort.as_deref(), Some("high"));
        let _ = fs::remove_dir_all(&root);
    }
}
