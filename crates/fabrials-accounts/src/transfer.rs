//! Explicit API-key transfer material. OAuth documents are never transferable.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub fn contains_oauth_material(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            matches!(
                key.as_str(),
                "refresh_token" | "access_token" | "refreshToken" | "accessToken"
            ) || (key == "type" && value.as_str() == Some("oauth"))
                || contains_oauth_material(value)
        }),
        Value::Array(values) => values.iter().any(contains_oauth_material),
        _ => false,
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ApiKey(String);
impl std::fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiKey([redacted])")
    }
}
impl<'de> Deserialize<'de> for ApiKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl ApiKey {
    pub fn new(value: String) -> Result<Self, &'static str> {
        if value.is_empty()
            || value.len() > 16 * 1024
            || value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err("Invalid API key");
        }
        Ok(Self(value))
    }
    /// Accept only a known static-key document. Never copy an opaque grant or
    /// strip OAuth fields to make a mixed document look transferable.
    pub fn from_document(document: &Value) -> Result<Self, &'static str> {
        let object = document.as_object().ok_or("Not an API-key document")?;
        if contains_oauth_material(document)
            || object.contains_key("api_key") == object.contains_key("key")
            || object.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "api_key" | "key" | "catalog" | "balance_observation" | "reset_inventory"
                )
            })
        {
            return Err("Only explicit API-key documents can be transferred");
        }
        let value = object
            .get("api_key")
            .or_else(|| object.get("key"))
            .and_then(Value::as_str)
            .ok_or("Not an API-key document")?;
        Self::new(value.to_owned())
    }
    /// Call only in a credential transport or persistence adapter.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiKeyTransfer {
    pub source_id: String,
    pub source_generation: String,
    pub provider: String,
    pub target_alias: String,
    pub api_key: ApiKey,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_oauth_mixed_documents_and_redacts_diagnostics() {
        let key =
            ApiKey::from_document(&serde_json::json!({"api_key":"synthetic-secret"})).unwrap();
        assert_eq!(key.expose(), "synthetic-secret");
        assert!(!format!("{key:?}").contains("synthetic-secret"));
        for value in [
            serde_json::json!({"api_key":"synthetic-secret","refresh_token":"rotating"}),
            serde_json::json!({"auth":{"key":"access"}}),
            serde_json::json!({"access_token":"access"}),
            serde_json::json!({"api_key":"one","key":"two"}),
        ] {
            assert!(ApiKey::from_document(&value).is_err());
        }
        for value in ["", "with space", "header\r\ninjection"] {
            assert!(ApiKey::new(value.into()).is_err());
        }
        assert!(ApiKey::new("x".repeat(16 * 1024 + 1)).is_err());
    }
    #[test]
    fn known_probe_metadata_is_not_transferred_and_cannot_hide_grants() {
        let mut document = serde_json::json!({"api_key":"synthetic-key","catalog":{"models":["model"],"ok":true},"balance_observation":{"value":{"remaining":4}},"reset_inventory":{"value":null}});
        let key = ApiKey::from_document(&document).unwrap();
        assert_eq!(key.expose(), "synthetic-key");
        assert_eq!(
            serde_json::to_value(key).unwrap(),
            serde_json::json!("synthetic-key")
        );
        document["catalog"]["refresh_token"] = serde_json::json!("hidden-grant");
        assert!(ApiKey::from_document(&document).is_err());
    }

    #[test]
    fn wire_rejects_extra_grant_fields_without_echoing_secret() {
        let error = serde_json::from_value::<ApiKeyTransfer>(serde_json::json!({
            "sourceId":"nous/work","sourceGeneration":"g1","provider":"nous","targetAlias":"copy","apiKey":"synthetic-secret","refresh_token":"private-grant"
        })).unwrap_err();
        assert!(!error.to_string().contains("private-grant"));
        assert!(!error.to_string().contains("synthetic-secret"));
        let key: ApiKey = serde_json::from_str("\"synthetic\"").unwrap();
        assert_eq!(serde_json::to_string(&key).unwrap(), "\"synthetic\"");
    }
}
