//! Shared hop accounting and persistence retry policy.
use fabrials_core::hop::{HopKind, Transport};
use fabrials_model::UsageRecord;
use serde::Deserialize;

pub fn persist_vps_with<P, Q>(
    rec: &UsageRecord,
    source: &str,
    owner: Option<&str>,
    mut database: P,
    mut enqueue: Q,
    on_log: &dyn Fn(&str),
) where
    P: FnMut(&UsageRecord, &str, Option<&str>) -> Result<(), String>,
    Q: FnMut(&UsageRecord, &str, Option<&str>) -> Result<(), String>,
{
    if database(rec, source, owner).is_ok() {
        return;
    }
    let message = if enqueue(rec, source, owner).is_ok() {
        "usage persistence deferred"
    } else {
        "usage persistence unavailable"
    };
    on_log(&persistence_diagnostic(message, rec, owner));
}

pub fn persistence_diagnostic(prefix: &str, rec: &UsageRecord, owner: Option<&str>) -> String {
    persistence_identity_diagnostic(prefix, rec.request_id.as_deref(), owner)
}

pub fn persistence_identity_diagnostic(
    prefix: &str,
    request_id: Option<&str>,
    owner: Option<&str>,
) -> String {
    format!(
        "{prefix} request_id={} owner={}",
        safe_log_field(request_id.unwrap_or("-")),
        safe_log_field(owner.unwrap_or("-")),
    )
}

/// Operator log line: never includes Authorization, cookies, or bodies.
pub fn log_line(rec: &UsageRecord, owner: Option<&str>) -> String {
    let usd = fabrials_metrics::list_cost_usd(rec).unwrap_or(0.0);
    format!(
        "hop request_id={} owner={} provider={} account={} kind={} model={} status={} duration_ms={} unit={} quantity={} usd={:.6}",
        safe_log_field(rec.request_id.as_deref().unwrap_or("-")),
        safe_log_field(owner.unwrap_or("-")),
        safe_log_field(rec.provider.as_deref().unwrap_or("-")),
        safe_log_field(rec.account_id.as_deref().unwrap_or("-")),
        safe_log_field(rec.kind.as_deref().unwrap_or("-")),
        safe_log_field(rec.model.as_deref().unwrap_or("-")),
        rec.status.map(|s| s.to_string()).unwrap_or_else(|| "-".into()),
        rec.duration_ms.map(|d| d.to_string()).unwrap_or_else(|| "-".into()),
        safe_log_field(rec.unit.as_deref().unwrap_or("-")),
        rec.quantity.map(|q| q.to_string()).unwrap_or_else(|| "-".into()),
        usd,
    )
}

