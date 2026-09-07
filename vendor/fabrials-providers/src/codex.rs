use fabrials_core::{ResetCredit, ResetInventory};
use serde_json::Value;

pub const RESET_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";

pub fn reset_count(usage: &Value) -> Option<u32> {
    usage
        .get("rate_limit_reset_credits")?
        .get("available_count")?
        .as_u64()?
        .try_into()
        .ok()
}

pub fn parse_resets(value: &Value, now_ms: i64) -> Result<ResetInventory, &'static str> {
    let rows = value
        .get("credits")
        .and_then(Value::as_array)
        .ok_or("invalid reset inventory")?;
    if rows.len() > 4096 {
        return Err("reset inventory too large");
    }
    let mut credits = Vec::new();
    for row in rows {
        let row = row.as_object().ok_or("invalid reset credit")?;
        let supported = match row.get("is_supported_by_plan") {
            None => true,
            Some(Value::Bool(value)) => *value,
            Some(_) => return Err("invalid reset plan support"),
        };
        if !supported {
            continue;
        }
        let status = row
            .get("status")
            .and_then(Value::as_str)
            .ok_or("invalid reset status")?;
        if status != "available" {
            continue;
        }
        let expires_at_ms = expiry_ms(row.get("expires_at"))?;
        if expires_at_ms.is_some_and(|expiry| expiry <= now_ms) {
            continue;
        }
        credits.push(ResetCredit {
            valid_from_ms: None,
            expires_at_ms,
        });
    }
    credits.sort_by_key(|credit| credit.expires_at_ms.unwrap_or(i64::MAX));
    let available = credits.len() as u32;
    let complete = credits.len() <= 64;
    credits.truncate(64);
    Ok(ResetInventory {
        available,
        credits,
        details_complete: complete,
    })
}

fn expiry_ms(value: Option<&Value>) -> Result<Option<i64>, &'static str> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            let date =
                time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
                    .map_err(|_| "invalid reset expiry")?;
            i64::try_from(date.unix_timestamp_nanos() / 1_000_000)
                .map(Some)
                .map_err(|_| "reset expiry overflow")
        }
        Some(value) => value
            .as_i64()
            .and_then(|seconds| seconds.checked_mul(1000))
            .map(Some)
            .ok_or("invalid reset expiry"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn string_expiries_and_plan_support_match_current_api_shape() {
        let value = serde_json::json!({"available_count":2,"credits":[
            {"id":"not-exported","status":"available","is_supported_by_plan":true,"expires_at":"2026-09-20T10:00:00.123Z"},
            {"status":"available","is_supported_by_plan":true,"expires_at":"2026-09-10T12:00:00+02:00"},
            {"status":"available","is_supported_by_plan":false,"expires_at":"2026-09-25T10:00:00Z"},
            {"status":"redeemed","is_supported_by_plan":true,"expires_at":"2026-09-25T10:00:00Z"},
            {"status":"available","is_supported_by_plan":true,"expires_at":"2020-01-01T00:00:00Z"}
        ]});
        let inventory = parse_resets(&value, 1_788_825_600_000).unwrap();
        assert_eq!(inventory.available, 2);
        assert!(inventory.details_complete);
        let dates: Vec<_> = inventory
            .credits
            .iter()
            .map(|credit| credit.expires_at_ms.unwrap())
            .collect();
        assert!(dates[0] < dates[1]);
        assert_eq!(dates[1] % 1000, 123);
        assert!(!serde_json::to_string(&inventory)
            .unwrap()
            .contains("not-exported"));
    }

    #[test]
    fn malformed_credits_do_not_become_available_resets() {
        for row in [
            serde_json::json!(null),
            serde_json::json!(17),
            serde_json::json!({}),
            serde_json::json!({"status":true}),
            serde_json::json!({"is_supported_by_plan":"yes"}),
            serde_json::json!({"status":"available","expires_at":"not-a-date"}),
            serde_json::json!({"status":"available","expires_at":i64::MAX}),
        ] {
            assert!(parse_resets(&serde_json::json!({"credits":[row]}), 0).is_err());
        }
    }
    #[test]
    fn filters_consumed_and_expired_without_exposing_ids() {
        let value = serde_json::json!({"credits":[
            {"id":"secret-handle", "status":"available", "expires_at":30},
            {"status":"consumed", "expires_at":40}, {"status":"available", "expires_at":1},
            {"status":"available"}]});
        let result = parse_resets(&value, 2000).unwrap();
        assert_eq!(result.available, 2);
        assert_eq!(result.credits[0].expires_at_ms, Some(30000));
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("secret-handle"));
        assert!(parse_resets(&serde_json::json!({}), 0).is_err());
    }
}
