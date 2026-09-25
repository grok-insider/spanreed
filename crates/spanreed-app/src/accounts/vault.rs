//! Account secrets: the locked registry/secret transaction (`Vault`), the OS
//! keyring with a 0600 file fallback, and credential rotation journals.

use std::fs;
use std::path::PathBuf;

use super::{
    Account, KEYRING_PREFIX, Registry, dir, index_path, name_conflict, new_account,
    routing_registry_at, valid_alias,
};
use crate::product;
use crate::secret;

pub(crate) struct Vault {
    files: fabrials_runtime::file_set::FileSet,
}

pub(crate) fn lock_vault() -> Result<Vault, String> {
    Ok(Vault {
        files: fabrials_runtime::file_set::FileSet::acquire_wait(&dir())?,
    })
}

impl Vault {
    pub(crate) fn registry(&self) -> Result<Registry, String> {
        routing_registry_at(&dir())
    }
    pub(crate) fn commit(
        &self,
        registry: &Registry,
        mut changes: Vec<fabrials_runtime::file_set::Change>,
    ) -> Result<(), String> {
        changes.push(fabrials_runtime::file_set::Change {
            path: "index.json".into(),
            contents: Some(serde_json::to_string_pretty(registry).map_err(|_| "Invalid registry")?),
        });
        self.files.commit(changes)
    }
    pub(crate) fn write_secret(
        &self,
        provider: &str,
        alias: &str,
        blob: &str,
    ) -> Result<(), String> {
        if !valid_alias(provider) || !valid_alias(alias) {
            return Err("Invalid account identity".into());
        }
        if !self
            .registry()?
            .accounts
            .iter()
            .any(|account| account.id == format!("{provider}/{alias}"))
        {
            return Err("Account no longer exists".into());
        }
        self.files
            .commit(vec![secret_change(provider, alias, Some(blob.to_string()))])?;
        cache_secret(provider, alias, blob);
        Ok(())
    }
    pub(crate) fn register(
        &self,
        account: Account,
        blob: &str,
        expected_removal: Option<&String>,
    ) -> Result<(), String> {
        self.register_with_activation(account, blob, expected_removal, true)
    }
    pub(crate) fn register_with_activation(
        &self,
        mut account: Account,
        blob: &str,
        expected_removal: Option<&String>,
        activate_first: bool,
    ) -> Result<(), String> {
        if !valid_alias(&account.provider)
            || !valid_alias(&account.alias)
            || account.id != format!("{}/{}", account.provider, account.alias)
            || account.aliases.iter().any(|name| !valid_alias(name))
        {
            return Err("Invalid account identity".into());
        }
        validate_document(blob)?;
        let mut registry = self.registry()?;
        if registry.removed.get(&account.id) != expected_removal {
            return Err("Account was removed during authorization".into());
        }
        if let Some(existing) = registry
            .accounts
            .iter()
            .find(|entry| entry.id == account.id)
        {
            if account.generation.is_some() && existing.generation == account.generation {
                return Ok(());
            }
            return Err("Account name is already in use".into());
        }
        if name_conflict(&registry, &account.alias, None)
            || account
                .aliases
                .iter()
                .any(|name| name_conflict(&registry, name, None))
        {
            return Err("Account name is already in use".into());
        }
        account.active = activate_first
            && !registry
                .accounts
                .iter()
                .any(|entry| entry.provider == account.provider && entry.active);
        registry.accounts.push(account.clone());
        self.commit(
            &registry,
            vec![secret_change(
                &account.provider,
                &account.alias,
                Some(blob.to_string()),
            )],
        )?;
        cache_secret(&account.provider, &account.alias, blob);
        Ok(())
    }
}

pub(super) fn validate_document(blob: &str) -> Result<serde_json::Value, String> {
    if blob.len() > 1024 * 1024 {
        return Err("Credential document too large".into());
    }
    let document: serde_json::Value =
        serde_json::from_str(blob).map_err(|_| "Invalid credential document")?;
    if !document.is_object() {
        return Err("Invalid credential document".into());
    }
    Ok(document)
}

pub(super) fn secret_change(
    provider: &str,
    alias: &str,
    contents: Option<String>,
) -> fabrials_runtime::file_set::Change {
    fabrials_runtime::file_set::Change {
        path: format!("{provider}/{alias}.json"),
        contents,
    }
}

pub(super) fn cache_secret(provider: &str, alias: &str, blob: &str) {
    let service = format!("{KEYRING_PREFIX}:{provider}");
    let label = format!("spanreed {provider}/{alias}");
    let _ = secret::store_user(&service, alias, &label, blob);
}

