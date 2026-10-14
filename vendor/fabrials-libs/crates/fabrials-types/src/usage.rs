//! One completed API call's official usage (from Responses `usage`).

use serde::{Deserialize, Serialize};

const TICKS_PER_USD: f64 = 1_000_000_000.0;

/// One completed API call's official usage (from Responses `usage`).
///
/// List-price USD is computed by `fabrials-pricing` (needs the price table).
/// Subscription-internal `cost_usd_ticks` are captured for reference only.
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[cfg_attr(feature = "contracts", ts(rename = "UsageRecord"))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HopRecord {
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
    /// Milliseconds from the start of the hop (same origin as `duration_ms`) to the first
    /// byte of the upstream response body the relay read. A lower bound on the upstream's
    /// first byte: the first read can already hold bytes that arrived with the headers.
    /// None where no body is piped (WebSocket tunnel, rejections, send errors).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_byte_ms: Option<u64>,
    /// Milliseconds from the start of the hop (same origin as `duration_ms`) to the first
    /// streamed event that carries content: text, reasoning or tool-call argument deltas.
    /// Spans retries, so it is the latency the client saw. Reasoning an upstream hides is
    /// not content. None unless the upstream streamed events the relay could read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<u64>,
    /// HTTP status or 101 after a WebSocket upgrade. None on legacy rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// Billing unit: `tokens`, `chars`, `audio_ms`, `images`, `video_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Amount in `unit` (character count, audio ms, image count, video ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<u64>,
    /// Hosted tool types from the request (`x_search`, `web_search`). Not a hop kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosted_tools: Option<Vec<String>>,
}

pub const UNIT_TOKENS: &str = "tokens";
pub const UNIT_CHARS: &str = "chars";
pub const UNIT_AUDIO_MS: &str = "audio_ms";
pub const UNIT_IMAGES: &str = "images";
pub const UNIT_VIDEO_MS: &str = "video_ms";

impl HopRecord {
    pub fn has_hosted_search(&self) -> bool {
        self.hosted_tools.as_ref().is_some_and(|tools| {
            tools
                .iter()
                .any(|name| name == "x_search" || name == "web_search")
        })
    }

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

    /// Enforce `first_byte_ms <= ttft_ms <= duration_ms` by dropping the values that break
    /// it. A timing that cannot be ordered is not trusted, and dropping is safe because
    /// every consumer reads `None` as "not measured".
    pub fn normalize_timings(&mut self) {
        if let Some(duration) = self.duration_ms {
            if self.first_byte_ms.is_some_and(|ms| ms > duration) {
                self.first_byte_ms = None;
            }
            if self.ttft_ms.is_some_and(|ms| ms > duration) {
                self.ttft_ms = None;
            }
        }
        if let (Some(first_byte), Some(ttft)) = (self.first_byte_ms, self.ttft_ms) {
            if first_byte > ttft {
                self.ttft_ms = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_for_total_prefers_total_field() {
        let r = HopRecord {
            input_tokens: 3,
            output_tokens: 4,
            total_tokens: 10,
            ..HopRecord::default()
        };
        assert_eq!(r.tokens_for_total(), 10);
        let r2 = HopRecord {
            input_tokens: 3,
            output_tokens: 4,
            ..HopRecord::default()
        };
        assert_eq!(r2.tokens_for_total(), 7);
    }

    #[test]
    fn ticks_usd_none_when_zero() {
        let r = HopRecord::default();
        assert!(r.ticks_usd().is_none());
        let r = HopRecord {
            cost_usd_ticks: 2_000_000_000,
            ..HopRecord::default()
        };
        assert_eq!(r.ticks_usd(), Some(2.0));
    }

    #[test]
    fn failed_when_status_is_client_or_server_error() {
        assert!(!HopRecord::default().is_failed());
        assert!(!HopRecord {
            status: Some(101),
            ..HopRecord::default()
        }
        .is_failed());
        assert!(HopRecord {
            status: Some(502),
            ..HopRecord::default()
        }
        .is_failed());
    }

    fn timed(first_byte: Option<u64>, ttft: Option<u64>, duration: Option<u64>) -> HopRecord {
        HopRecord {
            first_byte_ms: first_byte,
            ttft_ms: ttft,
            duration_ms: duration,
            ..HopRecord::default()
        }
    }

    #[test]
    fn normalize_keeps_ordered_timings() {
        let mut r = timed(Some(80), Some(900), Some(1_500));
        r.normalize_timings();
        assert_eq!(r, timed(Some(80), Some(900), Some(1_500)));
        // Equal values are ordered: the first read can hold the first content.
        let mut same = timed(Some(700), Some(700), Some(700));
        same.normalize_timings();
        assert_eq!(same, timed(Some(700), Some(700), Some(700)));
    }

    #[test]
    fn normalize_drops_timings_past_the_duration() {
        let mut r = timed(Some(2_000), Some(3_000), Some(1_500));
        r.normalize_timings();
        assert_eq!(r, timed(None, None, Some(1_500)));
        let mut only_ttft = timed(Some(80), Some(1_501), Some(1_500));
        only_ttft.normalize_timings();
        assert_eq!(only_ttft, timed(Some(80), None, Some(1_500)));
    }

    #[test]
    fn normalize_drops_ttft_that_precedes_the_first_byte() {
        let mut r = timed(Some(900), Some(100), Some(1_500));
        r.normalize_timings();
        assert_eq!(r, timed(Some(900), None, Some(1_500)));
    }

    #[test]
    fn normalize_without_a_duration_only_orders_the_pair() {
        let mut r = timed(Some(80), Some(900), None);
        r.normalize_timings();
        assert_eq!(r, timed(Some(80), Some(900), None));
        let mut inverted = timed(Some(900), Some(80), None);
        inverted.normalize_timings();
        assert_eq!(inverted, timed(Some(900), None, None));
    }

    #[test]
    fn timings_are_absent_from_old_json_and_from_new_json_when_unset() {
        let old = r#"{"ts_ms":1,"duration_ms":40}"#;
        let parsed: HopRecord = serde_json::from_str(old).unwrap();
        assert_eq!(parsed.first_byte_ms, None);
        assert_eq!(parsed.ttft_ms, None);
        let out = serde_json::to_string(&parsed).unwrap();
        assert!(!out.contains("first_byte_ms"), "{out}");
        assert!(!out.contains("ttft_ms"), "{out}");
    }

    #[test]
    fn timings_round_trip_through_json() {
        let record = timed(Some(80), Some(900), Some(1_500));
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains(r#""first_byte_ms":80"#), "{json}");
        assert!(json.contains(r#""ttft_ms":900"#), "{json}");
        assert_eq!(serde_json::from_str::<HopRecord>(&json).unwrap(), record);
    }
}
