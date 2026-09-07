//! One completed API call's official usage (from Responses `usage`).

use serde::{Deserialize, Serialize};

const TICKS_PER_USD: f64 = 1_000_000_000.0;

/// One completed API call's official usage (from Responses `usage`).
///
/// List-price USD is computed by `fabrials-metrics` (needs the price table).
/// Subscription-internal `cost_usd_ticks` are captured for reference only.
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UsageRecord {
    pub ts_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    /// xAI `cost_in_usd_ticks` (1e9 ticks = $1). Zero when not provided.
    #[serde(default)]
    pub cost_usd_ticks: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Host identity that paid this hop (`grok/heavy`). Missing on old lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Fabric route: `grok` (cli-chat-proxy) or `xai` (api.x.ai).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    /// Upstream provider id when not implied by `route` (`codex`, `claude`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// SHA-256 hex of the proxy Bearer that authorized this hop. Never the plaintext key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_hash: Option<String>,
    /// `chat`, `stt`, `tts`, `realtime`, `image`, `video`, `models`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Wall time of the hop (WebSocket session or HTTP round-trip). Never audio length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// HTTP status or 101 after a WebSocket upgrade. None on legacy rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// Billing unit: `tokens`, `chars`, `audio_ms`, `images`, `video_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Amount in `unit` (character count, audio ms, image count, video ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<u64>,
}

pub const UNIT_TOKENS: &str = "tokens";
pub const UNIT_CHARS: &str = "chars";
pub const UNIT_AUDIO_MS: &str = "audio_ms";
pub const UNIT_IMAGES: &str = "images";
pub const UNIT_VIDEO_MS: &str = "video_ms";

impl UsageRecord {
    pub fn tokens_for_total(&self) -> u64 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens.saturating_add(self.output_tokens)
        }
    }

    /// Subscription-internal ticks from the API (not public list price).
    pub fn ticks_usd(&self) -> Option<f64> {
        if self.cost_usd_ticks > 0 {
            Some(self.cost_usd_ticks as f64 / TICKS_PER_USD)
        } else {
            None
        }
    }

    /// Failed hop: do not add list-price USD to a key budget.
    pub fn is_failed(&self) -> bool {
        self.status.is_some_and(|s| s >= 400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_for_total_prefers_total_field() {
        let r = UsageRecord {
            input_tokens: 3,
            output_tokens: 4,
            total_tokens: 10,
            ..UsageRecord::default()
        };
        assert_eq!(r.tokens_for_total(), 10);
        let r2 = UsageRecord {
            input_tokens: 3,
            output_tokens: 4,
            ..UsageRecord::default()
        };
        assert_eq!(r2.tokens_for_total(), 7);
    }

    #[test]
    fn ticks_usd_none_when_zero() {
        let r = UsageRecord::default();
        assert!(r.ticks_usd().is_none());
        let r = UsageRecord {
            cost_usd_ticks: 2_000_000_000,
            ..UsageRecord::default()
        };
        assert_eq!(r.ticks_usd(), Some(2.0));
    }

    #[test]
    fn failed_when_status_is_client_or_server_error() {
        assert!(!UsageRecord::default().is_failed());
        assert!(!UsageRecord {
            status: Some(101),
            ..UsageRecord::default()
        }
        .is_failed());
        assert!(UsageRecord {
            status: Some(502),
            ..UsageRecord::default()
        }
        .is_failed());
    }
}
