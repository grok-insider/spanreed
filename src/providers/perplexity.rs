//! Perplexity credit pools.
//!
//! Uses `PERPLEXITY_COOKIE` or `PERPLEXITY_SESSION_TOKEN`. Browser cookie
//! import is not used. Amounts stay in the API's cent counts.

use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;
use crate::util;

const ID: &str = "perplexity";
const NAME: &str = "Perplexity";

pub struct Perplexity;

fn cookie_header() -> Option<String> {
    if let Some(cookie) = env_any(&["PERPLEXITY_COOKIE"]) {
        if cookie.contains('=') {
            return Some(cookie);
        }
        return Some(format!("__Secure-next-auth.session-token={cookie}"));
    }
    env_any(&["PERPLEXITY_SESSION_TOKEN"])
        .map(|token| format!("__Secure-next-auth.session-token={token}"))
}

fn grant_cents(grants: &[serde_json::Value], kind: &str) -> f64 {
    grants
        .iter()
        .filter(|grant| grant.get("type").and_then(|v| v.as_str()) == Some(kind))
        .filter_map(|grant| field(grant, "amount_cents").or_else(|| field(grant, "amountCents")))
        .sum()
}

pub(super) fn parse_credits(body: &serde_json::Value) -> (Vec<MetricLine>, Option<String>) {
    let grants = body
        .get("credit_grants")
        .or_else(|| body.get("creditGrants"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let recurring = grant_cents(&grants, "recurring");
    let promo = grant_cents(&grants, "promotional") + grant_cents(&grants, "bonus");
    let purchased = grant_cents(&grants, "purchased");
    let mut lines = Vec::new();
    if !grants.is_empty() {
        let resets = body
            .get("renewal_date_ts")
            .or_else(|| body.get("renewalDateTs"))
            .and_then(util::to_iso);
        if recurring > 0.0 {
            lines.push(json_api::text_line("Credits", format!("{recurring:.0}")));
            if let Some(resets) = resets {
                lines.push(MetricLine::text(MetricKind::Quota, "Renewal", resets));
            }
        }
        if promo > 0.0 {
            lines.push(json_api::text_line("Bonus credits", format!("{promo:.0}")));
        }
        if purchased > 0.0 {
            lines.push(json_api::text_line("Purchased", format!("{purchased:.0}")));
        }
    }
    if let Some(total) = field(body, "total_usage_cents").or_else(|| field(body, "totalUsageCents"))
    {
        lines.push(json_api::text_line("Used", format!("{total:.0}")));
    }
    let plan = if recurring > 0.0 {
        Some(if recurring < 5000.0 { "Pro" } else { "Max" }.to_string())
    } else {
        None
    };
    (lines, plan)
}

impl Provider for Perplexity {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        cookie_header().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(cookie) = cookie_header() else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Perplexity session found. Set PERPLEXITY_SESSION_TOKEN or PERPLEXITY_COOKIE.",
            );
        };
        match json_api::get_headers(
            "https://www.perplexity.ai/rest/billing/credits?version=2.18&source=default",
            &[
                ("Cookie", &cookie),
                ("Origin", "https://www.perplexity.ai"),
                ("Referer", "https://www.perplexity.ai/account/usage"),
            ],
        ) {
            Ok(data) => {
                let (lines, plan) = parse_credits(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "Perplexity credit response was empty.")
                } else {
                    ProviderOutput::new(ID, NAME, lines).with_plan(plan)
                }
            }
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_cent_counts() {
        let (lines, plan) = parse_credits(&serde_json::json!({
            "total_usage_cents": 120,
            "renewal_date_ts": 1700000000,
            "credit_grants": [
                {"type": "recurring", "amount_cents": 2000},
                {"type": "promotional", "amount_cents": 300},
                {"type": "purchased", "amount_cents": 50}
            ]
        }));
        assert_eq!(plan.as_deref(), Some("Pro"));
        assert!(lines.len() >= 3);
    }
}
