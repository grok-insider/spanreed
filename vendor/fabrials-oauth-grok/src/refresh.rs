//! Access-token refresh against auth.x.ai. HTTP is injected (no reqwest here).

use serde_json::Value;

use crate::{CLIENT_ID, REFRESH_BUFFER_MS, REFRESH_URL};

pub trait TokenHttp {
    fn post_form(&self, url: &str, body: &str) -> Result<(u16, String), String>;
}

/// Return a usable access token, refreshing the blob in place when expired.
pub fn ensure_access_token(doc: &mut Value, now_ms: i64, http: &impl TokenHttp) -> Option<String> {
    if !doc.is_object() {
        return None;
    }
    let keys: Vec<String> = doc.as_object()?.keys().cloned().collect();
    for entry_key in keys {
        let entry = doc.get(&entry_key)?.clone();
        if !entry.is_object() {
            continue;
        }
        let token = entry
            .get("key")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if token.is_empty() {
            continue;
        }
        if needs_refresh(&entry, &token, now_ms) {
            if let Some(new_tok) = refresh(doc, &entry_key, now_ms, http) {
                return Some(new_tok);
            }
        }
        let expiry = entry
            .get("expires_at")
            .or_else(|| entry.get("expires"))
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_ms);
        if expiry
            .into_iter()
            .chain(jwt_exp_ms(&token))
            .any(|expiry| expiry <= now_ms)
        {
            return None;
        }
        return Some(token);
    }
    None
}

/// Access that is outside the renewal window; this never performs HTTP.
pub fn fresh_access_token(doc: &Value, now_ms: i64) -> Option<String> {
    for entry in doc.as_object()?.values() {
        let Some(token) = entry
            .get("key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let known_expiry = entry
            .get("expires_at")
            .or_else(|| entry.get("expires"))
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_ms)
            .or_else(|| jwt_exp_ms(token));
        return (known_expiry.is_some() && !needs_refresh(entry, token, now_ms))
            .then(|| token.to_string());
    }
    None
}

fn needs_refresh(entry: &Value, token: &str, now_ms: i64) -> bool {
    let entry_ms = entry
        .get("expires_at")
        .or_else(|| entry.get("expires"))
        .and_then(|v| v.as_str())
        .and_then(parse_rfc3339_ms);
    let token_ms = jwt_exp_ms(token);
    entry_ms
        .map(|ms| now_ms + REFRESH_BUFFER_MS >= ms)
        .unwrap_or(false)
        || token_ms
            .map(|ms| now_ms + REFRESH_BUFFER_MS >= ms)
            .unwrap_or(false)
}

