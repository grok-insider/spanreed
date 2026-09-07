use super::Provider;
use crate::model::{MetricKind, MetricLine, ProviderOutput};
pub struct Nous;
impl Provider for Nous {
    fn id(&self) -> &'static str {
        "nous"
    }
    fn name(&self) -> &'static str {
        "Nous Research"
    }
    fn detect(&self) -> bool {
        !crate::accounts::list_provider("nous").is_empty()
    }
    fn probe(&self) -> ProviderOutput {
        let Some(account) = crate::accounts::active("nous") else {
            return ProviderOutput::error(
                "nous",
                "Nous Research",
                "Connect an account: spanreed account add nous",
            );
        };
        let result = crate::drivers::nous::token(&account.alias)
            .and_then(|token| fabrials_providers::nous::Client::new(None)?.account(&token));
        match result {
            Ok(value) => {
                let Some(balance) = fabrials_providers::nous::parse_balance(&value) else {
                    return ProviderOutput::error(
                        "nous",
                        "Nous Research",
                        "Account did not report credits",
                    );
                };
                let mut lines = Vec::new();
                if let Some(value) = balance.remaining_usd {
                    lines.push(MetricLine::text(
                        MetricKind::Plan,
                        "Available credits",
                        format!("${value:.2}"),
                    ));
                }
                if let Some(percent) = balance.subscription_used_percent() {
                    lines.push(MetricLine::percent("Subscription", percent, None));
                }
                if let Some(value) = balance.purchased_remaining_usd {
                    lines.push(MetricLine::text(
                        MetricKind::Plan,
                        "Purchased credits",
                        format!("${value:.2}"),
                    ));
                }
                ProviderOutput::new("nous", "Nous Research", lines)
            }
            Err(error) => ProviderOutput::error("nous", "Nous Research", error),
        }
    }
}
