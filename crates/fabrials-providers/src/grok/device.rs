//! Grok device authorization transport. The host owns polling and persistence.
use crate::device_flow::{DeviceAuthorization, DeviceView, PollResult};
use serde_json::Value;

pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const ISSUER: &str = "https://auth.x.ai";
const SCOPES: &str = "openid profile email offline_access grok-cli:access api:access conversations:read conversations:write workspaces:read workspaces:write";

pub struct Client(reqwest::blocking::Client);
impl Client {
    pub fn new() -> Result<Self, String> {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map(Self)
            .map_err(|_| "Could not initialize Grok authorization".into())
    }
    pub fn begin(&self) -> Result<DeviceAuthorization, String> {
        let response = self
            .0
            .post(format!("{ISSUER}/oauth2/device/code"))
            .header("x-grok-client-surface", "cli")
            .form(&[
                ("client_id", CLIENT_ID),
                ("scope", SCOPES),
                ("referrer", "grok-build"),
            ])
            .send()
            .map_err(|_| "Grok authorization request failed")?;
        if !response.status().is_success() {
            return Err(format!(
                "Grok authorization HTTP {}",
                response.status().as_u16()
            ));
        }
        parse_authorization(&read_json(response)?)
    }
    pub fn poll(&self, device_code: &str) -> Result<PollResult, String> {
        let response = self
            .0
            .post(format!("{ISSUER}/oauth2/token"))
            .header("x-grok-client-surface", "cli")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code),
                ("client_id", CLIENT_ID),
            ])
            .send()
            .map_err(|_| "Grok authorization check failed")?;
        let status = response.status().as_u16();
        parse_poll(status, read_json(response)?)
    }
}
fn read_json(response: reqwest::blocking::Response) -> Result<Value, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    response
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| "Grok authorization response unavailable")?;
    if bytes.len() > 1_048_576 {
        return Err("Grok authorization response too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid Grok authorization response".into())
}
fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value[field]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 8192 && !s.chars().any(char::is_control))
        .ok_or_else(|| format!("Invalid Grok authorization {field}"))
}
fn parse_authorization(value: &Value) -> Result<DeviceAuthorization, String> {
    let uri = value
        .get("verification_uri_complete")
        .or_else(|| value.get("verification_uri"))
        .and_then(Value::as_str)
        .ok_or("Grok verification URL missing")?;
    let url = reqwest::Url::parse(uri).map_err(|_| "Invalid Grok verification URL")?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("auth.x.ai" | "accounts.x.ai"))
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Unexpected Grok verification URL".into());
    }
    let expires_in = value["expires_in"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= 3600)
        .ok_or("Invalid Grok authorization expiry")?;
    let interval = value
        .get("interval")
        .map(|v| {
            v.as_u64()
                .filter(|n| *n > 0 && *n <= 60)
                .ok_or("Invalid Grok polling interval")
        })
        .transpose()?
        .unwrap_or(5);
    Ok(DeviceAuthorization {
        device_code: text(value, "device_code")?.into(),
        view: DeviceView {
            verification_uri: url.into(),
            user_code: text(value, "user_code")?.into(),
            expires_in,
            interval,
        },
    })
}
fn parse_poll(status: u16, value: Value) -> Result<PollResult, String> {
    if (200..300).contains(&status) {
        text(&value, "access_token")?;
        if value.get("refresh_token").is_some() {
            text(&value, "refresh_token")?;
        }
        return Ok(PollResult::Authorized(value));
    }
    match value["error"].as_str() {
        Some("authorization_pending") => Ok(PollResult::Pending),
        Some("slow_down") => Ok(PollResult::SlowDown),
        Some("access_denied") => Err("Grok authorization denied".into()),
        Some("expired_token") => Err("Grok authorization expired; start again".into()),
        _ => Err(format!("Grok authorization HTTP {status}")),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_browser_destination_and_keeps_device_grant_out_of_view() {
        let mut value = serde_json::json!({"device_code":"private-fixture", "user_code":"PUBLIC", "verification_uri":"https://auth.x.ai/activate", "expires_in":600});
        let authorization = parse_authorization(&value).unwrap();
        assert!(!serde_json::to_string(&authorization.view)
            .unwrap()
            .contains("private-fixture"));
        value["verification_uri_complete"] =
            "https://accounts.x.ai/oauth2/device?user_code=PUBLIC".into();
        let current = parse_authorization(&value).unwrap();
        assert_eq!(
            current.view.verification_uri,
            "https://accounts.x.ai/oauth2/device?user_code=PUBLIC"
        );
        value
            .as_object_mut()
            .unwrap()
            .remove("verification_uri_complete");
        for uri in [
            "https://accounts.x.ai.evil.example/oauth2/device",
            "http://accounts.x.ai/oauth2/device",
            "https://accounts.x.ai:444/oauth2/device",
            "https://user@accounts.x.ai/oauth2/device",
            "http://auth.x.ai/",
            "https://other.example/",
            "https://auth.x.ai:444/",
            "https://user@auth.x.ai/",
        ] {
            value["verification_uri"] = uri.into();
            assert!(parse_authorization(&value).is_err());
        }
    }
    #[test]
    fn rejects_empty_grants_and_does_not_echo_provider_errors() {
        assert!(parse_poll(200, serde_json::json!({"access_token":""})).is_err());
        assert!(matches!(
            parse_poll(400, serde_json::json!({"error":"slow_down"})).unwrap(),
            PollResult::SlowDown
        ));
        let error = parse_poll(400, serde_json::json!({"error":"private-fixture"}))
            .err()
            .unwrap();
        assert!(!error.contains("private-fixture"));
    }
}
