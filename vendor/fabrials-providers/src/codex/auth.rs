//! ChatGPT device authorization. Wire protocol follows openai/codex login.
pub mod identity;
use crate::device_flow::{DeviceAuthorization, DeviceView, PollResult};
pub use crate::oauth::{ensure_access, valid_access};
use base64::Engine;
use serde_json::{json, Value};

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const ISSUER: &str = "https://auth.openai.com";

pub fn claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}
pub fn account_id(document: &Value) -> Option<&str> {
    document
        .get("account_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| b.is_ascii_graphic()))
}
pub fn token_document(value: Value, now: i64, previous: Option<&Value>) -> Result<Value, String> {
    let access = value["access_token"]
        .as_str()
        .ok_or("Codex access token missing")?;
    let access_claims = claims(access).ok_or("Invalid Codex access token")?;
    let id_claims = value["id_token"].as_str().and_then(claims);
    let identity = access_claims
        .get("https://api.openai.com/auth")
        .or_else(|| id_claims.as_ref()?.get("https://api.openai.com/auth"));
    let id = identity
        .and_then(|v| v["chatgpt_account_id"].as_str())
        .or_else(|| previous.and_then(account_id))
        .ok_or("Codex account identity missing")?;
    if previous.and_then(account_id).is_some_and(|old| old != id) {
        return Err("Codex authorization changed account".into());
    }
    let expires = access_claims["exp"]
        .as_i64()
        .and_then(|n| n.checked_mul(1000))
        .ok_or("Codex token expiry missing")?;
    let refresh = value["refresh_token"]
        .as_str()
        .or_else(|| previous?.get("refresh_token")?.as_str())
        .ok_or("Codex refresh token missing")?;
    let document = json!({"type":"oauth","access_token":access,"refresh_token":refresh,
        "account_id":id,"expires_at_ms":expires,"id_token":value.get("id_token").or_else(|| previous?.get("id_token")),
        "plan":identity.and_then(|v| v.get("chatgpt_plan_type"))});
    validate(&document, now)?;
    Ok(document)
}
pub fn validate(document: &Value, now: i64) -> Result<(), String> {
    if valid_access(document, now).is_none()
        || account_id(document).is_none()
        || document["refresh_token"].as_str().is_none_or(|s| {
            s.is_empty() || s.len() > 16384 || !s.bytes().all(|b| b.is_ascii_graphic())
        })
    {
        return Err("Invalid or expired Codex authorization".into());
    }
    Ok(())
}

pub struct Client {
    http: reqwest::blocking::Client,
}
impl Client {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            http: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("codex_cli_rs")
                .build()
                .map_err(|_| "Could not initialize Codex authorization")?,
        })
    }
    pub fn begin(&self) -> Result<DeviceAuthorization, String> {
        let value = read(
            self.http
                .post(format!("{ISSUER}/api/accounts/deviceauth/usercode"))
                .json(&json!({"client_id":CLIENT_ID}))
                .send()
                .map_err(|_| "Codex authorization unavailable")?,
        )?;
        let code = required(&value, "user_code").or_else(|_| required(&value, "usercode"))?;
        let id = required(&value, "device_auth_id")?;
        let interval = value["interval"]
            .as_u64()
            .or_else(|| value["interval"].as_str()?.parse().ok())
            .unwrap_or(5)
            .clamp(1, 60);
        Ok(DeviceAuthorization {
            device_code: json!({"device_auth_id":id,"user_code":code}).to_string(),
            view: DeviceView {
                verification_uri: format!("{ISSUER}/codex/device"),
                user_code: code.into(),
                expires_in: 900,
                interval,
            },
        })
    }
    pub fn poll(&self, code: &str, now: i64) -> Result<PollResult, String> {
        let proof: Value =
            serde_json::from_str(code).map_err(|_| "Invalid saved Codex authorization")?;
        let response = self
            .http
            .post(format!("{ISSUER}/api/accounts/deviceauth/token"))
            .json(&proof)
            .send()
            .map_err(|_| "Codex authorization check unavailable")?;
        match response.status().as_u16() {
            403 | 404 => return Ok(PollResult::Pending),
            429 => return Ok(PollResult::SlowDown),
            _ => {}
        }
        let result = read(response)?;
        let value = read(
            self.http
                .post(format!("{ISSUER}/oauth/token"))
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("client_id", CLIENT_ID),
                    (
                        "redirect_uri",
                        "https://auth.openai.com/deviceauth/callback",
                    ),
                    ("code", required(&result, "authorization_code")?),
                    ("code_verifier", required(&result, "code_verifier")?),
                ])
                .send()
                .map_err(|_| "Codex authorization exchange unavailable")?,
        )?;
        Ok(PollResult::Authorized(token_document(value, now, None)?))
    }
    pub fn refresh(&self, document: &Value, now: i64) -> Result<Value, String> {
        let value = read(
            self.http
                .post(format!("{ISSUER}/oauth/token"))
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", CLIENT_ID),
                    ("refresh_token", required(document, "refresh_token")?),
                ])
                .send()
                .map_err(|_| "Codex token refresh unavailable")?,
        )?;
        token_document(value, now, Some(document))
    }
    pub fn get(&self, path: &str, document: &Value) -> Result<Value, String> {
        if !matches!(
            path,
            "/backend-api/codex/models?client_version=0.153.4"
                | "/backend-api/wham/usage"
                | "/backend-api/wham/rate-limit-reset-credits"
        ) {
            return Err("Unsupported Codex metadata operation".into());
        }
        let access = required(document, "access_token")?;
        read(
            self.http
                .get(format!("https://chatgpt.com{path}"))
                .bearer_auth(access)
                .header(
                    "ChatGPT-Account-Id",
                    account_id(document).ok_or("Codex account missing")?,
                )
                .header("originator", "codex_cli_rs")
                .send()
                .map_err(|_| "Codex metadata unavailable")?,
        )
    }
}
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 16384)
        .ok_or_else(|| "Incomplete Codex authorization".into())
}
fn read(response: reqwest::blocking::Response) -> Result<Value, String> {
    use std::io::Read;
    if !response.status().is_success() {
        return Err(format!("Codex authorization failed (HTTP {}). Check device login availability or authorize again.",response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    response
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read Codex authorization")?;
    if bytes.len() > 1_048_576 {
        return Err("Codex authorization response too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid Codex authorization response".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn jwt(account: &str, expiry: i64) -> String {
        let body = json!({"exp":expiry,"https://api.openai.com/auth":{"chatgpt_account_id":account,"chatgpt_plan_type":"pro"}});
        format!(
            "header.{}.signature",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(body.to_string())
        )
    }
    #[test]
    fn rotation_retains_identity_and_missing_refresh_without_accepting_account_changes() {
        let first = token_document(
            json!({"access_token":jwt("account-a",2000),"refresh_token":"private-fixture"}),
            1000,
            None,
        )
        .unwrap();
        let next = token_document(
            json!({"access_token":jwt("account-a",3000)}),
            2000,
            Some(&first),
        )
        .unwrap();
        assert_eq!(next["refresh_token"], "private-fixture");
        assert_eq!(next["expires_at_ms"], 3_000_000);
        assert!(token_document(
            json!({"access_token":jwt("account-b",3000)}),
            2000,
            Some(&first)
        )
        .is_err());
        assert!(validate(&first, 2_000_001).is_err());
    }
}
