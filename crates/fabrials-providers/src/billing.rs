//! Subscription billing: whether a plan renews on its own and when it renews or
//! ends. Each provider reports a different subset; fields it does not report
//! stay `None` and are never guessed.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanBilling {
    /// Provider status in lowercase words, for example `active` or `canceled`.
    pub status: Option<String>,
    /// `month` or `year`.
    pub interval: Option<String>,
    /// `Some(false)` when the plan is set to end instead of renewing.
    pub auto_renew: Option<bool>,
    /// Next charge, when the plan renews on its own.
    pub renews_at: Option<String>,
    /// When access ends, when the plan will not renew.
    pub ends_at: Option<String>,
    /// End of the paid period when the provider does not say whether it renews.
    pub paid_through: Option<String>,
}

impl PlanBilling {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty() && text.len() <= 64 && !text.chars().any(char::is_control))
        .map(str::to_owned)
}

fn interval(raw: Option<String>) -> Option<String> {
    let raw = raw?.to_ascii_lowercase();
    Some(
        if raw.contains("year") || raw.contains("annual") {
            "year"
        } else if raw.contains("month") {
            "month"
        } else {
            return None;
        }
        .into(),
    )
}

fn status(raw: Option<String>) -> Option<String> {
    let raw = raw?.to_ascii_lowercase();
    let raw = raw.trim_start_matches("subscription_status_");
    Some(raw.replace('_', " "))
}

/// Settle renewal and end dates once `auto_renew` and the period end are known.
fn with_period(mut billing: PlanBilling, period_end: Option<String>) -> PlanBilling {
    match billing.auto_renew {
        Some(true) => billing.renews_at = billing.renews_at.or(period_end),
        Some(false) => billing.ends_at = billing.ends_at.or(period_end),
        None => billing.paid_through = billing.paid_through.or(period_end),
    }
    billing
}

/// ChatGPT `GET /backend-api/subscriptions?account_id=…` (Codex OAuth token).
pub fn codex_subscriptions(value: &Value) -> Option<PlanBilling> {
    let entitlement = value.get("entitlement").unwrap_or(&Value::Null);
    let field = |key: &str| value.get(key).or_else(|| entitlement.get(key));
    let will_renew = field("will_renew").and_then(Value::as_bool);
    let cancels_at = text(field("cancels_at"));
    let auto_renew = match (will_renew, &cancels_at) {
        (_, Some(_)) => Some(false),
        (renew, None) => renew,
    };
    let active = field("is_active")
        .or_else(|| entitlement.get("has_active_subscription"))
        .and_then(Value::as_bool);
    let billing = PlanBilling {
        status: active.map(|active| if active { "active" } else { "inactive" }.into()),
        interval: interval(text(field("billing_period"))),
        auto_renew,
        renews_at: if auto_renew == Some(true) {
            text(field("renews_at"))
        } else {
            None
        },
        ends_at: if auto_renew == Some(false) {
            cancels_at.or_else(|| text(field("active_until")))
        } else {
            None
        },
        paid_through: None,
    };
    let billing = with_period(
        billing,
        text(field("active_until")).or_else(|| text(field("expires_at"))),
    );
    (!billing.is_empty()).then_some(billing)
}

/// Paid-through date from the ChatGPT claims in a Codex `id_token`, for when the
/// subscriptions endpoint is unreachable. It does not say whether it renews.
pub fn codex_id_token(claims: &Value) -> Option<PlanBilling> {
    let auth = claims.get("https://api.openai.com/auth")?;
    let billing = PlanBilling {
        paid_through: text(auth.get("chatgpt_subscription_active_until")),
        ..Default::default()
    };
    (!billing.is_empty()).then_some(billing)
}

