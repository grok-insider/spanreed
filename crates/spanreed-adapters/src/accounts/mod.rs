//! Host-owned identities. Plugins never own the registry or the secret files.
//!
//! Index: `$XDG_DATA_HOME/spanreed/accounts/index.json`
//! Secrets: OS keyring `(service=spanreed:account:<provider>, user=<alias>)`,
//! falling back to `accounts/<provider>/<alias>.json` (0600).

use std::fs;
use std::path::{Path, PathBuf};

use crate::product;
use crate::secret;

mod snapshot;
mod vault;

pub(crate) use snapshot::apply_codex_snapshot;
pub use snapshot::{PlanBilling, QuotaSnapshot, apply_snapshot};
pub(crate) use vault::{Vault, lock_vault, replace_authorization};
use vault::{cache_secret, migrate_legacy_grok_vault, rotation, secret_change};

/// Account internals for the cross-module lifecycle fixture (`lifecycle_tests`).
#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) use super::vault::{file_secret_path, get_secret_using, rotation};
    pub(crate) use super::{dir, lock_vault, new_account, rename, replace_authorization};
}
pub use vault::{get_secret, put_secret, read_secret_document, replace_secret};

const KEYRING_PREFIX: &str = "spanreed:account";

pub use fabrials_accounts::{Account, Registry, parse_id, unique_alias_among, valid_alias};

/// A new account with a fresh generation, so a re-added alias is a new identity.
pub fn new_account(provider: &str, alias: &str) -> Result<Account, String> {
    let mut account = Account::new(provider, alias)?;
    account.generation = Some(fabrials_fabric::accounting::new_request_id());
    Ok(account)
}

/// Next free canonical alias: `heavy-1`, `heavy-2`, …
pub fn unique_alias(provider: &str, base: &str) -> String {
    let taken = taken_names(provider);
    unique_alias_among(&taken, base)
}

fn taken_names(provider: &str) -> Vec<String> {
    list_provider(provider)
        .into_iter()
        .flat_map(|a| {
            let mut v = a.aliases;
            v.push(a.alias);
            v
        })
        .collect()
}

/// Look up by id (`grok/heavy-1`), canonical alias, or nickname (`work`).
pub fn resolve(raw: &str) -> Option<Account> {
    load().resolve(raw).cloned()
}

pub fn add_nick(id: &str, nick: &str) -> Result<Account, String> {
    if !valid_alias(nick) {
        return Err("alias must be [A-Za-z0-9_-]+".into());
    }
    let vault = lock_vault()?;
    let mut reg = vault.registry()?;
    if name_conflict(&reg, nick, Some(id)) {
        return Err(format!("name {nick} already in use"));
    }
    let Some(acc) = reg.accounts.iter_mut().find(|a| a.id == id) else {
        return Err(format!("unknown account {id}"));
    };
    if acc.alias == nick || acc.aliases.iter().any(|n| n == nick) {
        return Ok(acc.clone());
    }
    acc.aliases.push(nick.into());
    let out = acc.clone();
    vault.commit(&reg, Vec::new())?;
    Ok(out)
}

pub fn rm_nick(nick: &str) -> Result<Account, String> {
    let vault = lock_vault()?;
    let mut reg = vault.registry()?;
    let Some(acc) = reg
        .accounts
        .iter_mut()
        .find(|a| a.aliases.iter().any(|n| n == nick))
    else {
        return Err(format!("no alias {nick}"));
    };
    acc.aliases.retain(|n| n != nick);
    let out = acc.clone();
    vault.commit(&reg, Vec::new())?;
    Ok(out)
}

fn name_conflict(reg: &Registry, name: &str, except_id: Option<&str>) -> bool {
    reg.accounts.iter().any(|a| {
        if Some(a.id.as_str()) == except_id {
            return false;
        }
        a.alias == name || a.aliases.iter().any(|n| n == name)
    })
}

