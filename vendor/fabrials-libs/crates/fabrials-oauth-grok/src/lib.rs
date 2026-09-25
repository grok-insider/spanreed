//! Grok CLI OAuth token extract, inject headers, and access-token refresh.

pub mod refresh;

pub const TOKEN_AUTH: &str = "xai-grok-cli";
pub const UPSTREAM_GROK_CLI: &str = "https://cli-chat-proxy.grok.com";
pub const UPSTREAM_XAI_API: &str = "https://api.x.ai";
pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const REFRESH_URL: &str = "https://auth.x.ai/oauth2/token";
pub const REFRESH_BUFFER_MS: i64 = 5 * 60 * 1000;

pub use refresh::{ensure_access_token, TokenHttp};

/// Pull the first non-empty `key` from a `~/.grok/auth.json`-shaped object.
pub fn token_from_doc(doc: &serde_json::Value) -> Option<String> {
    let obj = doc.as_object()?;
    for entry in obj.values() {
        let t = entry.get("key").and_then(|v| v.as_str())?.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    None
}

/// Headers the fabric sets when injecting a SuperGrok bearer.
pub fn inject_headers(token: &str) -> Vec<(String, String)> {
    vec![
        ("Authorization".into(), format!("Bearer {token}")),
        ("X-XAI-Token-Auth".into(), TOKEN_AUTH.into()),
    ]
}

/// Map CCP `subscription_tier_display` to slug + autosteer rank.
pub fn classify_plan(display: &str) -> (&'static str, u8) {
    let s = display.to_ascii_lowercase();
    let compact: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if s.contains("heavy") {
        return ("heavy", 5);
    }
    if s.contains("lite") {
        return ("lite", 2);
    }
    let super_g = s.contains("supergrok") || s.contains("super grok") || s.contains("super_grok");
    if (s.contains("premium+") || compact.contains("premiumplus") || s.contains("premium plus"))
        && !super_g
    {
        return ("premium-plus", 3);
    }
    if s.contains("premium") && !super_g {
        return ("premium", 2);
    }
    if super_g && s.contains("plus") {
        return ("plus", 4);
    }
    if super_g {
        return ("supergrok", 3);
    }
    ("acct", 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_from_doc_reads_key() {
        let doc = serde_json::json!({
            "https://auth.x.ai::abc": { "key": "  tok_live_1  ", "other": "nope" }
        });
        assert_eq!(token_from_doc(&doc).as_deref(), Some("tok_live_1"));
        assert!(token_from_doc(&serde_json::json!({})).is_none());
    }

    #[test]
    fn inject_headers_do_not_echo_in_log_shape() {
        let h = inject_headers("secret-token");
        assert_eq!(h[0], ("Authorization".into(), "Bearer secret-token".into()));
        assert_eq!(h[1], ("X-XAI-Token-Auth".into(), TOKEN_AUTH.into()));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn classify_heavy_and_premium_plus() {
        assert_eq!(classify_plan("SuperGrok Heavy").0, "heavy");
        assert_eq!(classify_plan("X Premium+").0, "premium-plus");
    }
}
