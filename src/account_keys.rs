//! Local API-key enrollment. Credentials are never returned to the renderer.
use crate::accounts::{self, Account};

pub fn add(provider: &str, alias: &str, key: &str) -> Result<Account, String> {
    let key = validate(provider, key)?;
    let account = Account::new(provider, alias)?;
    let vault = accounts::lock_vault()?;
    let registry = vault.registry()?;
    let removal = registry.removed.get(&account.id);
    vault.register(
        account.clone(),
        &serde_json::json!({"api_key": key}).to_string(),
        removal,
    )?;
    vault
        .registry()?
        .accounts
        .into_iter()
        .find(|entry| entry.id == account.id)
        .ok_or("Account registration unavailable".into())
}

fn validate<'a>(provider: &str, key: &'a str) -> Result<&'a str, String> {
    if !matches!(provider, "openai" | "nous") {
        return Err("API-key enrollment supports OpenAI and Nous".into());
    }
    let key = key.trim();
    if key.is_empty() || key.len() > 8192 || !key.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err("Enter an API key without whitespace, up to 8192 characters".into());
    }
    Ok(key)
}

pub fn replace(id: &str, generation: Option<&str>, key: &str) -> Result<(), String> {
    let vault = accounts::lock_vault()?;
    let account = vault
        .registry()?
        .accounts
        .into_iter()
        .find(|a| a.id == id)
        .ok_or("Account no longer exists; refresh Accounts")?;
    if account.generation.as_deref() != generation {
        return Err("Account changed; refresh Accounts before replacing its key".into());
    }
    let key = validate(&account.provider, key)?;
    let document = accounts::read_secret_document(&account.provider, &account.alias)?;
    if !document
        .as_ref()
        .and_then(|value| value.get("api_key"))
        .is_some_and(|value| value.is_string())
    {
        return Err("This account does not use an API key; use Authorize again for OAuth".into());
    }
    drop(vault);
    accounts::replace_secret(&account, &serde_json::json!({"api_key": key}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsupported_providers_and_invalid_header_values() {
        assert!(validate("grok", "fixture").is_err());
        assert!(validate("openai", "").is_err());
        assert!(validate("openai", "fixture\r\nAuthorization: other").is_err());
        assert!(validate("nous", "fixture key").is_err());
        assert!(validate("nous", &"x".repeat(8193)).is_err());
        assert_eq!(
            validate("openai", " fixture-key \n").unwrap(),
            "fixture-key"
        );
    }
}