fn safe_log_field(value: &str) -> String {
    let mut out = String::with_capacity(value.len().min(256));
    for ch in value.chars().take(256) {
        if ch.is_control() || ch.is_whitespace() || ch == '=' {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    if out.is_empty() {
        "-".into()
    } else {
        out
    }
}

pub fn new_request_id() -> String {
    let mut b = [0u8; 16];
    if getrandom::getrandom(&mut b).is_err() {
        // Request IDs are persistence idempotency keys. Entropy failure must
        // not collapse every request to the same all-zero identifier.
        use sha2::{Digest, Sha256};
        use std::sync::atomic::{AtomicU64, Ordering};

        static FALLBACK_SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let sequence = FALLBACK_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let digest = Sha256::new()
            .chain_update(timestamp.to_le_bytes())
            .chain_update(std::process::id().to_le_bytes())
            .chain_update(sequence.to_le_bytes())
            .finalize();
        b.copy_from_slice(&digest[..16]);
    }
    hex::encode(b)
}

/// Unicode scalar count of TTS JSON `text` or `input`.
pub fn tts_char_count(body: &[u8]) -> u64 {
    #[derive(Deserialize)]
    struct TtsFields {
        text: Option<String>,
        input: Option<String>,
    }
    let Ok(fields) = serde_json::from_slice::<TtsFields>(body) else {
        return 0;
    };
    fields
        .text
        .as_deref()
        .or(fields.input.as_deref())
        .map(|s| s.chars().count() as u64)
        .unwrap_or(0)
}

/// xAI STT JSON `duration` (seconds of audio) → milliseconds.
pub fn stt_audio_duration_ms(body: &[u8]) -> Option<u64> {
    #[derive(Deserialize)]
    struct UsageFields {
        duration: Option<f64>,
    }
    #[derive(Deserialize)]
    struct DurationFields {
        duration: Option<f64>,
        audio_duration: Option<f64>,
        usage: Option<UsageFields>,
    }
    let fields: DurationFields = serde_json::from_slice(body).ok()?;
    let secs = fields
        .duration
        .or(fields.audio_duration)
        .or_else(|| fields.usage.and_then(|usage| usage.duration))?;
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    Some((secs * 1000.0).round() as u64)
}

/// `model` query string on WS paths (`/v1/realtime?model=grok-voice-latest`).
pub fn model_from_query(path: &str) -> Option<String> {
    validated_model_from_query(path).ok().flatten()
}

pub fn validated_model_from_query(path: &str) -> Result<Option<String>, ()> {
    let Some((_, q)) = path.split_once('?') else {
        return Ok(None);
    };
    let mut found = None;
    for (key, value) in url::form_urlencoded::parse(q.as_bytes()) {
        if key == "model" {
            if found.is_some() {
                return Err(());
            }
            let model = value.trim();
            if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
                return Err(());
            }
            found = Some(model.to_string());
        }
    }
    Ok(found)
}

/// Compact request-side values needed after the request body has moved into
/// the outbound HTTP client. This deliberately never retains prompt/audio
/// bytes.
#[derive(Debug, Default)]
pub struct MediaRequestSummary {
    model: Option<String>,
    tts_chars: Option<u64>,
    image_count: Option<u64>,
    video_duration_ms: Option<u64>,
}

pub fn summarize_media_request(kind: HopKind, request_body: &[u8]) -> MediaRequestSummary {
    match kind {
        HopKind::Tts => MediaRequestSummary {
            tts_chars: Some(tts_char_count(request_body)),
            ..MediaRequestSummary::default()
        },
        HopKind::Image => MediaRequestSummary {
            model: json_model(request_body),
            image_count: Some(image_count(request_body)),
            ..MediaRequestSummary::default()
        },
        HopKind::Video => MediaRequestSummary {
            model: json_model(request_body),
            video_duration_ms: video_duration_ms(request_body, &[]),
            ..MediaRequestSummary::default()
        },
        _ => MediaRequestSummary::default(),
    }
}

/// Fill billing units: TTS characters, STT audio length. Never overwrites wall `duration_ms`.
pub fn apply_media_units_from_summary(
    rec: &mut UsageRecord,
    kind: HopKind,
    transport: Transport,
    request: &MediaRequestSummary,
    response_body: &[u8],
) {
    match kind {
        HopKind::Tts => {
            if transport == Transport::Http || rec.model.as_deref().unwrap_or("").is_empty() {
                rec.model = Some("tts".into());
            }
            let n = request.tts_chars.unwrap_or(0);
            rec.unit = Some(fabrials_model::UNIT_CHARS.into());
            rec.quantity = Some(n);
            rec.input_tokens = n;
        }
        HopKind::Stt => {
            if transport == Transport::Http {
                rec.model = Some("stt-batch".into());
                if let Some(ms) = stt_audio_duration_ms(response_body) {
                    rec.unit = Some(fabrials_model::UNIT_AUDIO_MS.into());
                    rec.quantity = Some(ms);
                }
            } else {
                if rec.model.as_deref().unwrap_or("").is_empty() {
                    rec.model = Some("stt".into());
                }
                rec.unit = Some(fabrials_model::UNIT_AUDIO_MS.into());
                rec.quantity = rec.duration_ms;
            }
        }
        HopKind::Realtime => {
            if rec.model.as_deref().unwrap_or("").is_empty() {
                rec.model = Some("realtime".into());
            }
            rec.unit = Some(fabrials_model::UNIT_AUDIO_MS.into());
            rec.quantity = rec.duration_ms;
        }
        HopKind::Image => {
            if let Some(model) = &request.model {
                rec.model = Some(model.clone());
            }
            rec.unit = Some(fabrials_model::UNIT_IMAGES.into());
            rec.quantity = Some(request.image_count.unwrap_or(1));
        }
        HopKind::Video => {
            if let Some(model) = &request.model {
                rec.model = Some(model.clone());
            }
            rec.unit = Some(fabrials_model::UNIT_VIDEO_MS.into());
            rec.quantity = video_duration_ms(&[], response_body).or(request.video_duration_ms);
        }
        _ => {}
    }
}

pub fn apply_media_units(
    rec: &mut UsageRecord,
    kind: HopKind,
    transport: Transport,
    request_body: &[u8],
    response_body: &[u8],
) {
    let request = summarize_media_request(kind, request_body);
    apply_media_units_from_summary(rec, kind, transport, &request, response_body);
}

fn json_model(body: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct ModelFields {
        model: Option<String>,
    }
    let fields: ModelFields = serde_json::from_slice(body).ok()?;
    fields
        .model
        .map(|model| model.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn image_count(body: &[u8]) -> u64 {
    #[derive(Deserialize)]
    struct ImageFields {
        n: Option<u64>,
    }
    let Ok(fields) = serde_json::from_slice::<ImageFields>(body) else {
        return 1;
    };
    fields.n.filter(|n| *n > 0).unwrap_or(1)
}

fn video_duration_ms(request_body: &[u8], response_body: &[u8]) -> Option<u64> {
    #[derive(Deserialize)]
    struct VideoFields {
        duration: Option<f64>,
        seconds: Option<f64>,
    }
    for body in [response_body, request_body] {
        let Ok(fields) = serde_json::from_slice::<VideoFields>(body) else {
            continue;
        };
        let secs = fields.duration.or(fields.seconds);
        if let Some(secs) = secs {
            if secs.is_finite() && secs >= 0.0 {
                return Some((secs * 1000.0).round() as u64);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tts_counts_text_or_input() {
        assert_eq!(tts_char_count(r#"{"text":"héllo"}"#.as_bytes()), 5);
        assert_eq!(tts_char_count(br#"{"input":"abc"}"#), 3);
        assert_eq!(tts_char_count(b"not-json"), 0);
    }

    #[test]
    fn stt_duration_seconds_to_ms() {
        assert_eq!(
            stt_audio_duration_ms(br#"{"duration":0.4,"text":"hi"}"#),
            Some(400)
        );
        assert_eq!(
            stt_audio_duration_ms(br#"{"audio_duration":1.5}"#),
            Some(1500)
        );
        assert_eq!(stt_audio_duration_ms(br#"{}"#), None);
    }

    #[test]
    fn apply_sets_batch_model_and_audio_ms() {
        let mut rec = UsageRecord {
            duration_ms: Some(12_000),
            ..UsageRecord::default()
        };
        apply_media_units(
            &mut rec,
            HopKind::Stt,
            Transport::Http,
            b"",
            br#"{"duration":0.4}"#,
        );
        assert_eq!(rec.model.as_deref(), Some("stt-batch"));
        assert_eq!(rec.duration_ms, Some(12_000), "wall time stays wall time");
        assert_eq!(rec.unit.as_deref(), Some("audio_ms"));
        assert_eq!(rec.quantity, Some(400));
    }

    #[test]
    fn tts_sets_chars_unit_not_only_tokens() {
        let mut rec = UsageRecord::default();
        apply_media_units(
            &mut rec,
            HopKind::Tts,
            Transport::Http,
            br#"{"text":"abc"}"#,
            b"",
        );
        assert_eq!(rec.unit.as_deref(), Some("chars"));
        assert_eq!(rec.quantity, Some(3));
        assert_eq!(rec.model.as_deref(), Some("tts"));
    }

    #[test]
    fn compact_media_summary_survives_after_large_request_is_released() {
        let text = "x".repeat(1024 * 1024);
        let body = serde_json::to_vec(&serde_json::json!({"text": text})).unwrap();
        let request = summarize_media_request(HopKind::Tts, &body);
        drop(body);

        let mut rec = UsageRecord::default();
        apply_media_units_from_summary(&mut rec, HopKind::Tts, Transport::Http, &request, &[]);
        assert_eq!(rec.quantity, Some(1024 * 1024));
        assert!(request.model.is_none());
    }

    #[test]
    fn image_request_counts_n() {
        let mut rec = UsageRecord {
            model: Some("grok-imagine-image-quality".into()),
            ..UsageRecord::default()
        };
        apply_media_units(
            &mut rec,
            HopKind::Image,
            Transport::Http,
            br#"{"model":"grok-imagine-image-quality","n":2,"prompt":"x"}"#,
            b"",
        );
        assert_eq!(rec.unit.as_deref(), Some("images"));
        assert_eq!(rec.quantity, Some(2));
    }

    #[test]
    fn log_line_has_ids_not_secrets() {
        let rec = UsageRecord {
            request_id: Some("abc".into()),
            kind: Some("tts".into()),
            status: Some(200),
            unit: Some("chars".into()),
            quantity: Some(3),
            ..UsageRecord::default()
        };
        let line = log_line(&rec, Some("42"));
        assert!(line.contains("request_id=abc"));
        assert!(line.contains("owner=42"));
        assert!(line.contains("kind=tts"));
        let low = line.to_ascii_lowercase();
        assert!(!low.contains("authorization"));
        assert!(!low.contains("cookie"));
        assert!(!low.contains("bearer "));
    }

    #[test]
    fn log_fields_are_single_line_bounded_and_unambiguous() {
        let rec = UsageRecord {
            request_id: Some("ok\nowner=attacker".into()),
            model: Some("m".repeat(300)),
            ..UsageRecord::default()
        };
        let line = log_line(&rec, Some("42\r\nforged=yes"));
        assert_eq!(line.lines().count(), 1);
        assert!(!line.contains("owner=attacker"));
        assert!(!line.contains("forged=yes"));
        assert!(!line.contains(&"m".repeat(257)));
    }

    #[test]
    fn vps_failure_is_deferred_without_logging_backend_details() {
        let rec = UsageRecord {
            request_id: Some("req-accounting".into()),
            ..UsageRecord::default()
        };
        let mut queued = false;
        let logs = std::sync::Mutex::new(Vec::new());
        persist_vps_with(
            &rec,
            "ai-relay",
            Some("owner-42"),
            |_, _, _| Err("postgres password=super-secret".into()),
            |record, source, owner| {
                queued = true;
                assert_eq!(record.request_id.as_deref(), Some("req-accounting"));
                assert_eq!(source, "ai-relay");
                assert_eq!(owner, Some("owner-42"));
                Ok(())
            },
            &|line| logs.lock().unwrap().push(line.to_string()),
        );
        assert!(queued);
        let log = logs.lock().unwrap().join("\n");
        assert!(log.contains("usage persistence deferred"));
        assert!(log.contains("request_id=req-accounting"));
        assert!(log.contains("owner=owner-42"));
        assert!(!log.contains("postgres"));
        assert!(!log.contains("super-secret"));
    }

    #[test]
    fn model_from_query_reads_ws_model() {
        assert_eq!(
            model_from_query("/v1/realtime?model=grok-voice-latest"),
            Some("grok-voice-latest".into())
        );
        assert_eq!(model_from_query("/v1/stt"), None);
        assert_eq!(
            validated_model_from_query("/v1/realtime?model=grok%2Dvoice")
                .unwrap()
                .as_deref(),
            Some("grok-voice")
        );
        assert!(validated_model_from_query("/v1/realtime?model=a&model=b").is_err());
        assert!(validated_model_from_query("/v1/realtime?model=bad%0Amodel").is_err());
    }
}

pub fn request_model(body: &[u8]) -> Result<Option<String>, ()> {
    #[derive(serde::Deserialize)]
    struct ModelEnvelope {
        model: Option<String>,
    }
    let Ok(envelope) = serde_json::from_slice::<ModelEnvelope>(body) else {
        // Non-JSON payloads are valid on multipart/binary media routes.
        return if body
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace())
            == Some(b'{')
        {
            Err(())
        } else {
            Ok(None)
        };
    };
    let Some(model) = envelope.model else {
        return Ok(None);
    };
    let model = model.trim();
    if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
        return Err(());
    }
    Ok(Some(model.to_string()))
}
