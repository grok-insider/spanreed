//! Shared JSON helpers for API-key quota providers.
//!
//! Credential material stays in request headers. Errors report HTTP status only.

use crate::creds;
use crate::http::Request;
use crate::model::{MetricKind, MetricLine, ProgressFormat};

pub(super) fn env_any(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| creds::env(key))
}

pub(super) fn number(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|n| n as f64))
        .or_else(|| value.as_u64().map(|n| n as f64))
        .or_else(|| {
            value
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse().ok())
        })
}

pub(super) fn field(value: &serde_json::Value, key: &str) -> Option<f64> {
    value.get(key).and_then(number)
}

pub(super) fn text_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub(super) fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Accept an https origin, or http only for loopback and private hosts.
pub(super) fn allowed_base(raw: &str, private_http: bool) -> Result<String, String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err("endpoint is empty".into());
    }
    let (scheme, rest) = raw
        .split_once("://")
        .ok_or_else(|| "endpoint must include https://".to_string())?;
    if rest.contains('@') {
        return Err("endpoint must not include credentials".into());
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains(' ') {
        return Err("endpoint host is invalid".into());
    }
    match scheme {
        "https" => Ok(raw.to_string()),
        "http" if private_http && private_host(host) => Ok(raw.to_string()),
        "http" => Err("plain HTTP is only allowed for a loopback or private gateway".into()),
        _ => Err("endpoint must be https".into()),
    }
}

fn private_host(hostport: &str) -> bool {
    let host = hostport
        .trim_start_matches('[')
        .split(']')
        .next()
        .unwrap_or(hostport);
    let host = host.split(':').next().unwrap_or(host);
    if matches!(host, "localhost" | "127.0.0.1" | "::1" | "0.0.0.0") || host.ends_with(".local") {
        return true;
    }
    let octets: Vec<u8> = host.split('.').filter_map(|p| p.parse().ok()).collect();
    if octets.len() != 4 {
        return false;
    }
    match octets[0] {
        10 | 127 => true,
        192 => octets[1] == 168,
        172 => (16..=31).contains(&octets[1]),
        169 => octets[1] == 254,
        _ => false,
    }
}

pub(super) fn get_bearer(url: &str, token: &str) -> Result<serde_json::Value, String> {
    let resp = Request::get(url)
        .bearer(token)
        .header("Accept", "application/json")
        .send()?;
    ok_json("Request", &resp)
}

pub(super) fn get_headers(
    url: &str,
    headers: &[(&str, &str)],
) -> Result<serde_json::Value, String> {
    let mut req = Request::get(url).header("Accept", "application/json");
    for (key, value) in headers {
        req = req.header(*key, *value);
    }
    ok_json("Request", &req.send()?)
}

pub(super) fn post_bearer(url: &str, token: &str, body: &str) -> Result<serde_json::Value, String> {
    let resp = Request::post(url)
        .bearer(token)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .body(body)
        .send()?;
    ok_json("Request", &resp)
}

pub(super) fn post_headers(
    url: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Result<serde_json::Value, String> {
    let mut req = Request::post(url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json");
    for (key, value) in headers {
        req = req.header(*key, *value);
    }
    ok_json("Request", &req.body(body).send()?)
}

pub(super) fn ok_json(
    what: &str,
    resp: &crate::http::Response,
) -> Result<serde_json::Value, String> {
    if resp.is_auth_error() {
        return Err(format!(
            "{what} rejected the credentials (HTTP {}).",
            resp.status
        ));
    }
    if !(200..300).contains(&resp.status) {
        return Err(format!("{what} failed (HTTP {}).", resp.status));
    }
    resp.json()
        .ok_or_else(|| format!("{what} response was not JSON."))
}

pub(super) fn usd(amount: f64) -> String {
    format!("${amount:.2}")
}

pub(super) fn text_line(label: &str, value: impl Into<String>) -> MetricLine {
    MetricLine::text(MetricKind::Quota, label, value)
}

pub(super) fn count_line(
    label: &str,
    used: f64,
    limit: f64,
    suffix: &str,
    resets_at: Option<String>,
) -> MetricLine {
    MetricLine::Progress {
        kind: MetricKind::Quota,
        label: label.into(),
        used,
        limit,
        format: ProgressFormat::Count {
            suffix: suffix.into(),
        },
        resets_at,
        color: None,
    }
}

pub(super) fn used_percent(used: f64, limit: f64) -> Option<f64> {
    if limit > 0.0 && used.is_finite() && limit.is_finite() {
        Some((used / limit) * 100.0)
    } else {
        None
    }
}

pub(super) fn query_escape(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub(super) fn safe_segment(raw: &str, max: usize) -> Option<&str> {
    let raw = raw.trim();
    if (1..=max).contains(&raw.len())
        && raw != "."
        && raw != ".."
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        Some(raw)
    } else {
        None
    }
}

pub(super) fn utc_ymd(days_ago: i64) -> String {
    let now = time::OffsetDateTime::from_unix_timestamp(crate::util::now_ms() / 1000)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let dt = now - time::Duration::days(days_ago);
    format!(
        "{:04}-{:02}-{:02}",
        dt.year(),
        u8::from(dt.month()),
        dt.day()
    )
}

/// HTTPS, or HTTP only for loopback (`localhost`, `127.0.0.0/8`, `::1`).
pub(super) fn allowed_loopback(raw: &str) -> Result<String, String> {
    let base = allowed_base(raw, true)?;
    if base.starts_with("http://") {
        let host = base
            .trim_start_matches("http://")
            .split(['/', '?', '#'])
            .next()
            .unwrap_or("");
        let host = host
            .trim_start_matches('[')
            .split(']')
            .next()
            .unwrap_or(host);
        let host = host.split(':').next().unwrap_or(host);
        let parts: Vec<&str> = host.split('.').collect();
        let octets: Vec<u8> = parts.iter().filter_map(|part| part.parse().ok()).collect();
        let loopback = matches!(host, "localhost" | "::1")
            || (parts.len() == 4 && octets.len() == 4 && octets[0] == 127);
        if !loopback {
            return Err("plain HTTP is only allowed for a loopback gateway".into());
        }
    }
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_and_loopback_bases() {
        assert!(allowed_base("https://api.example.com/v1", false).is_ok());
        assert!(allowed_base("http://127.0.0.1:8088", true).is_ok());
        assert!(allowed_base("http://evil.example", true).is_err());
        assert!(allowed_base("https://user:pw@api.example.com", false).is_err());
    }

    #[test]
    fn reads_numeric_strings() {
        assert_eq!(number(&serde_json::json!("12.5")), Some(12.5));
        assert_eq!(field(&serde_json::json!({"n": 3}), "n"), Some(3.0));
    }

    #[test]
    fn escapes_query_and_rejects_path_segments() {
        assert_eq!(query_escape("a b"), "a%20b");
        assert_eq!(safe_segment("acct_1", 16), Some("acct_1"));
        assert!(safe_segment("..", 16).is_none());
        assert!(safe_segment("a/b", 16).is_none());
        assert!(allowed_loopback("http://127.0.0.1:8088").is_ok());
        assert!(allowed_loopback("http://10.0.0.5").is_err());
    }
}
