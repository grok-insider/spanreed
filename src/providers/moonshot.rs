//! Moonshot / Kimi Open Platform balance.
//!
//! `GET https://api.moonshot.ai/v1/users/me/balance` (or the `.cn` host).

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;

const ID: &str = "moonshot";
const NAME: &str = "Moonshot";

pub struct Moonshot;

fn origin() -> Result<String, String> {
    let raw = env_any(&["MOONSHOT_BASE_URL", "MOONSHOT_API_BASE"])
        .unwrap_or_else(|| "https://api.moonshot.ai".into());
    let base = json_api::allowed_base(&raw, false)?;
    if base != "https://api.moonshot.ai" && base != "https://api.moonshot.cn" {
        return Err("Moonshot base URL must be api.moonshot.ai or api.moonshot.cn.".into());
    }
    Ok(base)
}

pub(super) fn parse_balance(
    data: &serde_json::Value,
    cny: bool,
) -> Result<Vec<MetricLine>, String> {
    let code = field(data, "code").ok_or("Moonshot response missing code")?;
    if code != 0.0 || data.get("status").and_then(|v| v.as_bool()) != Some(true) {
        return Err("Moonshot balance request was rejected.".into());
    }
    let payload = data.get("data").ok_or("Moonshot response missing data")?;
    let available =
        field(payload, "available_balance").ok_or("Moonshot response missing balance")?;
    let currency = if cny { "CNY" } else { "USD" };
    Ok(vec![json_api::text_line(
        "Balance",
        format!("{available:.4} {currency}"),
    )])
}

impl Provider for Moonshot {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["MOONSHOT_API_KEY", "MOONSHOT_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["MOONSHOT_API_KEY", "MOONSHOT_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No Moonshot API key found. Set MOONSHOT_API_KEY.",
            );
        };
        let base = match origin() {
            Ok(b) => b,
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        let url = json_api::join_url(&base, "v1/users/me/balance");
        match json_api::get_bearer(&url, &key) {
            Ok(data) => match parse_balance(&data, base.ends_with(".cn")) {
                Ok(lines) => ProviderOutput::new(ID, NAME, lines),
                Err(err) => ProviderOutput::error(ID, NAME, err),
            },
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usd_balance() {
        let lines = parse_balance(
            &serde_json::json!({
                "code": 0,
                "scode": "0",
                "status": true,
                "data": {"available_balance": 12.5, "cash_balance": 10.0, "voucher_balance": 2.5}
            }),
            false,
        )
        .unwrap();
        assert!(
            matches!(&lines[0], MetricLine::Text { value, .. } if value.contains("12.5000 USD"))
        );
    }
}
