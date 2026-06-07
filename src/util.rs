//! Small formatting helpers shared by providers.

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Current unix time in milliseconds.
pub fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

/// Convert a JSON value that may be an ISO string, unix seconds, or unix ms
/// into an RFC3339 string (UTC). Mirrors upstream `ctx.util.toIso`.
pub fn to_iso(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            // Already RFC3339?
            if OffsetDateTime::parse(s, &Rfc3339).is_ok() {
                return Some(s.to_string());
            }
            // ISO-like missing tz -> assume UTC.
            if s.contains('T') && !s.ends_with('Z') && !s.contains('+') {
                let withz = format!("{s}Z");
                if OffsetDateTime::parse(&withz, &Rfc3339).is_ok() {
                    return Some(withz);
                }
            }
            // Numeric string?
            if let Ok(n) = s.parse::<f64>() {
                return ms_to_iso(numberish_to_ms(n));
            }
            None
        }
        serde_json::Value::Number(n) => {
            let f = n.as_f64()?;
            ms_to_iso(numberish_to_ms(f))
        }
        _ => None,
    }
}

/// Treat values below 1e10 as seconds, otherwise milliseconds.
fn numberish_to_ms(n: f64) -> i64 {
    if n.abs() < 1e10 {
        (n * 1000.0) as i64
    } else {
        n as i64
    }
}

pub fn ms_to_iso(ms: i64) -> Option<String> {
    let secs = ms.div_euclid(1000);
    let nanos = (ms.rem_euclid(1000) * 1_000_000) as i128;
    let total = secs as i128 * 1_000_000_000 + nanos;
    OffsetDateTime::from_unix_timestamp_nanos(total)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

/// Title-case a plan label ("pro" -> "Pro", "team max" -> "Team Max").
pub fn plan_label(value: &str) -> String {
    let v = value.trim();
    if v.is_empty() {
        return String::new();
    }
    v.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Cents -> dollars (rounded to cents).
pub fn cents_to_dollars(cents: f64) -> f64 {
    (cents / 100.0 * 100.0).round() / 100.0
}

/// Decode a JWT's `exp` claim (unix seconds) without verifying the signature.
pub fn jwt_exp_ms(token: &str) -> Option<i64> {
    let payload_b64 = token.split('.').nth(1)?;
    let bytes = base64url_decode(payload_b64)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let exp = json.get("exp")?.as_f64()?;
    Some((exp * 1000.0) as i64)
}

/// Minimal base64url decoder (no padding required).
fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut lut = [255u8; 256];
    for (i, &c) in ALPHA.iter().enumerate() {
        lut[c as usize] = i as u8;
    }
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &b in input.as_bytes() {
        if b == b'=' {
            break;
        }
        let v = lut[b as usize];
        if v == 255 {
            return None;
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}
