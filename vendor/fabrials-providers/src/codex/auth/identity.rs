//! Signed identity matching only. This proof MUST NOT authenticate a Fabrials
//! session or import credentials. The host also requires an existing account
//! owned by the authenticated user with the same provider fingerprint.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

pub fn verify(token: &str, now_ms: i64) -> Result<String, String> {
    static CACHE: OnceLock<Mutex<Option<(std::time::Instant, Value)>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "Identity verification unavailable")?;
    if cache
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() > std::time::Duration::from_secs(3600))
    {
        let response = super::Client::new()?
            .http
            .get("https://auth.openai.com/.well-known/jwks.json")
            .send()
            .map_err(|_| "OpenAI signing keys unavailable")?;
        *cache = Some((std::time::Instant::now(), super::read(response)?));
    }
    verify_with_keys(
        token,
        &cache.as_ref().ok_or("OpenAI signing keys unavailable")?.1,
        now_ms,
    )
}
fn decode(value: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| "Invalid OpenAI identity proof".into())
}
fn verify_with_keys(token: &str, jwks: &Value, now_ms: i64) -> Result<String, String> {
    if token.len() > 32_768 {
        return Err("Identity proof too large".into());
    }
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err("Invalid OpenAI identity proof".into());
    }
    let header: Value =
        serde_json::from_slice(&decode(parts[0])?).map_err(|_| "Invalid identity header")?;
    if header["alg"] != "RS256" {
        return Err("Unsupported identity signature".into());
    }
    let kid = header["kid"]
        .as_str()
        .filter(|kid| !kid.is_empty())
        .ok_or("Identity signing key missing")?;
    let keys = jwks["keys"]
        .as_array()
        .filter(|keys| keys.len() <= 64)
        .ok_or("Invalid OpenAI signing keys")?;
    let key = keys
        .iter()
        .find(|key| {
            key["kid"] == kid && key["kty"] == "RSA" && key["use"] == "sig" && key["alg"] == "RS256"
        })
        .ok_or("OpenAI identity signing key not found")?;
    let n = decode(key["n"].as_str().ok_or("Invalid signing key")?)?;
    let e = decode(key["e"].as_str().ok_or("Invalid signing key")?)?;
    ring::signature::RsaPublicKeyComponents { n: &n, e: &e }
        .verify(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &decode(parts[2])?,
        )
        .map_err(|_| "Invalid OpenAI identity signature")?;
    let claims: Value =
        serde_json::from_slice(&decode(parts[1])?).map_err(|_| "Invalid identity claims")?;
    let audience = &claims["aud"];
    let intended = audience == super::CLIENT_ID
        || audience
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value == super::CLIENT_ID));
    let issued = claims["iat"]
        .as_i64()
        .filter(|issued| *issued >= 0 && *issued <= now_ms / 1000 + 300)
        .ok_or("Invalid identity issue date")?;
    if claims["iss"] != super::ISSUER
        || !intended
        || claims["exp"]
            .as_i64()
            .is_none_or(|expires| expires <= issued)
    {
        return Err("Invalid OpenAI identity claims".into());
    }
    // Expired ID tokens still attest an identity, but grant no authority here.
    // Requiring a current owner-scoped hosted account is the authorization boundary.
    let id = claims["https://api.openai.com/auth"]["chatgpt_account_id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 256 && id.bytes().all(|b| b.is_ascii_graphic()))
        .ok_or("OpenAI account identity missing")?;
    Ok(id.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_attestation_requires_provider_signature_issuer_audience_and_issue_date() {
        let fixture: Value = serde_json::from_str(include_str!("identity-fixture.json")).unwrap();
        assert_eq!(
            verify_with_keys(fixture["valid"].as_str().unwrap(), &fixture["jwks"], 10_000).unwrap(),
            "fixture-account"
        );
        for name in ["wrong_audience", "wrong_issuer", "future"] {
            assert!(
                verify_with_keys(fixture[name].as_str().unwrap(), &fixture["jwks"], 10_000)
                    .is_err()
            );
        }
        let mut forged = fixture["valid"]
            .as_str()
            .unwrap()
            .split('.')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut claims: Value = serde_json::from_slice(&decode(&forged[1]).unwrap()).unwrap();
        claims["https://api.openai.com/auth"]["chatgpt_account_id"] =
            serde_json::json!("someone-else");
        forged[1] = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        assert!(verify_with_keys(&forged.join("."), &fixture["jwks"], 10_000).is_err());
        forged[0] = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","kid":"fixture"}"#);
        assert!(verify_with_keys(&forged.join("."), &fixture["jwks"], 10_000).is_err());
    }
}
