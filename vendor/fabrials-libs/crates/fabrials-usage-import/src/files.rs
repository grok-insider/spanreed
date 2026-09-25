//! File adapters own format I/O; only normalized records escape this boundary.
use super::formats::sessions::{self as readers, UnifiedMessage};
use fabrials_types::consumption::{
    ConsumptionRecord, CostOrigin, Granularity, Tokens, UsageCost, UsageOrigin,
};
use sha2::{Digest, Sha256};
use std::path::Path;

/// One local client's session reader. `read` gets the path and whether it is
/// a SQLite database.
pub trait SessionReader: Sync {
    fn client(&self) -> &'static str;
    fn read(&self, path: &Path, sqlite: bool) -> Vec<UnifiedMessage>;
}

macro_rules! session_readers {
    ($($name:ident => $client:literal, |$path:ident, $sqlite:ident| $body:expr;)*) => {
        $(
            #[derive(Debug, Clone, Copy, Default)]
            pub struct $name;

            impl SessionReader for $name {
                fn client(&self) -> &'static str {
                    $client
                }
                fn read(&self, $path: &Path, $sqlite: bool) -> Vec<UnifiedMessage> {
                    $body
                }
            }
        )*

        /// Every wired client, in catalog order. The catalog is checked against
        /// this registry, so a client is wired in exactly one place.
        pub static READERS: &[&dyn SessionReader] = &[$(&$name),*];
    };
}

session_readers! {
    OpencodeReader => "opencode", |path, sqlite| {
        if sqlite {
            readers::opencode::parse_opencode_sqlite(path)
        } else {
            readers::opencode::parse_opencode_file(path)
                .into_iter()
                .collect()
        }
    };
    ClaudeReader => "claude", |path, _sqlite| readers::claudecode::parse_claude_file(path);
    CodexReader => "codex", |path, _sqlite| readers::codex::parse_codex_file(path);
    CursorReader => "cursor", |path, _sqlite| readers::cursor::parse_cursor_file(path);
    GeminiReader => "gemini", |path, _sqlite| readers::gemini::parse_gemini_file(path);
    AmpReader => "amp", |path, _sqlite| readers::amp::parse_amp_file(path);
    DroidReader => "droid", |path, _sqlite| readers::droid::parse_droid_file(path);
    OpenclawReader => "openclaw", |path, sqlite| {
        if sqlite {
            readers::openclaw::parse_openclaw_sqlite(path)
        } else if path.file_name().is_some_and(|n| n == "sessions.json") {
            readers::openclaw::parse_openclaw_index(path)
        } else {
            readers::openclaw::parse_openclaw_transcript(path)
        }
    };
    PiReader => "pi", |path, _sqlite| readers::pi::parse_pi_file(path);
    KimiReader => "kimi", |path, _sqlite| readers::kimi::parse_kimi_file(path);
    QwenReader => "qwen", |path, _sqlite| readers::qwen::parse_qwen_file(path);
    RoocodeReader => "roocode", |path, _sqlite| readers::roocode::parse_roocode_file(path);
    KilocodeReader => "kilocode", |path, _sqlite| readers::kilocode::parse_kilocode_file(path);
    MuxReader => "mux", |path, _sqlite| readers::mux::parse_mux_file(path);
    KiloReader => "kilo", |path, _sqlite| readers::kilo::parse_kilo_sqlite(path);
    CrushReader => "crush", |path, _sqlite| readers::crush::parse_crush_sqlite(path);
    HermesReader => "hermes", |path, _sqlite| readers::hermes::parse_hermes_sqlite(path);
    CopilotReader => "copilot", |path, _sqlite| readers::copilot::parse_copilot_file(path);
    GooseReader => "goose", |path, _sqlite| readers::goose::parse_goose_sqlite(path);
    CodebuffReader => "codebuff", |path, _sqlite| readers::codebuff::parse_codebuff_file(path);
    AntigravityReader => "antigravity", |path, _sqlite| readers::antigravity::parse_antigravity_file(path);
    ZedReader => "zed", |path, _sqlite| readers::zed::parse_zed_sqlite(path);
    KiroReader => "kiro", |path, sqlite| {
        if sqlite {
            readers::kiro::parse_kiro_sqlite(path)
        } else {
            readers::kiro::parse_kiro_file(path)
        }
    };
    TraeReader => "trae", |path, _sqlite| readers::trae::parse_trae_file("trae", path);
    WarpReader => "warp", |path, _sqlite| readers::warp::parse_warp_file(path);
    ClineReader => "cline", |path, _sqlite| readers::cline::parse_cline_file(path);
    GjcReader => "gjc", |path, _sqlite| readers::gjc::parse_gjc_file(path);
    GrokReader => "grok", |path, _sqlite| readers::grok::parse_grok_file(path);
    JcodeReader => "jcode", |path, _sqlite| readers::jcode::parse_jcode_file(path);
    CommandcodeReader => "commandcode", |path, _sqlite| readers::commandcode::parse_commandcode_file(path);
    MicodeReader => "micode", |path, _sqlite| readers::micode::parse_micode_sqlite(path);
    AntigravityCliReader => "antigravity-cli", |path, _sqlite| readers::antigravity_cli::parse_antigravity_cli_file(path);
    JunieReader => "junie", |path, _sqlite| readers::junie::parse_junie_file(path);
    ZcodeReader => "zcode", |path, sqlite| {
        if sqlite {
            readers::zcode::parse_zcode_sqlite(path)
        } else {
            readers::zcode::parse_zcode_file(path)
        }
    };
    OpencodereviewReader => "opencodereview", |path, _sqlite| readers::opencodereview::parse_opencodereview_file(path);
    CodebuddyReader => "codebuddy", |path, _sqlite| readers::codebuddy::parse_codebuddy_file(path);
    WorkbuddyReader => "workbuddy", |path, sqlite| {
        if sqlite {
            readers::workbuddy::parse_workbuddy_sqlite(path)
        } else {
            readers::workbuddy::parse_workbuddy_file(path)
        }
    };
    DevinCliReader => "devin-cli", |path, _sqlite| readers::devin::parse_devin_cli_sqlite(path);
    DevinDesktopReader => "devin-desktop", |path, _sqlite| readers::devin::parse_devin_desktop_ndjson(path);
    SenpiReader => "senpi", |path, _sqlite| readers::senpi::parse_senpi_file(path);
    AugmentReader => "augment", |path, _sqlite| readers::augment::parse_augment_file(path);
    KimchiReader => "kimchi", |path, _sqlite| readers::kimchi::parse_kimchi_file(path);
    ReasonixReader => "reasonix", |path, _sqlite| readers::reasonix::parse_reasonix_file(path);
    PrimeAgentReader => "prime-agent", |path, _sqlite| readers::prime_agent::parse_prime_agent_file(path);
    FreebuffReader => "freebuff", |path, _sqlite| readers::freebuff::parse_freebuff_file(path);
    CherrystudioReader => "cherrystudio", |path, _sqlite| readers::cherrystudio::parse_cherrystudio_file(path);
    DshReader => "dsh", |path, _sqlite| readers::dsh::parse_dsh_file(path);
    McodeReader => "mcode", |path, _sqlite| readers::mcode::parse_mcode_file(path);
    FxReader => "fx", |path, _sqlite| readers::fx::parse_fx_file(path);
    OmpReader => "omp", |path, _sqlite| readers::omp::parse_omp_file(path);
    LmstudioReader => "lmstudio", |path, _sqlite| readers::lmstudio::parse_lmstudio_file(path);
    UnslothReader => "unsloth", |path, _sqlite| readers::unsloth::parse_unsloth_sqlite(path);
    HindsightReader => "hindsight", |path, _sqlite| readers::hindsight::parse_hindsight_file(path);
}