const GENERIC_ALIASES: &[&str] = &[
    "default",
    "imported",
    "principal",
    "primera",
    "segunda",
    "segunda-2",
    "mine",
    "work",
];

pub fn is_generic_alias(alias: &str) -> bool {
    GENERIC_ALIASES.contains(&alias) || alias.starts_with("imported")
}

pub(crate) fn dir() -> PathBuf {
    product::data_dir().join("accounts")
}

fn index_path() -> PathBuf {
    dir().join("index.json")
}

pub fn load() -> Registry {
    match lock_vault().and_then(|vault| {
        migrate_legacy_grok_vault(&vault)?;
        vault.registry()
    }) {
        Ok(registry) => registry,
        Err(error) => {
            log::warn!("Account registry unavailable: {error}");
            Registry::default()
        }
    }
}

/// Routing must distinguish an empty vault from an unreadable or corrupt one.
/// Unlike interactive loading, this path never migrates or renames accounts.
pub fn routing_registry() -> Result<Registry, String> {
    lock_vault()?.registry()
}

fn routing_registry_at(directory: &Path) -> Result<Registry, String> {
    use std::io::Read;
    let index = directory.join("index.json");
    let registry = match fs::File::open(&index) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "Account registry unavailable")?;
            if bytes.len() > 8 * 1024 * 1024 {
                return Err("Account registry too large".into());
            }
            serde_json::from_slice::<Registry>(&bytes).map_err(|_| "Invalid account registry")?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Registry::default(),
        Err(_) => return Err("Account registry unavailable".into()),
    };
    let mut names = std::collections::HashSet::new();
    let mut active = std::collections::HashSet::new();
    for account in &registry.accounts {
        if !valid_alias(&account.provider)
            || !valid_alias(&account.alias)
            || account.id != format!("{}/{}", account.provider, account.alias)
        {
            return Err("Invalid account identity".into());
        }
        if account.active && !active.insert(&account.provider) {
            return Err("Ambiguous active account".into());
        }
        for alias in std::iter::once(&account.alias).chain(account.aliases.iter()) {
            if !valid_alias(alias) || !names.insert((&account.provider, alias)) {
                return Err("Ambiguous account alias".into());
            }
        }
    }
    for provider in ["grok", "codex", "nous", "openai"] {
        let entries = match fs::read_dir(directory.join(provider)) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("Provider vault unavailable".into()),
        };
        for (count, entry) in entries.enumerate() {
            if count >= 10_000 {
                return Err("Provider vault too large".into());
            }
            let entry = entry.map_err(|_| "Provider vault unavailable")?;
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let alias = path
                .file_stem()
                .and_then(|v| v.to_str())
                .ok_or("Invalid credential path")?;
            if !entry
                .file_type()
                .map_err(|_| "Provider vault unavailable")?
                .is_file()
                || !registry
                    .accounts
                    .iter()
                    .any(|a| a.provider == provider && a.alias == alias)
            {
                return Err("Unregistered provider credential".into());
            }
        }
    }
    Ok(registry)
}

pub fn list_provider(provider: &str) -> Vec<Account> {
    load()
        .accounts
        .into_iter()
        .filter(|a| a.provider == provider)
        .collect()
}

pub fn get(id: &str) -> Option<Account> {
    load().accounts.into_iter().find(|a| a.id == id)
}

pub fn upsert(mut acc: Account) -> Result<Account, String> {
    let vault = lock_vault()?;
    let mut reg = vault.registry()?;
    if let Some(existing) = reg.accounts.iter_mut().find(|a| a.id == acc.id) {
        existing.label = acc.label.clone();
        if acc.plan_slug.is_some() {
            existing.plan_slug = acc.plan_slug.clone();
        }
        if acc.plan_label.is_some() {
            existing.plan_label = acc.plan_label.clone();
        }
        if acc.used_pct.is_some() {
            existing.used_pct = acc.used_pct;
        }
        if acc.resets_at.is_some() {
            existing.resets_at = acc.resets_at.clone();
        }
        if acc.quota_at.is_some() {
            existing.quota_at = acc.quota_at;
        }
        let out = existing.clone();
        vault.commit(&reg, Vec::new())?;
        return Ok(out);
    }
    if !reg
        .accounts
        .iter()
        .any(|a| a.provider == acc.provider && a.active)
    {
        acc.active = true;
    }
    reg.accounts.push(acc.clone());
    vault.commit(&reg, Vec::new())?;
    Ok(acc)
}