fn refresh(doc: &mut Value, entry_key: &str, now_ms: i64, http: &impl TokenHttp) -> Option<String> {
    let entry = doc.get(entry_key)?.clone();
    let refresh_token = ["refresh_token", "refresh"]
        .iter()
        .find_map(|k| entry.get(*k).and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let client_id = entry
        .get("oidc_client_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(CLIENT_ID);
    let body = format!(
        "grant_type=refresh_token&client_id={}&refresh_token={}",
        urlenc(client_id),
        urlenc(refresh_token)
    );
    let (status, resp_body) = http.post_form(REFRESH_URL, &body).ok()?;
    if !(200..300).contains(&status) {
        return None;
    }
    let json: Value = serde_json::from_str(&resp_body).ok()?;
    let access = json.get("access_token")?.as_str()?.trim().to_string();
    if access.is_empty() {
        return None;
    }
    if let Some(obj) = doc.get_mut(entry_key).and_then(|v| v.as_object_mut()) {
        obj.insert("key".into(), serde_json::json!(access));
        if let Some(rt) = json.get("refresh_token").and_then(|v| v.as_str()) {
            if !rt.trim().is_empty() {
                obj.insert("refresh_token".into(), serde_json::json!(rt.trim()));
            }
        }
        let expires_at = json
            .get("expires_in")
            .and_then(|v| v.as_f64())
            .filter(|n| *n > 0.0)
            .map(|n| now_ms + (n as i64) * 1000)
            .or_else(|| jwt_exp_ms(&access))
            .unwrap_or(now_ms + 3600 * 1000);
        obj.insert(
            "expires_at".into(),
            serde_json::json!(ms_to_rfc3339(expires_at)),
        );
    }
    Some(access)
}

fn urlenc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn jwt_exp_ms(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let json = decode_b64url(payload)?;
    let v: Value = serde_json::from_slice(&json).ok()?;
    let exp = v.get("exp")?.as_i64()?;
    Some(exp.saturating_mul(1000))
}

fn decode_b64url(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let mut t = s.replace('-', "+").replace('_', "/");
    while t.len() % 4 != 0 {
        t.push('=');
    }
    base64::engine::general_purpose::STANDARD.decode(t).ok()
}

fn parse_rfc3339_ms(iso: &str) -> Option<i64> {
    // 2026-01-01T00:00:00Z or with fractional seconds
    let s = iso.trim().trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let mo: i64 = d.next()?.parse().ok()?;
    let da: i64 = d.next()?.parse().ok()?;
    let time = time.split('+').next()?.split('-').next()?;
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let se: f64 = t.next()?.parse().ok()?;
    days_from_civil(y, mo, da).map(|days| (days * 86400 + h * 3600 + mi * 60 + se as i64) * 1000)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

fn ms_to_rfc3339(ms: i64) -> String {
    let s = ms.div_euclid(1000);
    let days = s.div_euclid(86400);
    let rem = s.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let h = rem / 3600;
    let mi = (rem % 3600) / 60;
    let se = rem % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{se:02}Z")
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct MockHttp {
        status: u16,
        body: String,
    }

    impl TokenHttp for MockHttp {
        fn post_form(&self, url: &str, body: &str) -> Result<(u16, String), String> {
            assert!(url.contains("oauth2/token"));
            assert!(body.contains("grant_type=refresh_token"));
            assert!(body.contains("refresh_token=rt_old"));
            Ok((self.status, self.body.clone()))
        }
    }

    #[test]
    fn failed_refresh_never_returns_an_expired_token() {
        let mut doc = json!({"auth": {"key": "old", "refresh_token": "rt_old", "expires_at": "2020-01-01T00:00:00Z"}});
        let http = MockHttp {
            status: 503,
            body: "{}".into(),
        };
        assert!(ensure_access_token(&mut doc, 1_700_000_000_000, &http).is_none());
    }

    #[test]
    fn unexpired_key_is_returned_without_http() {
        let mut doc = json!({
            "https://auth.x.ai::abc": {
                "key": "tok_fresh",
                "expires_at": "2099-01-01T00:00:00Z"
            }
        });
        let http = MockHttp {
            status: 500,
            body: "{}".into(),
        };
        assert_eq!(
            ensure_access_token(&mut doc, 1_700_000_000_000, &http).as_deref(),
            Some("tok_fresh")
        );
    }

    #[test]
    fn expired_refreshes_and_rewrites_blob() {
        let mut doc = json!({
            "https://auth.x.ai::abc": {
                "key": "tok_old",
                "refresh_token": "rt_old",
                "expires_at": "2020-01-01T00:00:00Z"
            }
        });
        let http = MockHttp {
            status: 200,
            body: json!({
                "access_token": "tok_new",
                "refresh_token": "rt_new",
                "expires_in": 3600
            })
            .to_string(),
        };
        let tok = ensure_access_token(&mut doc, 1_700_000_000_000, &http).unwrap();
        assert_eq!(tok, "tok_new");
        let entry = &doc["https://auth.x.ai::abc"];
        assert_eq!(entry["key"], "tok_new");
        assert_eq!(entry["refresh_token"], "rt_new");
        assert!(entry["expires_at"].as_str().unwrap().starts_with("20"));
    }
}

#[cfg(test)]
mod fresh_access_tests {
    use super::*;
    #[test]
    fn fresh_access_excludes_renewal_window_expired_and_unknown_expiry() {
        let document = serde_json::json!({"auth":{"key":"fixture-access","expires_at":"2026-01-01T00:00:00Z"}});
        let expiry = parse_rfc3339_ms("2026-01-01T00:00:00Z").unwrap();
        assert_eq!(
            fresh_access_token(&document, expiry - REFRESH_BUFFER_MS - 1).as_deref(),
            Some("fixture-access")
        );
        assert!(fresh_access_token(&document, expiry - REFRESH_BUFFER_MS).is_none());
        assert!(fresh_access_token(&document, expiry).is_none());
        assert!(
            fresh_access_token(&serde_json::json!({"auth":{"key":"fixture-access"}}), 0).is_none()
        );
    }
}
