use serde_json::Value;

pub fn valid_access(document: &Value, now_ms: i64) -> Option<String> {
    if document.get("expires_at_ms")?.as_i64()? <= now_ms {
        return None;
    }
    document
        .get("access_token")?
        .as_str()
        .filter(|token| {
            !token.is_empty() && token.len() <= 16384 && token.bytes().all(|b| b.is_ascii_graphic())
        })
        .map(str::to_string)
}

pub fn ensure_access(
    document: &mut Value,
    now_ms: i64,
    refresh: impl FnOnce(&Value, i64) -> Result<Value, String>,
) -> Option<String> {
    if document
        .get("expires_at_ms")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        <= now_ms.saturating_add(120_000)
    {
        if let Ok(refreshed) = refresh(document, now_ms) {
            valid_access(&refreshed, now_ms)?;
            document
                .as_object_mut()?
                .extend(refreshed.as_object()?.clone());
        }
    }
    valid_access(document, now_ms)
}
