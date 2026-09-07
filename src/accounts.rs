//! Host-owned identities. Plugins never own the registry or the secret files.
//!
//! Index: `$XDG_DATA_HOME/spanreed/accounts/index.json`
//! Secrets: OS keyring `(service=spanreed:account:<provider>, user=<alias>)`,
//! falling back to `accounts/<provider>/<alias>.json` (0600).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app;
use crate::secret;

const KEYRING_PREFIX: &str = "spanreed:account";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Registry {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub removed: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Stable id: `{provider}/{alias}` (e.g. `grok/heavy`).
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    pub provider: String,
    pub alias: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub active: bool,
    /// Extra names (`work`) that resolve to this account. Id stays `grok/heavy-1`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    /// Unix ms when used_pct was last fetched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_at: Option<i64>,
    /// Plan charge interval: `month` / `year` (not the weekly usage pool).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing_interval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renews_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_at_period_end: Option<bool>,
    /// Subscriptions endpoint was queried (even if it returned no period).
    #[serde(default)]
    pub billing_checked: bool,
}

impl fabrials_accounts::Steerable for Account {
    fn used_pct(&self) -> Option<f64> {
        self.used_pct
    }
    fn plan_slug(&self) -> Option<&str> {
        self.plan_slug.as_deref()
    }
    fn resets_at(&self) -> Option<&str> {
        self.resets_at.as_deref()
    }
}

impl Account {
    pub fn new(provider: &str, alias: &str) -> Result<Self, String> {
        if !valid_alias(alias) {
            return Err("alias must be [A-Za-z0-9_-]+".into());
        }
        if provider.is_empty() || !valid_alias(provider) {
            return Err("unknown provider".into());
        }
        Ok(Self {
            id: format!("{provider}/{alias}"),
            generation: Some(fabrials_runtime::accounting::new_request_id()),
            provider: provider.into(),
            alias: alias.into(),
            label: alias.into(),
            active: false,
            aliases: Vec::new(),
            plan_slug: None,
            plan_label: None,
            used_pct: None,
            resets_at: None,
            quota_at: None,
            billing_interval: None,
            renews_at: None,
            cancel_at_period_end: None,
            billing_checked: false,
        })
    }
}

/// Next free canonical alias: `heavy-1`, `heavy-2`, …
pub fn unique_alias(provider: &str, base: &str) -> String {
    let taken = taken_names(provider);
    unique_alias_among(&taken, base)
}

