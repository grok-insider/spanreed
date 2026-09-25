//! File adapters own format I/O; only normalized records escape this boundary.
use super::formats::sessions::{self as readers, UnifiedMessage};
use fabrials_core::usage::{CostOrigin, Granularity, Tokens, UsageCost, UsageOrigin, UsageRecord};
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn has_reader(client: &str) -> bool {
    super::catalog::clients().iter().any(|c| c.id == client)
        && matches!(
            client,
            "opencode"
                | "claude"
                | "codex"
                | "cursor"
                | "gemini"
                | "amp"
                | "droid"
                | "openclaw"
                | "pi"
                | "kimi"
                | "qwen"
                | "roocode"
                | "kilocode"
                | "mux"
                | "kilo"
                | "crush"
                | "hermes"
                | "copilot"
                | "goose"
                | "codebuff"
                | "antigravity"
                | "zed"
                | "kiro"
                | "trae"
                | "warp"
                | "cline"
                | "gjc"
                | "grok"
                | "jcode"
                | "commandcode"
                | "micode"
                | "antigravity-cli"
                | "junie"
                | "zcode"
                | "opencodereview"
                | "codebuddy"
                | "workbuddy"
                | "devin-cli"
                | "devin-desktop"
                | "senpi"
                | "augment"
                | "kimchi"
                | "reasonix"
                | "prime-agent"
                | "freebuff"
                | "cherrystudio"
                | "dsh"
                | "mcode"
                | "fx"
                | "omp"
                | "lmstudio"
                | "unsloth"
                | "hindsight"
        )
}

pub fn read(client: &str, path: &Path) -> Result<Vec<UsageRecord>, String> {
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
    let messages = match client {
        "opencode" if sqlite => readers::opencode::parse_opencode_sqlite(path),
        "opencode" => readers::opencode::parse_opencode_file(path)
            .into_iter()
            .collect(),
        "claude" => readers::claudecode::parse_claude_file(path),
        "codex" => readers::codex::parse_codex_file(path),
        "cursor" => readers::cursor::parse_cursor_file(path),
        "gemini" => readers::gemini::parse_gemini_file(path),
        "amp" => readers::amp::parse_amp_file(path),
        "droid" => readers::droid::parse_droid_file(path),
        "openclaw" if sqlite => readers::openclaw::parse_openclaw_sqlite(path),
        "openclaw" if path.file_name().is_some_and(|n| n == "sessions.json") => {
            readers::openclaw::parse_openclaw_index(path)
        }
        "openclaw" => readers::openclaw::parse_openclaw_transcript(path),
        "pi" => readers::pi::parse_pi_file(path),
        "kimi" => readers::kimi::parse_kimi_file(path),
        "qwen" => readers::qwen::parse_qwen_file(path),
        "roocode" => readers::roocode::parse_roocode_file(path),
        "kilocode" => readers::kilocode::parse_kilocode_file(path),
        "mux" => readers::mux::parse_mux_file(path),
        "kilo" => readers::kilo::parse_kilo_sqlite(path),
        "crush" => readers::crush::parse_crush_sqlite(path),
        "hermes" => readers::hermes::parse_hermes_sqlite(path),
        "copilot" => readers::copilot::parse_copilot_file(path),
        "goose" => readers::goose::parse_goose_sqlite(path),
        "codebuff" => readers::codebuff::parse_codebuff_file(path),
        "antigravity" => readers::antigravity::parse_antigravity_file(path),
        "zed" => readers::zed::parse_zed_sqlite(path),
        "kiro" if sqlite => readers::kiro::parse_kiro_sqlite(path),
        "kiro" => readers::kiro::parse_kiro_file(path),
        "trae" => readers::trae::parse_trae_file(client, path),
        "warp" => readers::warp::parse_warp_file(path),
        "cline" => readers::cline::parse_cline_file(path),
        "gjc" => readers::gjc::parse_gjc_file(path),
        "grok" => readers::grok::parse_grok_file(path),
        "jcode" => readers::jcode::parse_jcode_file(path),
        "commandcode" => readers::commandcode::parse_commandcode_file(path),
        "micode" => readers::micode::parse_micode_sqlite(path),
        "antigravity-cli" => readers::antigravity_cli::parse_antigravity_cli_file(path),
        "junie" => readers::junie::parse_junie_file(path),
        "zcode" if sqlite => readers::zcode::parse_zcode_sqlite(path),
        "zcode" => readers::zcode::parse_zcode_file(path),
        "opencodereview" => readers::opencodereview::parse_opencodereview_file(path),
        "codebuddy" => readers::codebuddy::parse_codebuddy_file(path),
        "workbuddy" if sqlite => readers::workbuddy::parse_workbuddy_sqlite(path),
        "workbuddy" => readers::workbuddy::parse_workbuddy_file(path),
        "devin-cli" => readers::devin::parse_devin_cli_sqlite(path),
        "devin-desktop" => readers::devin::parse_devin_desktop_ndjson(path),
        "senpi" => readers::senpi::parse_senpi_file(path),
        "augment" => readers::augment::parse_augment_file(path),
        "kimchi" => readers::kimchi::parse_kimchi_file(path),
        "reasonix" => readers::reasonix::parse_reasonix_file(path),
        "prime-agent" => readers::prime_agent::parse_prime_agent_file(path),
        "freebuff" => readers::freebuff::parse_freebuff_file(path),
        "cherrystudio" => readers::cherrystudio::parse_cherrystudio_file(path),
        "dsh" => readers::dsh::parse_dsh_file(path),
        "mcode" => readers::mcode::parse_mcode_file(path),
        "fx" => readers::fx::parse_fx_file(path),
        "omp" => readers::omp::parse_omp_file(path),
        "lmstudio" => readers::lmstudio::parse_lmstudio_file(path),
        "unsloth" => readers::unsloth::parse_unsloth_sqlite(path),
        "hindsight" => readers::hindsight::parse_hindsight_file(path),
        _ => return Err("Unknown usage client".into()),
    };
    normalize(client, path, messages)
}

pub(crate) fn normalize(
    client: &str,
    path: &Path,
    messages: Vec<UnifiedMessage>,
) -> Result<Vec<UsageRecord>, String> {
    let definition = super::catalog::clients()
        .iter()
        .find(|c| c.id == client)
        .ok_or("Unknown usage client")?;
    let remote = definition.remote_collection();
    let aggregate = definition.aggregate_only();
    let mut ordinals = std::collections::HashMap::<String, u64>::new();
    let mut records = std::collections::BTreeMap::<String, UsageRecord>::new();
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
        let mut record = UsageRecord {
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