/// Move secret + id from `old_alias` to `new_alias` (same provider).
pub fn rename(provider: &str, old_alias: &str, new_alias: &str) -> Result<Account, String> {
    if !valid_alias(new_alias) {
        return Err("alias must be [A-Za-z0-9_-]+".into());
    }
    let old_id = format!("{provider}/{old_alias}");
    let new_id = format!("{provider}/{new_alias}");
    if old_id == new_id {
        return get(&old_id).ok_or_else(|| format!("unknown account {old_id}"));
    }
    let vault = lock_vault()?;
    let mut reg = vault.registry()?;
    if name_conflict(&reg, new_alias, Some(&old_id)) {
        return Err("Account name already in use".into());
    }
    let journal = rotation(provider, old_alias)?;
    let mut document = read_secret_document(provider, old_alias)?;
    if let Some(current) = &document {
        match journal.recover(current)? {
            fabrials_fabric::ports::Recovery::Clean => {}
            fabrials_fabric::ports::Recovery::Interrupted => {
                return Err(
                    "Authorize this account again before renaming; its refresh was interrupted"
                        .into(),
                );
            }
            fabrials_fabric::ports::Recovery::Replacement(value) => document = Some(value),
        }
    }
    let acc = reg
        .accounts
        .iter_mut()
        .find(|a| a.id == old_id)
        .ok_or_else(|| format!("unknown account {old_id}"))?;
    acc.alias = new_alias.into();
    acc.id = new_id;
    acc.aliases.retain(|name| name != new_alias);
    if acc.label == old_alias {
        acc.label = new_alias.into();
    }
    let out = acc.clone();
    reg.removed
        .insert(old_id, fabrials_fabric::accounting::new_request_id());
    let mut changes = Vec::new();
    if let Some(document) = &document {
        changes.push(secret_change(
            provider,
            new_alias,
            Some(document.to_string()),
        ));
    }
    changes.push(secret_change(provider, old_alias, None));
    vault.commit(&reg, changes)?;
    journal.complete()?;
    let _ = secret::delete_user(&format!("{KEYRING_PREFIX}:{provider}"), old_alias);
    if let Some(document) = document {
        cache_secret(provider, new_alias, &document.to_string());
    }

    Ok(out)
}

pub fn set_active(id: &str) -> Result<Account, String> {
    let vault = lock_vault()?;
    let mut reg = vault.registry()?;
    let acc = reg
        .accounts
        .iter()
        .find(|a| a.id == id)
        .cloned()
        .ok_or_else(|| format!("unknown account {id}"))?;
    for a in &mut reg.accounts {
        if a.provider == acc.provider {
            a.active = a.id == id;
        }
    }
    vault.commit(&reg, Vec::new())?;
    Ok(acc)
}

pub fn active(provider: &str) -> Option<Account> {
    let reg = load();
    reg.accounts
        .iter()
        .find(|a| a.provider == provider && a.active)
        .cloned()
        .or_else(|| reg.accounts.into_iter().find(|a| a.provider == provider))
}

pub fn remove(id: &str) -> Result<(), String> {
    remove_inner(id, None)
}

pub fn remove_if_current(id: &str, generation: Option<&str>) -> Result<(), String> {
    remove_inner(id, Some(generation))
}

