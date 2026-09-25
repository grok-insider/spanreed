//! Bounded model identifiers shared by local and hosted catalog consumers.
use serde_json::Value;

pub fn model_ids(value: &Value) -> Result<Vec<String>, String> {
    let rows = value
        .get("data")
        .or_else(|| value.get("items"))
        .and_then(Value::as_array)
        .ok_or("Model catalog format unavailable")?;
    if rows.len() > 2048 {
        return Err("Model catalog exceeds 2048 entries".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for row in rows {
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .ok_or("Model identifier missing")?
            .trim();
        if id.is_empty() || id.len() > 256 || !id.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err("Invalid model identifier".into());
        }
        ids.insert(id.to_string());
    }
    let ids = ids.into_iter().collect::<Vec<_>>();
    validate_ids(&ids)?;
    Ok(ids)
}

/// Validate an already materialized catalog without silently dropping entries.
pub fn validate_ids(models: &[String]) -> Result<(), String> {
    if models.len() > 2048 {
        return Err("Model catalog exceeds 2048 entries".into());
    }
    let mut total = 0usize;
    let mut seen = std::collections::HashSet::new();
    for model in models {
        total = total.saturating_add(model.len());
        if model.is_empty()
            || model.len() > 256
            || total > 512 * 1024
            || !model.bytes().all(|byte| byte.is_ascii_graphic())
            || !seen.insert(model.as_str())
        {
            return Err("Invalid model catalog".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_primary_catalog_cannot_be_hidden_by_fallback() {
        assert!(model_ids(
            &serde_json::json!({"data":[{"id":"valid"},{}],"items":[{"id":"fallback"}]})
        )
        .is_err());
        assert!(model_ids(&serde_json::json!({"data":[{"id":"has space"}]})).is_err());
        assert!(model_ids(&serde_json::json!({"data":[{"id":"x".repeat(257)}]})).is_err());
        assert_eq!(
            model_ids(&serde_json::json!({"items":[{"id":" fallback "}]})).unwrap(),
            vec!["fallback"]
        );
        assert!(validate_ids(&["repeated".into(), "repeated".into()]).is_err());
    }
    #[test]
    fn distinguishes_empty_catalog_from_malformed_and_deduplicates() {
        assert_eq!(
            model_ids(&serde_json::json!({"data":[{"id":"b"},{"id":"a"},{"id":"b"}]})).unwrap(),
            vec!["a", "b"]
        );
        assert!(model_ids(&serde_json::json!({"data":[]}))
            .unwrap()
            .is_empty());
        assert!(model_ids(&serde_json::json!({"error":"unavailable"})).is_err());
        assert!(model_ids(&serde_json::json!({"data":[{}]})).is_err());
        assert!(model_ids(&serde_json::json!({"items":[{"id":"bad\nmodel"}]})).is_err());
        assert!(
            model_ids(&serde_json::json!({"data":vec![serde_json::json!({"id":"x"});2049]}))
                .is_err()
        );
    }
}
