//! File adapters own format I/O; only normalized records escape this boundary.
use super::formats::sessions::{self as readers, UnifiedMessage};
use fabrials_types::consumption::{
    ConsumptionRecord, CostOrigin, Granularity, Tokens, UsageCost, UsageOrigin,
};
use sha2::{Digest, Sha256};
use std::path::Path;

/// One local client's session reader. `parse` gets the path and whether it is
/// a SQLite database; the table below is the only place a client is wired in.
pub struct SessionReader {
    pub client: &'static str,
    parse: fn(&Path, bool) -> Vec<UnifiedMessage>,
}

pub const READERS: &[SessionReader] = &[
    SessionReader {
        client: "opencode",
        parse: |path, sqlite| {
            if sqlite {
                readers::opencode::parse_opencode_sqlite(path)
            } else {
                readers::opencode::parse_opencode_file(path)
                    .into_iter()
                    .collect()
            }
        },
    },
    SessionReader {
        client: "claude",
        parse: |path, _| readers::claudecode::parse_claude_file(path),
    },
    SessionReader {
        client: "codex",
        parse: |path, _| readers::codex::parse_codex_file(path),
    },
    SessionReader {
        client: "cursor",
        parse: |path, _| readers::cursor::parse_cursor_file(path),
    },
    SessionReader {
        client: "gemini",
        parse: |path, _| readers::gemini::parse_gemini_file(path),
    },
    SessionReader {
        client: "amp",
        parse: |path, _| readers::amp::parse_amp_file(path),
    },
    SessionReader {
        client: "droid",
        parse: |path, _| readers::droid::parse_droid_file(path),
    },
    SessionReader {
        client: "openclaw",
        parse: |path, sqlite| {
            if sqlite {
                readers::openclaw::parse_openclaw_sqlite(path)
            } else if path.file_name().is_some_and(|n| n == "sessions.json") {
                readers::openclaw::parse_openclaw_index(path)
            } else {
                readers::openclaw::parse_openclaw_transcript(path)
            }
        },
    },
    SessionReader {
        client: "pi",
        parse: |path, _| readers::pi::parse_pi_file(path),
    },
    SessionReader {
        client: "kimi",
        parse: |path, _| readers::kimi::parse_kimi_file(path),
    },
    SessionReader {
        client: "qwen",
        parse: |path, _| readers::qwen::parse_qwen_file(path),
    },
    SessionReader {
        client: "roocode",
        parse: |path, _| readers::roocode::parse_roocode_file(path),
    },
    SessionReader {
        client: "kilocode",
        parse: |path, _| readers::kilocode::parse_kilocode_file(path),
    },
    SessionReader {
        client: "mux",
        parse: |path, _| readers::mux::parse_mux_file(path),
    },
    SessionReader {
        client: "kilo",
        parse: |path, _| readers::kilo::parse_kilo_sqlite(path),
    },
    SessionReader {
        client: "crush",
        parse: |path, _| readers::crush::parse_crush_sqlite(path),
    },
    SessionReader {
        client: "hermes",
        parse: |path, _| readers::hermes::parse_hermes_sqlite(path),
    },
    SessionReader {
        client: "copilot",
        parse: |path, _| readers::copilot::parse_copilot_file(path),
    },
    SessionReader {
        client: "goose",
        parse: |path, _| readers::goose::parse_goose_sqlite(path),
    },
    SessionReader {
        client: "codebuff",
        parse: |path, _| readers::codebuff::parse_codebuff_file(path),
    },
    SessionReader {
        client: "antigravity",
        parse: |path, _| readers::antigravity::parse_antigravity_file(path),
    },
    SessionReader {
        client: "zed",
        parse: |path, _| readers::zed::parse_zed_sqlite(path),
    },
    SessionReader {
        client: "kiro",
        parse: |path, sqlite| {
            if sqlite {
                readers::kiro::parse_kiro_sqlite(path)
            } else {
                readers::kiro::parse_kiro_file(path)
            }
        },
    },
    SessionReader {
        client: "trae",
        parse: |path, _| readers::trae::parse_trae_file("trae", path),
    },
    SessionReader {
        client: "warp",
        parse: |path, _| readers::warp::parse_warp_file(path),
    },
    SessionReader {
        client: "cline",
        parse: |path, _| readers::cline::parse_cline_file(path),
    },
    SessionReader {
        client: "gjc",
        parse: |path, _| readers::gjc::parse_gjc_file(path),
    },
    SessionReader {
        client: "grok",
        parse: |path, _| readers::grok::parse_grok_file(path),
    },
    SessionReader {
        client: "jcode",
        parse: |path, _| readers::jcode::parse_jcode_file(path),
    },
    SessionReader {
        client: "commandcode",
        parse: |path, _| readers::commandcode::parse_commandcode_file(path),
    },
    SessionReader {
        client: "micode",
        parse: |path, _| readers::micode::parse_micode_sqlite(path),
    },
    SessionReader {
        client: "antigravity-cli",
        parse: |path, _| readers::antigravity_cli::parse_antigravity_cli_file(path),
    },
    SessionReader {
        client: "junie",
        parse: |path, _| readers::junie::parse_junie_file(path),
    },
    SessionReader {
        client: "zcode",
        parse: |path, sqlite| {
            if sqlite {
                readers::zcode::parse_zcode_sqlite(path)
            } else {
                readers::zcode::parse_zcode_file(path)
            }
        },
    },
    SessionReader {
        client: "opencodereview",
        parse: |path, _| readers::opencodereview::parse_opencodereview_file(path),
    },
    SessionReader {
        client: "codebuddy",
        parse: |path, _| readers::codebuddy::parse_codebuddy_file(path),
    },
    SessionReader {
        client: "workbuddy",
        parse: |path, sqlite| {
            if sqlite {
                readers::workbuddy::parse_workbuddy_sqlite(path)
            } else {
                readers::workbuddy::parse_workbuddy_file(path)
            }
        },
    },
    SessionReader {
        client: "devin-cli",
        parse: |path, _| readers::devin::parse_devin_cli_sqlite(path),
    },
    SessionReader {
        client: "devin-desktop",
        parse: |path, _| readers::devin::parse_devin_desktop_ndjson(path),
    },
    SessionReader {
        client: "senpi",
        parse: |path, _| readers::senpi::parse_senpi_file(path),
    },
    SessionReader {
        client: "augment",
        parse: |path, _| readers::augment::parse_augment_file(path),
    },
    SessionReader {
        client: "kimchi",
        parse: |path, _| readers::kimchi::parse_kimchi_file(path),
    },
    SessionReader {
        client: "reasonix",
        parse: |path, _| readers::reasonix::parse_reasonix_file(path),
    },
    SessionReader {
        client: "prime-agent",
        parse: |path, _| readers::prime_agent::parse_prime_agent_file(path),
    },
    SessionReader {
        client: "freebuff",
        parse: |path, _| readers::freebuff::parse_freebuff_file(path),
    },
    SessionReader {
        client: "cherrystudio",
        parse: |path, _| readers::cherrystudio::parse_cherrystudio_file(path),
    },
    SessionReader {
        client: "dsh",
        parse: |path, _| readers::dsh::parse_dsh_file(path),
    },
    SessionReader {
        client: "mcode",
        parse: |path, _| readers::mcode::parse_mcode_file(path),
    },
    SessionReader {
        client: "fx",
        parse: |path, _| readers::fx::parse_fx_file(path),
    },
    SessionReader {
        client: "omp",
        parse: |path, _| readers::omp::parse_omp_file(path),
    },
    SessionReader {
        client: "lmstudio",
        parse: |path, _| readers::lmstudio::parse_lmstudio_file(path),
    },
    SessionReader {
        client: "unsloth",
        parse: |path, _| readers::unsloth::parse_unsloth_sqlite(path),
    },
    SessionReader {
        client: "hindsight",
        parse: |path, _| readers::hindsight::parse_hindsight_file(path),
    },
];

pub fn reader(client: &str) -> Option<&'static SessionReader> {
    READERS.iter().find(|reader| reader.client == client)
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
    let messages = (reader.parse)(path, sqlite);
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
            assert!(seen.insert(entry.client), "{} listed twice", entry.client);
            assert!(
                has_reader(entry.client),
                "{} missing from the catalog",
                entry.client
            );
        }
        assert!(!has_reader("no-such-client"));
    }
}
