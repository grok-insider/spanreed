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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Stable id: `{provider}/{alias}` (e.g. `grok/heavy`).
    pub id: String,
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
    let mut reg = load();
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
    save(&reg)?;
    Ok(out)
}

pub fn rm_nick(nick: &str) -> Result<Account, String> {
    let mut reg = load();
    let Some(acc) = reg
        .accounts
        .iter_mut()
        .find(|a| a.aliases.iter().any(|n| n == nick))
    else {
        return Err(format!("no alias {nick}"));
    };
    acc.aliases.retain(|n| n != nick);
    let out = acc.clone();
    save(&reg)?;
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
    migrate_legacy_grok_vault();
    let mut reg = read_raw();
    if canonicalize_ids(&mut reg) {
        let _ = save(&reg);
    }
    reg
}

fn read_raw() -> Registry {
    let text = match fs::read_to_string(index_path()) {
        Ok(t) => t,
        Err(_) => return Registry::default(),
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn is_numbered_plan_id(alias: &str) -> bool {
    let Some((base, n)) = alias.rsplit_once('-') else {
        return false;
    };
    n.chars().all(|c| c.is_ascii_digit())
        && matches!(
            base,
            "heavy" | "plus" | "normal" | "lite" | "premium" | "premium-plus" | "acct"
        )
}

fn canonicalize_ids(reg: &mut Registry) -> bool {
    let mut dirty = false;
    for acc in &mut reg.accounts {
        if acc.provider != "grok" {
            continue;
        }
        if let Some(label) = acc.plan_label.clone() {
            let slug = crate::drivers::grok::classify_plan(&label).0;
            if acc.plan_slug.as_deref() != Some(slug) {
                acc.plan_slug = Some(slug.to_string());
                dirty = true;
            }
        }
    }
    let mut taken: Vec<String> = reg
        .accounts
        .iter()
        .flat_map(|a| {
            let mut v = a.aliases.clone();
            v.push(a.alias.clone());
            v
        })
        .collect();
    let mut moves: Vec<(usize, String)> = Vec::new();
    for (i, acc) in reg.accounts.iter().enumerate() {
        if acc.provider != "grok" || is_numbered_plan_id(&acc.alias) {
            continue;
        }
        let slug = acc.plan_slug.as_deref().unwrap_or("acct");
        let should = is_generic_alias(&acc.alias)
            || acc.alias == slug
            || matches!(
                acc.alias.as_str(),
                "heavy" | "plus" | "normal" | "lite" | "premium" | "premium-plus"
            );
        if !should {
            continue;
        }
        let new = unique_alias_among(&taken, slug);
        if new != acc.alias {
            moves.push((i, new.clone()));
            taken.push(new);
        }
    }
    for (i, new) in moves {
        let old = reg.accounts[i].alias.clone();
        let provider = reg.accounts[i].provider.clone();
        if let Some(blob) = get_secret(&provider, &old) {
            let _ = put_secret(&provider, &new, &blob);
            delete_secret(&provider, &old);
        }
        let acc = &mut reg.accounts[i];
        acc.alias = new.clone();
        acc.id = format!("{provider}/{new}");
        if acc.label == old {
            acc.label = acc.plan_label.clone().unwrap_or(new);
        }
        dirty = true;
    }
    dirty
}

pub fn save(reg: &Registry) -> Result<(), String> {
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(reg).map_err(|e| e.to_string())?;
    write_0600(&index_path(), text.as_bytes())
}

fn write_0600(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    fs::write(path, bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
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
    let mut reg = load();
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
        save(&reg)?;
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
    save(&reg)?;
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
    let mut reg = load();
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
    save(&reg)?;
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
    if get(&new_id).is_some() {
        return Err(format!("{new_id} already exists"));
    }
    let blob = get_secret(provider, old_alias);
    let mut reg = load();
    let Some(acc) = reg.accounts.iter_mut().find(|a| a.id == old_id) else {
        return Err(format!("unknown account {old_id}"));
    };
    acc.alias = new_alias.into();
    acc.id = new_id.clone();
    if acc.label == old_alias {
        acc.label = new_alias.into();
    }
    let out = acc.clone();
    save(&reg)?;
    if let Some(b) = blob {
        let _ = put_secret(provider, new_alias, &b);
        delete_secret(provider, old_alias);
    }
    Ok(out)
}

pub fn set_active(id: &str) -> Result<Account, String> {
    let mut reg = load();
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
    save(&reg)?;
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
    let mut reg = load();
    let Some(pos) = reg.accounts.iter().position(|a| a.id == id) else {
        return Err(format!("unknown account {id}"));
    };
    let acc = reg.accounts.remove(pos);
    if acc.active {
        if let Some(next) = reg.accounts.iter_mut().find(|a| a.provider == acc.provider) {
            next.active = true;
        }
    }
    delete_secret(&acc.provider, &acc.alias);
    save(&reg)
}

pub fn put_secret(provider: &str, alias: &str, blob: &str) -> Result<(), String> {
    // File is authoritative (0600). Keyring is best-effort extra.
    write_0600(&file_secret_path(provider, alias), blob.as_bytes())?;
    let service = format!("{KEYRING_PREFIX}:{provider}");
    let label = format!("spanreed {provider}/{alias}");
    let _ = secret::store_user(&service, alias, &label, blob);
    Ok(())
}

pub fn get_secret(provider: &str, alias: &str) -> Option<String> {
    let from_file = fs::read_to_string(file_secret_path(provider, alias))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if from_file.is_some() {
        return from_file;
    }
    let service = format!("{KEYRING_PREFIX}:{provider}");
    secret::lookup_user(&service, alias)
}

fn delete_secret(provider: &str, alias: &str) {
    let service = format!("{KEYRING_PREFIX}:{provider}");
    let _ = secret::delete_user(&service, alias);
    let _ = fs::remove_file(file_secret_path(provider, alias));
}

fn file_secret_path(provider: &str, alias: &str) -> PathBuf {
    dir().join(provider).join(format!("{alias}.json"))
}

/// One-shot: lift `grok-accounts/` vault from the prototype into the host registry.
fn migrate_legacy_grok_vault() {
    let legacy = app::data_dir().join("grok-accounts");
    let idx = legacy.join("index.json");
    if !idx.exists() || index_path().exists() {
        return;
    }
    let Ok(text) = fs::read_to_string(&idx) else {
        return;
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut reg = Registry::default();
    let active = v.get("active").and_then(|x| x.as_str()).map(str::to_string);
    if let Some(arr) = v.get("accounts").and_then(|a| a.as_array()) {
        for item in arr {
            let Some(id) = item.get("id").and_then(|x| x.as_str()) else {
                continue;
            };
            let Ok(mut acc) = Account::new("grok", id) else {
                continue;
            };
            acc.active = active.as_deref() == Some(id);
            let auth = legacy.join(id).join("auth.json");
            if let Ok(blob) = fs::read_to_string(&auth) {
                let _ = put_secret("grok", id, &blob);
            }
            reg.accounts.push(acc);
        }
    }
    if !reg.accounts.is_empty() {
        let _ = save(&reg);
    }
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