/// `GET grok.com/rest/subscriptions` with a Grok CLI session. The account's
/// active SuperGrok subscription wins over an X Premium one, which X bills and
/// reports no dates for.
pub fn grok_subscriptions(value: &Value) -> Option<PlanBilling> {
    let rows = value
        .get("subscriptions")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())?;
    let score = |row: &Value| {
        let tier = row.get("tier").and_then(Value::as_str).unwrap_or_default();
        if tier.contains("SUPER_GROK") {
            3
        } else if tier.contains("GROK") {
            2
        } else if tier.contains("PREMIUM") {
            1
        } else {
            0
        }
    };
    let row = rows
        .iter()
        .take(64)
        .filter(|row| {
            row.get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.ends_with("_ACTIVE") || status == "ACTIVE")
        })
        .max_by_key(|row| score(row))?;
    let cancel = row.get("cancelAtPeriodEnd").and_then(Value::as_bool);
    let billing = PlanBilling {
        status: status(text(row.get("status"))),
        interval: interval(text(row.get("billingInterval"))),
        auto_renew: cancel.map(|cancel| !cancel),
        ..Default::default()
    };
    Some(with_period(
        billing,
        text(row.get("billingPeriodEnd")).or_else(|| text(row.get("currentPeriodEnd"))),
    ))
}

/// Claude's OAuth profile only carries the subscription status; renewal and end
/// dates are not available to an OAuth session.
pub fn claude_profile(profile: &Value) -> Option<PlanBilling> {
    let organization = profile.get("organization").unwrap_or(profile);
    let billing = PlanBilling {
        status: status(text(organization.get("subscription_status"))),
        ..Default::default()
    };
    (!billing.is_empty()).then_some(billing)
}