pub fn unique_alias_among(taken: &[String], base: &str) -> String {
    let base = if valid_alias(base) { base } else { "acct" };
    for n in 1..100 {
        let cand = format!("{base}-{n}");
        if !taken.iter().any(|a| a == &cand) {
            return cand;
        }
    }
    format!("{base}-x")
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
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let reg = load();
    if let Some(a) = reg.accounts.iter().find(|a| a.id == raw) {
        return Some(a.clone());
    }
    if let Ok((p, al)) = parse_id(raw) {
        if let Some(a) = reg
            .accounts
            .iter()
            .find(|a| a.provider == p && (a.alias == al || a.aliases.iter().any(|n| n == &al)))
        {
            return Some(a.clone());
        }
    }
    reg.accounts
        .into_iter()
        .find(|a| a.aliases.iter().any(|n| n == raw))
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

pub fn valid_alias(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn parse_id(raw: &str) -> Result<(String, String), String> {
    match raw.split_once('/') {
        Some((p, a)) if valid_alias(p) && valid_alias(a) => Ok((p.into(), a.into())),
        _ if valid_alias(raw) => Ok(("grok".into(), raw.into())),
        _ => Err(format!("bad account id: {raw} (want provider/alias)")),
    }
}

fn dir() -> PathBuf {
    app::data_dir().join("accounts")
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
    for provider in ["grok", "nous", "openai"] {
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
    fn commit(
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
fn validate_document(blob: &str) -> Result<serde_json::Value, String> {
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

fn secret_change(
    provider: &str,
    alias: &str,
    contents: Option<String>,
) -> fabrials_runtime::file_set::Change {
    fabrials_runtime::file_set::Change {
        path: format!("{provider}/{alias}.json"),
        contents,
    }
}
fn cache_secret(provider: &str, alias: &str, blob: &str) {
    let service = format!("{KEYRING_PREFIX}:{provider}");
    let label = format!("spanreed {provider}/{alias}");
    let _ = secret::store_user(&service, alias, &label, blob);
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

pub fn apply_snapshot(
    id: &str,
    plan_slug: Option<String>,
    plan_label: Option<String>,
    used_pct: Option<f64>,
    resets_at: Option<String>,
    quota_at: i64,
    billing: Option<PlanBilling>,
) -> Result<Account, String> {
    let vault = lock_vault()?;
    let mut reg = vault.registry()?;
    let Some(existing) = reg.accounts.iter_mut().find(|a| a.id == id) else {
        return Err(format!("unknown account {id}"));
    };
    if plan_slug.is_some() {
        existing.plan_slug = plan_slug;
    }
    if plan_label.is_some() {
        existing.plan_label = plan_label;
    }
    if used_pct.is_some() {
        existing.used_pct = used_pct;
    }
    if resets_at.is_some() {
        existing.resets_at = resets_at;
    }
    existing.quota_at = Some(quota_at);
    if let Some(b) = billing {
        existing.billing_interval = b.interval;
        existing.renews_at = b.renews_at;
        existing.cancel_at_period_end = b.cancel_at_period_end;
        existing.billing_checked = true;
    }
    let out = existing.clone();
    vault.commit(&reg, Vec::new())?;
    Ok(out)
}

#[derive(Debug, Clone, Default)]
pub struct PlanBilling {
    pub interval: Option<String>,
    pub renews_at: Option<String>,
    pub cancel_at_period_end: Option<bool>,
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
            fabrials_runtime::credential_journal::Recovery::Clean => {}
            fabrials_runtime::credential_journal::Recovery::Interrupted => {
                return Err(
                    "Authorize this account again before renaming; its refresh was interrupted"
                        .into(),
                )
            }
            fabrials_runtime::credential_journal::Recovery::Replacement(value) => {
                document = Some(value)
            }
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
        .insert(old_id, fabrials_runtime::accounting::new_request_id());
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
    if acc.active {
        if let Some(next) = reg.accounts.iter_mut().find(|a| a.provider == acc.provider) {
            next.active = true;
        }
    }
    let journal = rotation(&acc.provider, &acc.alias)?;
    reg.removed
        .insert(id.into(), fabrials_runtime::accounting::new_request_id());
    vault.commit(&reg, vec![secret_change(&acc.provider, &acc.alias, None)])?;
    journal.complete()?;
    let _ = secret::delete_user(&format!("{KEYRING_PREFIX}:{}", acc.provider), &acc.alias);
    Ok(())
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

fn get_secret_using(
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

fn rotation(
    provider: &str,
    alias: &str,
) -> Result<fabrials_runtime::credential_journal::Rotation, String> {
    fabrials_runtime::credential_journal::Rotation::acquire(
        &app::data_dir().join("credential-recovery"),
        fabrials_runtime::credential_journal::Scope {
            environment: &app::data_dir().to_string_lossy(),
            owner: "local",
            provider,
            alias,
        },
    )
}

fn file_secret_path(provider: &str, alias: &str) -> PathBuf {
    dir().join(provider).join(format!("{alias}.json"))
}

/// One-shot: lift `grok-accounts/` vault from the prototype into the host registry.
fn migrate_legacy_grok_vault(vault: &Vault) -> Result<(), String> {
    let legacy = app::data_dir().join("grok-accounts");
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
        let mut account = Account::new("grok", id)?;
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

pub fn cmd(args: &[String]) -> std::process::ExitCode {
    match crate::drivers::dispatch_account(args) {
        Ok(out) => {
            if !out.is_empty() {
                print!("{out}");
                if !out.ends_with('\n') {
                    println!();
                }
            }
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_lifecycle_in_isolated_process() {
        let root = std::env::temp_dir().join(format!(
            "spanreed-vault-{}",
            fabrials_runtime::accounting::new_request_id()
        ));
        fs::create_dir_all(&root).unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "accounts::tests::account_lifecycle_fixture",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", &root)
            .env("USERPROFILE", &root)
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("APPDATA", root.join("config"))
            .env("LOCALAPPDATA", root.join("data"))
            .env("SPANREED_OFFLINE", "1")
            .env("SPANREED_VAULT_FIXTURE", "1")
            .output()
            .unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    #[ignore = "Runs in an isolated HOME through account_lifecycle_in_isolated_process"]
    fn account_lifecycle_fixture() {
        assert_eq!(std::env::var("SPANREED_VAULT_FIXTURE").unwrap(), "1");
        {
            let vault = lock_vault().unwrap();
            let ghost = Account::new("grok", "disconnected").unwrap();
            let mut registry = vault.registry().unwrap();
            registry
                .removed
                .insert(ghost.id.clone(), "fixture-disconnection".into());
            registry.accounts.push(ghost);
            vault.commit(&registry, Vec::new()).unwrap();
        }
        assert!(get_secret_using("grok", "disconnected", |_, _| panic!(
            "must not revive keyring credential"
        ))
        .is_none());
        assert_eq!(
            crate::migration::candidates().unwrap()[0].credential_kind,
            fabrials_core::migration::CredentialKind::Unavailable
        );
        {
            let vault = lock_vault().unwrap();
            let mut registry = vault.registry().unwrap();
            registry
                .accounts
                .retain(|account| account.id != "grok/disconnected");
            vault.commit(&registry, Vec::new()).unwrap();
        }

        assert!(!crate::notifications::settings().unwrap().reset_expiry);
        assert_eq!(
            crate::notifications::deliver(|_, _| panic!("notifications default off")).unwrap(),
            0
        );
        crate::notifications::set_enabled(true).unwrap();
        assert!(crate::notifications::settings().unwrap().reset_expiry);
        crate::notifications::set_enabled(false).unwrap();
        let original = serde_json::json!({"access_token":"fixture-old"});
        let replacement = serde_json::json!({"access_token":"fixture-new"});
        {
            let vault = lock_vault().unwrap();
            let inactive = Account::new("nous", "migration-fixture").unwrap();
            vault
                .register_with_activation(inactive, &original.to_string(), None, false)
                .unwrap();
            assert!(
                !vault
                    .registry()
                    .unwrap()
                    .accounts
                    .iter()
                    .find(|a| a.id == "nous/migration-fixture")
                    .unwrap()
                    .active
            );
        }
        remove("nous/migration-fixture").unwrap();
        let account = Account::new("nous", "work").unwrap();
        {
            let vault = lock_vault().unwrap();
            vault
                .register(account.clone(), &original.to_string(), None)
                .unwrap();
            vault
                .register(account.clone(), &original.to_string(), None)
                .unwrap();
            assert!(fabrials_runtime::file_set::FileSet::acquire(&dir()).is_err());
        }
        let journal = rotation("nous", "work").unwrap();
        journal.begin(&original).unwrap();
        journal.rotated(&replacement).unwrap();
        drop(journal);
        let moved = rename("nous", "work", "personal").unwrap();
        assert_eq!(moved.generation, account.generation);
        assert_eq!(
            read_secret_document("nous", "personal").unwrap(),
            Some(replacement.clone())
        );
        assert!(!file_secret_path("nous", "work").exists());
        assert!(replace_secret(&account, &original.to_string()).is_err());
        let journal = rotation("nous", "personal").unwrap();
        journal.begin(&replacement).unwrap();
        drop(journal);
        assert!(rename("nous", "personal", "other").is_err());
        remove("nous/personal").unwrap();
        assert!(!file_secret_path("nous", "personal").exists());
        let vault = lock_vault().unwrap();
        assert!(vault
            .register(moved.clone(), &replacement.to_string(), None)
            .is_err());
        let removal = vault.registry().unwrap().removed.get(&moved.id).cloned();
        let fresh = Account::new("nous", "personal").unwrap();
        vault
            .register(fresh, &original.to_string(), removal.as_ref())
            .unwrap();
        drop(vault);
        assert!(replace_secret(&moved, &replacement.to_string()).is_err());
        assert_eq!(routing_registry().unwrap().accounts.len(), 1);
        let api_account = crate::account_keys::add("openai", "api", "fixture-api-key").unwrap();
        assert!(api_account.active);
        assert!(crate::account_keys::add("openai", "api", "replacement-key").is_err());
        assert_eq!(
            read_secret_document("openai", "api").unwrap().unwrap()["api_key"],
            "fixture-api-key"
        );
        assert!(crate::account_keys::add("unsupported", "other", "fixture").is_err());
        assert_eq!(routing_registry().unwrap().accounts.len(), 2);
        assert!(crate::desktop::models("openai/does-not-exist").is_err());
        replace_authorization(
            &api_account,
            "fixture-reauth-generation",
            r#"{"api_key":"fixture-new-grant"}"#,
        )
        .unwrap();
        put_secret("openai", "api", r#"{"api_key":"fixture-rotated-grant"}"#).unwrap();
        replace_authorization(
            &api_account,
            "fixture-reauth-generation",
            r#"{"api_key":"fixture-new-grant"}"#,
        )
        .unwrap();
        assert_eq!(
            read_secret_document("openai", "api").unwrap().unwrap()["api_key"],
            "fixture-rotated-grant"
        );
        assert!(replace_authorization(
            &api_account,
            "different-attempt",
            r#"{"api_key":"fixture-stale"}"#
        )
        .is_err());
        assert!(crate::account_keys::replace(
            &api_account.id,
            api_account.generation.as_deref(),
            "stale-key"
        )
        .is_err());
        assert!(remove_if_current(&api_account.id, api_account.generation.as_deref()).is_err());
        let current = routing_registry()
            .unwrap()
            .accounts
            .into_iter()
            .find(|a| a.id == api_account.id)
            .unwrap();
        crate::account_keys::replace(
            &current.id,
            current.generation.as_deref(),
            "replacement-api-key",
        )
        .unwrap();
        assert_eq!(
            read_secret_document("openai", "api").unwrap().unwrap()["api_key"],
            "replacement-api-key"
        );
        assert!(remove_if_current(&current.id, current.generation.as_deref()).is_err());
        let current = routing_registry()
            .unwrap()
            .accounts
            .into_iter()
            .find(|a| a.id == api_account.id)
            .unwrap();
        remove_if_current(&current.id, current.generation.as_deref()).unwrap();
        assert!(read_secret_document("openai", "api").unwrap().is_none());
        assert!(crate::account_keys::replace(
            &current.id,
            current.generation.as_deref(),
            "removed-key"
        )
        .is_err());
        let recreated = crate::account_keys::add("openai", "api", "recreated-api-key").unwrap();
        let candidate = crate::migration::candidates()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == recreated.id)
            .unwrap();
        assert_eq!(
            candidate.credential_kind,
            fabrials_core::migration::CredentialKind::ApiKey
        );
        assert_eq!(Some(candidate.generation), recreated.generation);
        assert_ne!(recreated.generation, current.generation);
        assert!(lock_vault()
            .unwrap()
            .registry()
            .unwrap()
            .removed
            .contains_key(&recreated.id));
        remove_if_current(&recreated.id, recreated.generation.as_deref()).unwrap();
        let oauth = routing_registry()
            .unwrap()
            .accounts
            .into_iter()
            .find(|a| a.provider == "nous")
            .unwrap();
        assert!(
            crate::account_keys::replace(&oauth.id, oauth.generation.as_deref(), "api-key")
                .is_err()
        );
        assert_eq!(
            read_secret_document(&oauth.provider, &oauth.alias)
                .unwrap()
                .unwrap(),
            original
        );
    }

    #[test]
    fn routing_registry_rejects_corruption_orphans_and_ambiguous_identity() {
        let directory = std::env::temp_dir().join(format!(
            "spanreed-registry-{}",
            fabrials_runtime::accounting::new_request_id()
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
        let mut account = Account::new("grok", "work").unwrap();
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
        let mut second = Account::new("grok", "personal").unwrap();
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