pub fn reader(client: &str) -> Option<&'static dyn SessionReader> {
    READERS
        .iter()
        .copied()
        .find(|reader| reader.client() == client)
}

pub fn has_reader(client: &str) -> bool {
    super::catalog::clients().iter().any(|c| c.id == client) && reader(client).is_some()
}

pub fn read(client: &str, path: &Path) -> Result<Vec<ConsumptionRecord>, String> {
    if !path.is_file() {
        return Err("Usage input is not a regular file".into());
    }
    std::fs::File::open(path).map_err(|e| format!("Cannot read usage input: {e}"))?;
    let sqlite = path
        .extension()
        .is_some_and(|s| s == "db" || s == "sqlite" || s == "sqlite3");
    if sqlite {
        use std::io::Read;
        let mut header = [0u8; 16];
        let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        if file.read_exact(&mut header).is_err() || &header != b"SQLite format 3\0" {
            return Err("Unsupported usage format: expected an unencrypted SQLite database".into());
        }
    }
    let reader = reader(client).ok_or("Unknown usage client")?;
    let messages = reader.read(path, sqlite);
    normalize(client, path, messages)
}

pub(crate) fn normalize(
    client: &str,
    path: &Path,
    messages: Vec<UnifiedMessage>,
) -> Result<Vec<ConsumptionRecord>, String> {
    let definition = super::catalog::clients()
        .iter()
        .find(|c| c.id == client)
        .ok_or("Unknown usage client")?;
    let remote = definition.remote_collection();
    let aggregate = definition.aggregate_only();
    let mut ordinals = std::collections::HashMap::<String, u64>::new();
    let mut records = std::collections::BTreeMap::<String, ConsumptionRecord>::new();
    for message in messages {
        let raw = &message.tokens;
        let token_values = [
            raw.input,
            raw.output,
            raw.cache_read,
            raw.cache_write,
            raw.reasoning,
        ];
        if token_values.iter().any(|n| *n < 0) {
            return Err("Source reported negative tokens".into());
        }
        let tokens = Tokens {
            input: (raw.input as u64)
                .saturating_add(raw.cache_read as u64)
                .saturating_add(raw.cache_write as u64),
            output: (raw.output as u64).saturating_add(raw.reasoning as u64),
            cache_read: raw.cache_read as u64,
            cache_write: raw.cache_write as u64,
            reasoning: raw.reasoning as u64,
        };
        let mut cost = if message.cost_source == readers::CostSource::ProviderReported
            || message.cost_source == readers::CostSource::Unknown && message.cost > 0.0
        {
            Some(UsageCost {
                usd: message.cost,
                origin: if remote {
                    CostOrigin::ProviderReported
                } else {
                    CostOrigin::ClientReported
                },
                pricing_revision: None,
            })
        } else {
            None
        };
        if cost
            .as_ref()
            .is_some_and(|c| !c.usd.is_finite() || c.usd < 0.0)
        {
            cost = None;
        }
        let identity = if aggregate {
            format!(
                "{}:{}",
                message.session_id,
                if client == "droid" {
                    "session-total"
                } else {
                    &message.model_id
                }
            )
        } else if let Some(key) = message.dedup_key {
            // Prefix session even when a source's fallback key contains only
            // counts. Separate sessions cannot be identical requests.
            format!("{}:{key}", message.session_id)
        } else {
            let base = format!(
                "{}:{}:{}:{}",
                if message.session_id.is_empty() {
                    path.to_string_lossy().into_owned()
                } else {
                    String::new()
                },
                message.session_id,
                message.model_id,
                message.timestamp
            );
            let ordinal = ordinals.entry(base.clone()).or_default();
            *ordinal += 1;
            format!("{base}:{ordinal}")
        };
        let id = format!("{:x}", Sha256::digest(identity.as_bytes()));
        let model = (client != "droid"
            && !matches!(
                message.model_id.as_str(),
                "unknown" | "session-total" | "aggregate-requests" | "auto"
            ))
        .then_some(message.model_id);
        let mut record = ConsumptionRecord {
            id: id.clone(),
            client: client.into(),
            provider: (client != "droid"
                && !message.provider_id.is_empty()
                && message.provider_id != "unknown")
                .then_some(message.provider_id),
            account: None,
            session: (!message.session_id.is_empty()).then_some(message.session_id),
            project: message.workspace_key,
            model,
            service_tier: None,
            at_ms: message.timestamp,
            timestamp_inferred: false,
            period_end_ms: aggregate.then_some(message.timestamp),
            origin: if remote {
                UsageOrigin::RemoteReport
            } else {
                UsageOrigin::LocalLog
            },
            granularity: if client == "warp" {
                Granularity::AccountPeriod
            } else if aggregate {
                Granularity::Session
            } else {
                Granularity::Request
            },
            tokens: (!matches!(client, "crush" | "warp")).then_some(tokens),
            cost,
            request_id: None,
        };
        record.validate()?;
        if aggregate {
            if let Some(old) = records.get_mut(&id) {
                old.at_ms = old.at_ms.min(record.at_ms);
                old.period_end_ms = Some(old.period_end_ms.unwrap_or(old.at_ms).max(record.at_ms));
                if let (Some(a), Some(b)) = (&mut old.tokens, record.tokens.take()) {
                    a.input = a.input.saturating_add(b.input);
                    a.output = a.output.saturating_add(b.output);
                    a.cache_read = a.cache_read.saturating_add(b.cache_read);
                    a.cache_write = a.cache_write.saturating_add(b.cache_write);
                    a.reasoning = a.reasoning.saturating_add(b.reasoning);
                }
                if let (Some(a), Some(b)) = (&mut old.cost, record.cost) {
                    a.usd += b.usd;
                }
                old.validate()?;
                continue;
            }
        }
        records.insert(id, record);
    }
    Ok(records.into_values().collect())
}

#[cfg(test)]
mod reader_table_tests {
    use super::*;

    #[test]
    fn every_reader_is_a_catalog_client_and_listed_once() {
        let mut seen = std::collections::HashSet::new();
        for entry in READERS {
            assert!(
                seen.insert(entry.client()),
                "{} listed twice",
                entry.client()
            );
            assert!(
                has_reader(entry.client()),
                "{} missing from the catalog",
                entry.client()
            );
        }
        assert!(!has_reader("no-such-client"));
    }
}
