//! Quota, plan and billing snapshots stored on registry entries.

use super::{Account, lock_vault, read_secret_document};

pub(crate) fn apply_codex_snapshot(
    account: &Account,
    expected: &serde_json::Value,
    output: &crate::model::ProviderOutput,
    now: i64,
) -> Result<(), String> {
    let vault = lock_vault()?;
    let mut registry = vault.registry()?;
    let existing = registry
        .accounts
        .iter_mut()
        .find(|candidate| candidate.id == account.id && candidate.generation == account.generation)
        .ok_or("Account changed during quota refresh")?;
    if read_secret_document("codex", &account.alias)?.as_ref() != Some(expected) {
        return Err("Authorization changed during quota refresh".into());
    }
    let primary = output
        .lines
        .iter()
        .filter_map(|line| match line {
            crate::model::MetricLine::Progress {
                label,
                used,
                resets_at,
                ..
            } if !label.starts_with("Review ") => Some((*used, resets_at.clone())),
            _ => None,
        })
        .max_by(|a, b| a.0.total_cmp(&b.0));
    existing.used_pct = primary.as_ref().map(|value| value.0);
    existing.resets_at = primary.and_then(|value| value.1);
    existing.plan_slug = output.plan.clone();
    existing.plan_label = output.plan.clone();
    existing.quota_at = Some(now);
    vault.commit(&registry, Vec::new())
}

/// Plan and quota observed for one account; `None` fields keep stored values.
#[derive(Debug, Clone, Default)]
pub struct QuotaSnapshot {
    pub plan_slug: Option<String>,
    pub plan_label: Option<String>,
    pub used_pct: Option<f64>,
    pub resets_at: Option<String>,
    pub quota_at: i64,
    pub billing: Option<PlanBilling>,
}

pub fn apply_snapshot(id: &str, snapshot: QuotaSnapshot) -> Result<Account, String> {
    let QuotaSnapshot {
        plan_slug,
        plan_label,
        used_pct,
        resets_at,
        quota_at,
        billing,
    } = snapshot;
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