pub fn put_secret(provider: &str, alias: &str, blob: &str) -> Result<(), String> {
    lock_vault()?.write_secret(provider, alias, blob)
}

pub fn replace_secret(account: &Account, blob: &str) -> Result<(), String> {
    replace_authorization(
        account,
        &fabrials_runtime::accounting::new_request_id(),
        blob,
    )
}

pub(crate) fn replace_authorization(
    account: &Account,
    generation: &str,
    blob: &str,
) -> Result<(), String> {
    validate_document(blob)?;
    if generation.is_empty() {
        return Err("Invalid authorization generation".into());
    }
    let vault = lock_vault()?;
    let mut registry = vault.registry()?;
    let current = registry
        .accounts
        .iter_mut()
        .find(|current| current.id == account.id)
        .ok_or("Account changed during authorization")?;
    // A retry after journal replay must not replace a grant that has since rotated.
    if current.generation.as_deref() == Some(generation) {
        return Ok(());
    }
    if current.generation != account.generation {
        return Err("Account changed during authorization".into());
    }
    current.generation = Some(generation.into());
    vault.commit(
        &registry,
        vec![secret_change(
            &account.provider,
            &account.alias,
            Some(blob.into()),
        )],
    )?;
    cache_secret(&account.provider, &account.alias, blob);
    Ok(())
}

pub fn get_secret(provider: &str, alias: &str) -> Option<String> {
    get_secret_using(provider, alias, secret::lookup_user)
}

pub(crate) fn get_secret_using(
    provider: &str,
    alias: &str,
    lookup: impl FnOnce(&str, &str) -> Option<String>,
) -> Option<String> {
    let vault = lock_vault().ok()?;
    let registry = vault.registry().ok()?;
    let id = format!("{provider}/{alias}");
    if !registry.accounts.iter().any(|account| account.id == id) {
        return None;
    }
    let from_file = fs::read_to_string(file_secret_path(provider, alias))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if from_file.is_some() {
        return from_file;
    }
    // A retained metrics row must not revive a disconnected keyring credential.
    if registry.removed.contains_key(&id) {
        return None;
    }
    let service = format!("{KEYRING_PREFIX}:{provider}");
    lookup(&service, alias)
}

/// Refresh reads must distinguish missing files from inaccessible/corrupt state.
pub fn read_secret_document(
    provider: &str,
    alias: &str,
) -> Result<Option<serde_json::Value>, String> {
    use std::io::Read;
    if !valid_alias(provider) || !valid_alias(alias) {
        return Err("Invalid credential identity".into());
    }
    let file = match fs::File::open(file_secret_path(provider, alias)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Credential file unavailable".into()),
    };
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Credential file unavailable")?;
    if bytes.len() > 1024 * 1024 {
        return Err("Credential document too large".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "Invalid credential document".into())
}

pub(crate) fn rotation(
    provider: &str,
    alias: &str,
) -> Result<fabrials_runtime::credential_journal::Rotation, String> {
    fabrials_runtime::credential_journal::Rotation::acquire(
        &product::data_dir().join("credential-recovery"),
        fabrials_runtime::credential_journal::Scope {
            environment: &product::data_dir().to_string_lossy(),
            owner: "local",
            provider,
            alias,
        },
    )
}

pub(crate) fn file_secret_path(provider: &str, alias: &str) -> PathBuf {
    dir().join(provider).join(format!("{alias}.json"))
}

/// One-shot: lift `grok-accounts/` vault from the prototype into the host registry.
pub(super) fn migrate_legacy_grok_vault(vault: &Vault) -> Result<(), String> {
    let legacy = product::data_dir().join("grok-accounts");
    let index = legacy.join("index.json");
    if !index.exists() || index_path().exists() {
        return Ok(());
    }
    let text = fs::read_to_string(index).map_err(|_| "Legacy account registry unavailable")?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "Invalid legacy account registry")?;
    let mut registry = Registry::default();
    let mut changes = Vec::new();
    for item in value
        .get("accounts")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
    {
        let id = item
            .get("id")
            .and_then(|value| value.as_str())
            .ok_or("Invalid legacy account identity")?;
        let mut account = new_account("grok", id)?;
        account.active = value.get("active").and_then(|value| value.as_str()) == Some(id);
        let blob = fs::read_to_string(legacy.join(id).join("auth.json"))
            .map_err(|_| "Legacy credential unavailable")?;
        changes.push(secret_change("grok", id, Some(blob)));
        registry.accounts.push(account);
    }
    if !registry.accounts.is_empty() {
        vault.commit(&registry, changes)?;
    }
    Ok(())
}
