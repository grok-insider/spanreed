use fabrials_core::CreditBalance;
use serde_json::Value;
pub mod device_flow;

pub const DEVICE_URL: &str = "https://portal.nousresearch.com/api/oauth/device/code";
pub const TOKEN_URL: &str = "https://portal.nousresearch.com/api/oauth/token";
pub const ACCOUNT_URL: &str = "https://portal.nousresearch.com/api/oauth/account";
pub const CLIENT_ID: &str = "hermes-cli";
pub const SCOPE: &str = "inference:invoke";

pub use crate::oauth::{ensure_access, valid_access};

pub fn parse_balance(value: &Value) -> Option<CreditBalance> {
    let access = value
        .get("paid_service_access")
        .filter(|value| value.is_object())
        .unwrap_or(value);
    let number = |key: &str| {
        access
            .get(key)
            .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
            .filter(|n| n.is_finite() && *n >= 0.0)
    };
    let balance = CreditBalance {
        remaining_usd: number("total_usable_credits"),
        subscription_remaining_usd: number("subscription_credits_remaining"),
        purchased_remaining_usd: number("purchased_credits_remaining"),
        subscription_limit_usd: value
            .get("subscription")
            .and_then(|s| s.get("monthly_credits"))
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .or_else(|| number("monthly_credits")),
        paid_access: access
            .get("allowed")
            .or_else(|| access.get("paid_access"))
            .and_then(Value::as_bool),
    };
    (balance != CreditBalance::default()).then_some(balance)
}

pub use crate::device_flow::{DeviceAuthorization, DeviceView, PollResult};

pub struct Client {
    http: reqwest::blocking::Client,
    client_id: String,
}
impl Client {
    pub fn new(client_id: Option<&str>) -> Result<Self, String> {
        Ok(Self {
            http: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "Could not initialize Nous client")?,
            client_id: client_id.unwrap_or(CLIENT_ID).to_string(),
        })
    }
    pub fn begin(&self) -> Result<DeviceAuthorization, String> {
        let response = self
            .http
            .post(DEVICE_URL)
            .form(&[("client_id", self.client_id.as_str()), ("scope", SCOPE)])
            .send()
            .map_err(|_| "Nous authorization unavailable")?;
        let value = read_response(response)?;
        let uri = value
            .get("verification_uri_complete")
            .or_else(|| value.get("verification_uri"))
            .and_then(Value::as_str)
            .ok_or("Nous verification URL missing")?;
        let url = reqwest::Url::parse(uri).map_err(|_| "Invalid Nous verification URL")?;
        if url.scheme() != "https"
            || url.host_str() != Some("portal.nousresearch.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some_and(|port| port != 443)
        {
            return Err("Invalid Nous verification origin".into());
        }
        let required = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty() && s.len() <= 4096)
                .map(str::to_string)
                .ok_or_else(|| "Incomplete Nous authorization".to_string())
        };
        Ok(DeviceAuthorization {
            device_code: required("device_code")?,
            view: DeviceView {
                verification_uri: uri.to_string(),
                user_code: required("user_code")?,
                expires_in: value
                    .get("expires_in")
                    .and_then(Value::as_u64)
                    .filter(|s| *s > 0 && *s <= 3600)
                    .ok_or("Invalid Nous authorization expiry")?,
                interval: value
                    .get("interval")
                    .and_then(Value::as_u64)
                    .unwrap_or(5)
                    .clamp(1, 60),
            },
        })
    }
    pub fn poll(&self, code: &str, now_ms: i64) -> Result<PollResult, String> {
        let response = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", code),
            ])
            .send()
            .map_err(|_| "Nous authorization check unavailable")?;
        let status = response.status();
        let value = read_json(response)?;
        match value.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => return Ok(PollResult::Pending),
            Some("slow_down") => return Ok(PollResult::SlowDown),
            Some("access_denied") => return Err("Nous authorization denied".into()),
            Some("expired_token") => return Err("Nous authorization expired".into()),
            Some(_) => return Err("Nous authorization rejected".into()),
            _ => {}
        }
        if !status.is_success() {
            return Err(format!("Nous authorization HTTP {}", status.as_u16()));
        }
        Ok(PollResult::Authorized(self.token_document(value, now_ms)?))
    }
    pub fn refresh(&self, document: &Value, now_ms: i64) -> Result<Value, String> {
        let refresh = document
            .get("refresh_token")
            .and_then(Value::as_str)
            .ok_or("Nous login required")?;
        let client = document
            .get("client_id")
            .and_then(Value::as_str)
            .unwrap_or(&self.client_id);
        let response = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", client),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
            ])
            .send()
            .map_err(|_| "Nous token refresh unavailable")?;
        let value = read_response(response)?;
        let mut replacement = self.token_document(value, now_ms)?;
        if replacement
            .get("refresh_token")
            .and_then(Value::as_str)
            .is_none()
        {
            replacement["refresh_token"] = Value::String(refresh.into());
        }
        replacement["client_id"] = Value::String(client.into());
        Ok(replacement)
    }
    pub fn account(&self, token: &str) -> Result<Value, String> {
        read_response(
            self.http
                .get(ACCOUNT_URL)
                .bearer_auth(token)
                .header("Accept", "application/json")
                .send()
                .map_err(|_| "Nous account unavailable")?,
        )
    }
    fn token_document(&self, value: Value, now_ms: i64) -> Result<Value, String> {
        if value
            .get("scope")
            .and_then(Value::as_str)
            .is_some_and(|scope| !scope.split_whitespace().any(|item| item == SCOPE))
        {
            return Err("Nous authorization is missing inference access".into());
        }
        let token = value
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("Nous access token missing")?;
        let seconds = value
            .get("expires_in")
            .and_then(Value::as_i64)
            .filter(|s| *s > 0)
            .ok_or("Nous token expiry missing")?;
        let expires = seconds
            .checked_mul(1000)
            .and_then(|s| now_ms.checked_add(s))
            .ok_or("Invalid Nous token expiry")?;
        Ok(
            serde_json::json!({"auth_type":"oauth", "access_token":token,
            "refresh_token":value.get("refresh_token").and_then(Value::as_str),
            "expires_at_ms":expires,"client_id":self.client_id,"scope":value.get("scope").and_then(Value::as_str).unwrap_or(SCOPE)}),
        )
    }
}