fn remove_inner(id: &str, expected: Option<Option<&str>>) -> Result<(), String> {
    let vault = lock_vault()?;
    let mut reg = vault.registry()?;
    let Some(pos) = reg.accounts.iter().position(|a| a.id == id) else {
        return Err(format!("unknown account {id}"));
    };
    if expected.is_some_and(|generation| reg.accounts[pos].generation.as_deref() != generation) {
        return Err("Account changed; refresh Accounts before removing it".into());
    }
    let acc = reg.accounts.remove(pos);
    if acc.active
        && let Some(next) = reg.accounts.iter_mut().find(|a| a.provider == acc.provider)
    {
        next.active = true;
    }
    let journal = rotation(&acc.provider, &acc.alias)?;
    reg.removed
        .insert(id.into(), fabrials_fabric::accounting::new_request_id());
    vault.commit(&reg, vec![secret_change(&acc.provider, &acc.alias, None)])?;
    journal.complete()?;
    let _ = secret::delete_user(&format!("{KEYRING_PREFIX}:{}", acc.provider), &acc.alias);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accounts_get_a_fresh_generation_on_the_shared_type() {
        let first = new_account("grok", "heavy-1").unwrap();
        let second = new_account("grok", "heavy-1").unwrap();
        assert_eq!(first.id, "grok/heavy-1");
        assert!(first.generation.is_some());
        assert_ne!(first.generation, second.generation);
        let plain: fabrials_accounts::Account = first.clone();
        assert_eq!(plain, first);
        assert!(new_account("grok", "bad alias").is_err());
    }

    #[test]
    fn the_shared_id_rules_apply() {
        assert_eq!(parse_id("heavy").unwrap(), ("grok".into(), "heavy".into()));
        assert_eq!(unique_alias_among(&["heavy-1".into()], "heavy"), "heavy-2");
        assert!(!valid_alias(""));
    }

    #[test]
    fn routing_registry_rejects_corruption_orphans_and_ambiguous_identity() {
        let directory = std::env::temp_dir().join(format!(
            "spanreed-registry-{}",
            fabrials_fabric::accounting::new_request_id()
        ));
        fs::create_dir_all(directory.join("grok")).unwrap();
        assert!(routing_registry_at(&directory).unwrap().accounts.is_empty());
        let index = directory.join("index.json");
        fs::write(&index, b"invalid").unwrap();
        assert!(routing_registry_at(&directory).is_err());
        assert_eq!(fs::read(&index).unwrap(), b"invalid");
        fs::remove_file(&index).unwrap();
        let secret = directory.join("grok/work.json");
        fs::write(&secret, b"{}").unwrap();
        assert!(routing_registry_at(&directory).is_err());
        let mut account = new_account("grok", "work").unwrap();
        let write = |accounts: Vec<Account>| {
            fs::write(
                &index,
                serde_json::json!({"accounts": accounts}).to_string(),
            )
            .unwrap();
        };
        write(vec![account.clone()]);
        assert_eq!(routing_registry_at(&directory).unwrap().accounts.len(), 1);
        account.aliases.push("work".into());
        write(vec![account.clone()]);
        assert!(routing_registry_at(&directory).is_err());
        account.aliases.clear();
        account.active = true;
        let mut second = new_account("grok", "personal").unwrap();
        second.active = true;
        write(vec![account.clone(), second]);
        assert!(routing_registry_at(&directory).is_err());
        account.id = "grok/someone-else".into();
        write(vec![account]);
        assert!(routing_registry_at(&directory).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn ids() {
        assert_eq!(
            parse_id("grok/heavy").unwrap(),
            ("grok".into(), "heavy".into())
        );
        assert_eq!(
            parse_id("principal").unwrap(),
            ("grok".into(), "principal".into())
        );
        assert!(parse_id("../x").is_err());
        assert!(valid_alias("work_1"));
        assert!(valid_alias("premium-plus"));
        assert!(!valid_alias("a/b"));
    }

    #[test]
    fn numbered_ids_start_at_one() {
        let taken = vec!["premium".into()];
        assert_eq!(unique_alias_among(&taken, "heavy"), "heavy-1");
        let taken = vec!["heavy-1".into()];
        assert_eq!(unique_alias_among(&taken, "heavy"), "heavy-2");
    }
}