/// Nous portal account: a paid plan's period end. Whether it renews is not
/// reported; the free plan has no billing period.
pub fn nous_account(account: &Value) -> Option<PlanBilling> {
    let subscription = account.get("subscription")?;
    let charge = subscription
        .get("monthly_charge")
        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(0.0);
    if charge <= 0.0 {
        return None;
    }
    Some(PlanBilling {
        status: Some("active".into()),
        interval: Some("month".into()),
        paid_through: text(subscription.get("current_period_end")),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn codex_reports_renewal_and_a_cancelled_plan_end() {
        let renewing = json!({
            "plan_type": "pro", "is_active": true, "will_renew": true,
            "renews_at": "2026-10-06T11:04:15+00:00", "cancels_at": null,
            "active_until": "2026-10-06T11:04:15Z", "billing_period": "monthly",
            "entitlement": {"subscription_plan": "chatgptpro", "expires_at": "2026-10-06T17:04:15+00:00"}
        });
        assert_eq!(
            codex_subscriptions(&renewing).unwrap(),
            PlanBilling {
                status: Some("active".into()),
                interval: Some("month".into()),
                auto_renew: Some(true),
                renews_at: Some("2026-10-06T11:04:15+00:00".into()),
                ends_at: None,
                paid_through: None,
            }
        );
        let cancelled = json!({
            "is_active": true, "will_renew": false, "cancels_at": "2026-10-06T11:04:15+00:00",
            "active_until": "2026-10-06T11:04:15Z", "billing_period": "yearly"
        });
        let billing = codex_subscriptions(&cancelled).unwrap();
        assert_eq!(billing.auto_renew, Some(false));
        assert_eq!(
            billing.ends_at.as_deref(),
            Some("2026-10-06T11:04:15+00:00")
        );
        assert_eq!(billing.interval.as_deref(), Some("year"));
        assert!(billing.renews_at.is_none());
    }

    #[test]
    fn a_codex_token_only_knows_the_paid_through_date() {
        let claims = json!({"https://api.openai.com/auth": {
            "chatgpt_plan_type": "pro",
            "chatgpt_subscription_active_until": "2026-10-06T11:04:15+00:00"
        }});
        let billing = codex_id_token(&claims).unwrap();
        assert_eq!(
            billing.paid_through.as_deref(),
            Some("2026-10-06T11:04:15+00:00")
        );
        assert_eq!(billing.auto_renew, None);
        assert!(codex_id_token(&json!({})).is_none());
    }

    #[test]
    fn grok_picks_the_active_supergrok_and_reads_its_renewal() {
        let value = json!({"subscriptions": [
            {"tier": "SUBSCRIPTION_TIER_GROK_PRO", "status": "SUBSCRIPTION_STATUS_INACTIVE",
             "billingInterval": "BILLING_INTERVAL_MONTHLY", "billingPeriodEnd": "2026-09-16T15:58:18Z", "cancelAtPeriodEnd": false},
            {"tier": "SUBSCRIPTION_TIER_SUPER_GROK_PRO", "status": "SUBSCRIPTION_STATUS_ACTIVE",
             "billingInterval": "BILLING_INTERVAL_YEARLY", "billingPeriodEnd": "2027-08-17T06:42:46Z", "cancelAtPeriodEnd": false},
            {"tier": "SUBSCRIPTION_TIER_X_PREMIUM", "status": "SUBSCRIPTION_STATUS_ACTIVE"}
        ]});
        let billing = grok_subscriptions(&value).unwrap();
        assert_eq!(billing.status.as_deref(), Some("active"));
        assert_eq!(billing.interval.as_deref(), Some("year"));
        assert_eq!(billing.auto_renew, Some(true));
        assert_eq!(billing.renews_at.as_deref(), Some("2027-08-17T06:42:46Z"));
        let cancelling = json!({"subscriptions": [
            {"tier": "SUBSCRIPTION_TIER_SUPER_GROK_PRO", "status": "SUBSCRIPTION_STATUS_ACTIVE",
             "billingInterval": "BILLING_INTERVAL_MONTHLY", "billingPeriodEnd": "2026-10-21T05:34:01Z", "cancelAtPeriodEnd": true}
        ]});
        let billing = grok_subscriptions(&cancelling).unwrap();
        assert_eq!(billing.auto_renew, Some(false));
        assert_eq!(billing.ends_at.as_deref(), Some("2026-10-21T05:34:01Z"));
    }

    #[test]
    fn an_x_premium_grok_account_reports_only_its_status() {
        let value = json!({"subscriptions": [
            {"tier": "SUBSCRIPTION_TIER_SUPER_GROK_PRO", "status": "SUBSCRIPTION_STATUS_INACTIVE",
             "billingPeriodEnd": "2026-08-21T05:34:01Z", "cancelAtPeriodEnd": false},
            {"tier": "SUBSCRIPTION_TIER_X_PREMIUM_PLUS", "status": "SUBSCRIPTION_STATUS_ACTIVE"}
        ]});
        let billing = grok_subscriptions(&value).unwrap();
        assert_eq!(billing.status.as_deref(), Some("active"));
        assert_eq!(billing.auto_renew, None);
        assert!(
            billing.renews_at.is_none()
                && billing.ends_at.is_none()
                && billing.paid_through.is_none()
        );
        assert!(grok_subscriptions(&json!({"subscriptions": []})).is_none());
    }

    #[test]
    fn claude_and_nous_report_what_they_have() {
        let profile = json!({"organization": {"subscription_status": "active", "billing_type": "stripe_subscription"}});
        assert_eq!(
            claude_profile(&profile).unwrap().status.as_deref(),
            Some("active")
        );
        let free = json!({"subscription": {"plan": "Free", "monthly_charge": 0, "current_period_end": "2026-10-11T00:24:25.000Z"}});
        assert!(nous_account(&free).is_none());
        let plus = json!({"subscription": {"plan": "Plus", "monthly_charge": 20, "current_period_end": "2026-10-11T00:24:25.000Z"}});
        let billing = nous_account(&plus).unwrap();
        assert_eq!(
            billing.paid_through.as_deref(),
            Some("2026-10-11T00:24:25.000Z")
        );
        assert_eq!(billing.auto_renew, None);
    }
}
