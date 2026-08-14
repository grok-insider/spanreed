//! Stable install id for community share device tracking.
//!
//! Generated once (setup or first share), stored under the spanreed config
//! dir. Sent as `X-Spanreed-Client`. Voting identity is the Grok Insider
//! account (Bearer); this id is only a device fingerprint (HMAC on server).

use std::path::PathBuf;

use crate::creds;

const FILE_NAME: &str = "client_id";

fn path() -> PathBuf {
    crate::app::config_dir().join(FILE_NAME)
}

fn looks_like_uuid(s: &str) -> bool {
    let s = s.trim();
    s.len() == 36
        && s.as_bytes().iter().enumerate().all(|(i, &b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// Load or create the install client id.
pub fn ensure() -> Result<String, String> {
    let p = path();
    if let Some(raw) = creds::read_file(&p) {
        let id = raw.trim().to_string();
        if looks_like_uuid(&id) {
            return Ok(id);
        }
    }
    let id = uuid_v4();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir client_id: {e}"))?;
    }
    std::fs::write(&p, format!("{id}\n")).map_err(|e| format!("write client_id: {e}"))?;
    Ok(id)
}

/// Minimal UUID v4 without extra deps (rand via getrandom not required — use
/// a simple entropy mix from time + process id + counter).
fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mut n = t ^ (pid << 64) ^ 0x9e37_79b9_7f4a_7c15;
    let mut bytes = [0u8; 16];
    for b in &mut bytes {
        n = n
            .wrapping_mul(0x5851_f42d_4c95_7f2d)
            .wrapping_add(0x1405_7b7e_f767_814f);
        *b = (n >> 56) as u8;
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_shape() {
        let id = uuid_v4();
        assert!(looks_like_uuid(&id), "{id}");
    }
}