fn read_response(response: reqwest::blocking::Response) -> Result<Value, String> {
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            401 => "Nous rejected this credential (HTTP 401). Reconnect with a new API key or authorize the account again.".into(),
            403 => "Nous denied access (HTTP 403). Check this account's permissions and authorization.".into(),
            status => format!("Nous HTTP {status}"),
        });
    }
    read_json(response)
}
fn read_json(response: reqwest::blocking::Response) -> Result<Value, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    response
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read Nous response")?;
    if bytes.len() > 1_048_576 {
        return Err("Nous response too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid Nous response".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn access_validation_and_refresh_failure_never_serve_expired_tokens() {
        let mut document = json!({"access_token":"valid", "expires_at_ms":1000});
        assert_eq!(
            ensure_access(&mut document, 999, |_, _| Err("offline".into())).as_deref(),
            Some("valid")
        );
        assert!(ensure_access(&mut document, 1000, |_, _| Err("offline".into())).is_none());
        assert!(valid_access(
            &json!({"access_token":"bad\r\nheader", "expires_at_ms":2000}),
            1000
        )
        .is_none());
        let mut document = json!({"access_token":"valid", "expires_at_ms":200_000});
        assert_eq!(
            ensure_access(&mut document, 0, |_, _| panic!(
                "fresh token must not refresh"
            ))
            .as_deref(),
            Some("valid")
        );
    }

    #[test]
    fn account_balance_keeps_purchased_credits_out_of_subscription_usage() {
        let balance = parse_balance(&json!({"paid_service_access": {
            "total_usable_credits": "104.5", "subscription_credits_remaining": 4.5,
            "purchased_credits_remaining": 100, "allowed": false
        }, "subscription": {"monthly_credits": 9}}))
        .unwrap();
        assert_eq!(balance.remaining_usd, Some(104.5));
        assert_eq!(balance.paid_access, Some(false));
        assert_eq!(balance.subscription_used_percent(), Some(50.0));
        assert!(parse_balance(&json!({"total_usable_credits": "NaN"})).is_none());
        assert!(parse_balance(&json!({"total_usable_credits": -1})).is_none());
    }

    #[test]
    fn grant_rejects_missing_expiry_and_wrong_scope() {
        let client = Client::new(None).unwrap();
        assert!(client
            .token_document(json!({"access_token":"fixture"}), 1000)
            .is_err());
        assert!(client
            .token_document(
                json!({"access_token":"fixture","expires_in":30,"scope":"profile"}),
                1000
            )
            .is_err());
        let grant = client.token_document(json!({"access_token":"fixture","refresh_token":"rotated-fixture","expires_in":30,"scope":"inference:invoke"}),1000).unwrap();
        assert_eq!(grant["expires_at_ms"], 31_000);
        assert_eq!(grant["refresh_token"], "rotated-fixture");
    }
}
