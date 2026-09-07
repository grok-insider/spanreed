//! Host-owned identities. Plugins never own the registry.

mod steer;
pub mod transfer;

pub use steer::{
    cursor_plan_rank, deadline_first_score, format_reset_in, grok_burn_rank, grok_plan_rank,
    hours_to_reset, parse_rfc3339_ms, pick_autosteer, pick_deadline_autosteer, plan_first_score,
    Steerable, DEFAULT_EXHAUSTED_PCT, GROK_PLAN_RANK_MAX, URGENT_RESET_HOURS,
};

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Registry {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub removed: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    pub provider: String,
    pub alias: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub active: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub billing_interval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renews_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_at_period_end: Option<bool>,
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
            generation: None,
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

impl Registry {
    pub fn resolve(&self, raw: &str) -> Option<&Account> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        if let Some(a) = self.accounts.iter().find(|a| a.id == raw) {
            return Some(a);
        }
        if let Ok((p, al)) = parse_id(raw) {
            if let Some(a) = self
                .accounts
                .iter()
                .find(|a| a.provider == p && (a.alias == al || a.aliases.iter().any(|n| n == &al)))
            {
                return Some(a);
            }
        }
        self.accounts
            .iter()
            .find(|a| a.aliases.iter().any(|n| n == raw))
    }

    pub fn active(&self, provider: &str) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|a| a.provider == provider && a.active)
            .or_else(|| self.accounts.iter().find(|a| a.provider == provider))
    }

    pub fn list_provider(&self, provider: &str) -> Vec<&Account> {
        self.accounts
            .iter()
            .filter(|a| a.provider == provider)
            .collect()
    }

    pub fn upsert(&mut self, acc: Account) {
        if let Some(i) = self.accounts.iter().position(|a| a.id == acc.id) {
            self.accounts[i] = acc;
        } else {
            self.accounts.push(acc);
        }
    }
}

pub fn load(path: &Path) -> Registry {
    let Ok(raw) = fs::read_to_string(path) else {
        return Registry::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(path: &Path, reg: &Registry) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(reg).map_err(|e| e.to_string())?;
    fs::write(path, body).map_err(|e| e.to_string())
}

pub fn index_path(data_dir: &Path) -> PathBuf {
    data_dir.join("accounts").join("index.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_alias_skips_taken() {
        let taken = vec!["heavy-1".into()];
        assert_eq!(unique_alias_among(&taken, "heavy"), "heavy-2");
    }

    #[test]
    fn parse_id_defaults_grok() {
        assert_eq!(parse_id("work").unwrap(), ("grok".into(), "work".into()));
        assert_eq!(
            parse_id("grok/heavy-1").unwrap(),
            ("grok".into(), "heavy-1".into())
        );
        assert!(parse_id("bad alias!").is_err());
    }

    #[test]
    fn resolve_by_id_and_nick() {
        let mut reg = Registry::default();
        let mut a = Account::new("grok", "heavy-1").unwrap();
        a.aliases.push("work".into());
        a.used_pct = Some(22.0);
        reg.upsert(a);
        assert_eq!(reg.resolve("grok/heavy-1").unwrap().alias, "heavy-1");
        assert_eq!(reg.resolve("work").unwrap().alias, "heavy-1");
        assert!(reg.resolve("missing").is_none());
    }
}
