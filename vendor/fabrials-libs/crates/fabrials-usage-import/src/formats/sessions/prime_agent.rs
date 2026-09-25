// Adapted from Tokscale 0d621cae343af9b057fb4a38c84d089524ac4378.
// Copyright (c) 2025 Junho Yeo. MIT; see TOKSCALE-LICENSE.
//! Prime Agent session parser.
//!
//! Prime Agent stores root sessions in `~/.prime/agent/sessions/*.jsonl` and
//! RLM child sessions below the sibling `session-artifacts` tree. Both use the
//! Pi append-only JSONL record format, so token extraction is shared with the
//! Pi parser. `child_usage_attributed` records are never emitted as messages:
//! tokscale scans each child's own transcript directly. Their usage metadata is
//! used only to reverse aggregate parent usage that Prime may persist while
//! serializing a fork, before the copied parent is deduplicated across files.

use super::pi::{
    has_replacement_character, parse_pi_format_rlm_file_with_observer, PiFormatObserver,
    PiSessionEntry, PiSessionHeader,
};
use super::utils::parse_timestamp_str;
use super::UnifiedMessage;
use crate::formats::TokenBreakdown;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[cfg(test)]
#[derive(Default)]
struct PrimeDecodeCounter {
    root: Option<PathBuf>,
    messages: usize,
    accounting: usize,
}

#[cfg(test)]
static PRIME_DECODE_COUNTER: std::sync::LazyLock<std::sync::Mutex<PrimeDecodeCounter>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(PrimeDecodeCounter::default()));

#[cfg(test)]
fn record_transcript_decode(path: &Path, accounting: bool) {
    let mut counter = PRIME_DECODE_COUNTER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if counter
        .root
        .as_deref()
        .is_some_and(|root| path.starts_with(root))
    {
        if accounting {
            counter.accounting += 1;
        } else {
            counter.messages += 1;
        }
    }
}

pub fn parse_prime_agent_file(path: &Path) -> Vec<UnifiedMessage> {
    parse_prime_agent_file_with_accounting(path).0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PrimeAttribution {
    id: String,
    timestamp: Option<i64>,
    child_usage: TokenBreakdown,
    aggregate_usage: TokenBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChildMessageUsage {
    timestamp: Option<i64>,
    usage: TokenBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PrimeUsageAdjustment {
    dedup_key: String,
    persisted_usage: TokenBreakdown,
    attributions: Vec<PrimeAttribution>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct PrimeFileAccounting {
    source_path: PathBuf,
    attributions: Vec<PrimeAttribution>,
    adjustments: Vec<PrimeUsageAdjustment>,
    child_message_usages: Vec<ChildMessageUsage>,
    child_parent_path: Option<PathBuf>,
    fork_parent_path: Option<PathBuf>,
}

struct PrimeAccountingBuilder<'a> {
    path: &'a Path,
    found_header: bool,
    is_rlm_child: bool,
    child_parent_path: Option<PathBuf>,
    fork_parent_path: Option<PathBuf>,
    targets: HashMap<String, (String, TokenBreakdown)>,
    attributions: HashMap<String, Vec<PrimeAttribution>>,
    child_message_usages: Vec<ChildMessageUsage>,
}

impl<'a> PrimeAccountingBuilder<'a> {
    fn new(path: &'a Path) -> Self {
        Self {
            path,
            found_header: false,
            is_rlm_child: false,
            child_parent_path: None,
            fork_parent_path: None,
            targets: HashMap::new(),
            attributions: HashMap::new(),
            child_message_usages: Vec::new(),
        }
    }

    fn finish(self) -> PrimeFileAccounting {
        if !self.found_header {
            return PrimeFileAccounting::default();
        }

        let all_attributions = self
            .attributions
            .values()
            .flat_map(|entries| entries.iter().cloned())
            .collect();
        let mut adjustments = Vec::new();
        for (target_id, entries) in self.attributions {
            let Some((dedup_key, persisted_usage)) = self.targets.get(&target_id) else {
                continue;
            };
            let mut matching_prefix = None;
            for (index, entry) in entries.iter().enumerate() {
                if entry.aggregate_usage == *persisted_usage {
                    matching_prefix = Some(entries[..=index].to_vec());
                }
            }
            if let Some(prefix) = matching_prefix {
                adjustments.push(PrimeUsageAdjustment {
                    dedup_key: dedup_key.clone(),
                    persisted_usage: persisted_usage.clone(),
                    attributions: prefix,
                });
            }
        }

        PrimeFileAccounting {
            source_path: lineage_path(self.path),
            attributions: all_attributions,
            adjustments,
            child_message_usages: self.child_message_usages,
            child_parent_path: self.child_parent_path,
            fork_parent_path: self.fork_parent_path,
        }
    }
}

impl PiFormatObserver for PrimeAccountingBuilder<'_> {
    fn observe_header(&mut self, header: &PiSessionHeader) {
        self.found_header = true;
        self.is_rlm_child = header.rlm_depth.unwrap_or(0) > 0;
        let parent_path = header
            .parent_session
            .as_deref()
            .filter(|parent| !has_replacement_character(parent))
            .map(Path::new)
            .map(|parent| referenced_lineage_path(self.path, parent));
        if self.is_rlm_child {
            self.child_parent_path = parent_path;
        } else {
            self.fork_parent_path = parent_path;
        }
    }

    fn observe_entry(&mut self, entry: &PiSessionEntry, emitted: Option<&UnifiedMessage>) {
        let entry_timestamp = entry.timestamp.as_deref().and_then(parse_timestamp_str);
        if entry.entry_type == "child_usage_attributed" {
            if let (Some(id), Some(target_id), Some(child_usage), Some(aggregate_usage)) = (
                entry.id.as_ref(),
                entry.target_id.as_ref(),
                entry.child_usage.as_ref(),
                entry.aggregate_usage.as_ref(),
            ) {
                if has_replacement_character(id)
                    || has_replacement_character(target_id)
                    || child_usage.has_damaged_key()
                    || aggregate_usage.has_damaged_key()
                {
                    return;
                }
                self.attributions
                    .entry(target_id.clone())
                    .or_default()
                    .push(PrimeAttribution {
                        id: id.clone(),
                        timestamp: entry_timestamp,
                        child_usage: child_usage.to_breakdown(),
                        aggregate_usage: aggregate_usage.to_breakdown(),
                    });
            }
            return;
        }

        let Some(parsed) = emitted else {
            return;
        };
        if self.is_rlm_child {
            self.child_message_usages.push(ChildMessageUsage {
                timestamp: entry_timestamp,
                usage: parsed.tokens.clone(),
            });
        }
        if let (Some(id), Some(dedup_key)) = (entry.id.as_ref(), parsed.dedup_key.as_ref()) {
            if !has_replacement_character(id) {
                self.targets
                    .insert(id.clone(), (dedup_key.clone(), parsed.tokens.clone()));
            }
        }
    }
}

pub(crate) fn parse_prime_agent_file_with_accounting(
    path: &Path,
) -> (Vec<UnifiedMessage>, PrimeFileAccounting) {
    #[cfg(test)]
    record_transcript_decode(path, false);

    let mut accounting = PrimeAccountingBuilder::new(path);
    let messages =
        parse_pi_format_rlm_file_with_observer(path, "prime-agent", "prime-agent", &mut accounting);
    (messages, accounting.finish())
}

fn lineage_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn referenced_lineage_path(source_file: &Path, referenced: &Path) -> PathBuf {
    if referenced.is_absolute() {
        lineage_path(referenced)
    } else {
        lineage_path(
            &source_file
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(referenced),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn session_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn accepts_bom_crlf_and_later_records_after_invalid_utf8() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(
            b"\xef\xbb\xbf{\"type\":\"session\",\"version\":3,\"id\":\"root\",\"cwd\":\"/tmp/project\"}\r\n",
        )
        .unwrap();
        file.write_all(b"invalid \xff record\r\n").unwrap();
        file.write_all(
            br#"{"type":"message","id":"assistant","timestamp":"2026-08-08T00:00:01Z","message":{"role":"assistant","provider":"anthropic","model":"claude-opus-5","usage":{"input":100,"output":50,"cacheRead":20,"cacheWrite":10}}}"#,
        )
        .unwrap();
        file.flush().unwrap();

        let messages = parse_prime_agent_file(file.path());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].session_id, "root");
        assert_eq!(messages[0].tokens.total(), 180);
    }

    #[test]
    fn rejects_only_headers_with_damaged_lineage_keys() {
        for (prefix, suffix, rejected) in [
            // A replacement consumes at least one byte of a real lineage key.
            (b"parentSess".as_slice(), b"on".as_slice(), true),
            (b"rlmDep".as_slice(), b"h".as_slice(), true),
            (b"".as_slice(), b"arentSession".as_slice(), true),
            // These can only be damaged extension keys adjacent to, rather
            // than damaged spellings of, the complete structural key.
            // Raw-byte identity distinguishes invalid UTF-8 damage from a
            // clean literal replacement-bearing extension name.
            (b"parentSession".as_slice(), b"".as_slice(), true),
            (b"".as_slice(), b"parentSession".as_slice(), true),
            (b"parentSession".as_slice(), b"Extra".as_slice(), false),
            (b"xparentSess".as_slice(), b"on".as_slice(), false),
            (b"rlmDepth".as_slice(), b"".as_slice(), true),
            (br"rlmDepth".as_slice(), b"".as_slice(), true),
            (br"parentSession".as_slice(), b"".as_slice(), true),
            (b"extension".as_slice(), b"Field".as_slice(), false),
        ] {
            let mut file = NamedTempFile::new().unwrap();
            file.write_all(
                b"{\"type\":\"session\",\"version\":3,\"id\":\"child\",\"cwd\":\"/tmp/project\",\"",
            )
            .unwrap();
            file.write_all(prefix).unwrap();
            file.write_all(b"\xff").unwrap();
            file.write_all(suffix).unwrap();
            file.write_all(b"\":1}\n").unwrap();
            file.write_all(
                br#"{"type":"message","id":"assistant","message":{"role":"assistant","provider":"anthropic","model":"claude-opus-5","usage":{"input":10,"output":5}}}
"#,
            )
            .unwrap();
            file.flush().unwrap();

            let (messages, accounting) = parse_prime_agent_file_with_accounting(file.path());

            assert_eq!(messages.is_empty(), rejected);
            if rejected {
                assert!(accounting.attributions.is_empty());
                assert!(accounting.adjustments.is_empty());
                assert!(accounting.child_parent_path.is_none());
                assert!(accounting.fork_parent_path.is_none());
            }
        }

        let file = session_file(
            r#"{"type":"session","version":3,"id":"root","cwd":"/tmp/project","extensionField":1}
{"type":"message","id":"assistant","message":{"role":"assistant","provider":"anthropic","model":"claude-opus-5","usage":{"input":10,"output":5}}}"#,
        );
        assert_eq!(parse_prime_agent_file(file.path()).len(), 1);

        for (encoded_key, rejected) in [
            (r"rlmDep\uFFFDth", true),
            (r#"unrelated\uD800":1,"rlmDep\uFFFDth"#, true),
            (r"parentSess\uFFFDon", true),
            (r"extension\uFFFDField", false),
            (r#"unrelated\uD800":1,"extension\uFFFDField"#, false),
            (r"rlmDepth\uFFFD", false),
            (r"\uFFFDparentSession", false),
            (r"parentSession\uFFFD", false),
        ] {
            let file = session_file(&format!(
                "{{\"type\":\"session\",\"version\":3,\"id\":\"child\",\"cwd\":\"/tmp/project\",\"parentSession\":\"/tmp/parent.jsonl\",\"rlmDepth\":1,\"{encoded_key}\":1}}\n{{\"type\":\"message\",\"id\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"provider\":\"anthropic\",\"model\":\"claude-opus-5\",\"usage\":{{\"input\":10,\"output\":5}}}}}}"
            ));
            let (messages, accounting) = parse_prime_agent_file_with_accounting(file.path());
            assert_eq!(messages.is_empty(), rejected, "escaped key {encoded_key:?}");
            assert_eq!(
                accounting.child_parent_path.is_none(),
                rejected,
                "the shared parser and accounting analyzer must agree for {encoded_key:?}"
            );
        }
        for damaged_key in ["rlmDep�th", "parentSess�on"] {
            let file = session_file(&format!(
                "{{\"type\":\"session\",\"version\":3,\"id\":\"child\",\"cwd\":\"/tmp/project\",\"parentSession\":\"/tmp/parent.jsonl\",\"rlmDepth\":1,\"{damaged_key}\":1}}\n{{\"type\":\"message\",\"id\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"provider\":\"anthropic\",\"model\":\"claude-opus-5\",\"usage\":{{\"input\":10,\"output\":5}}}}}}"
            ));
            let (messages, accounting) = parse_prime_agent_file_with_accounting(file.path());
            assert!(messages.is_empty(), "literal damaged key {damaged_key:?}");
            assert!(
                accounting.child_parent_path.is_none(),
                "literal damaged key {damaged_key:?}"
            );
        }

        for extension_key in ["rlmDepth�", "�parentSession", "parentSession�"] {
            let file = session_file(&format!(
                "{{\"type\":\"session\",\"version\":3,\"id\":\"root\",\"cwd\":\"/tmp/project\",\"{extension_key}\":1}}\n{{\"type\":\"message\",\"id\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"provider\":\"anthropic\",\"model\":\"claude-opus-5\",\"usage\":{{\"input\":10,\"output\":5}}}}}}"
            ));
            assert_eq!(
                parse_prime_agent_file(file.path()).len(),
                1,
                "valid UTF-8 extension key {extension_key:?}"
            );
        }
    }

    #[test]
    fn rejects_rlm_child_without_usable_parent_session() {
        for parent_field in ["", ",\"parentSession\":\"\""] {
            let file = session_file(&format!(
                "{{\"type\":\"session\",\"id\":\"child\",\"rlmDepth\":1{parent_field}}}\n{{\"type\":\"message\",\"id\":\"assistant\",\"timestamp\":\"2026-08-08T00:00:01Z\",\"message\":{{\"role\":\"assistant\",\"provider\":\"anthropic\",\"model\":\"claude-opus-5\",\"usage\":{{\"input\":10}}}}}}"
            ));
            let (messages, accounting) = parse_prime_agent_file_with_accounting(file.path());
            assert!(messages.is_empty(), "parent field {parent_field:?}");
            assert!(accounting.child_message_usages.is_empty());
        }
    }

    #[test]
    fn sanitizes_replacement_mangled_model_and_provider() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(
            b"{\"type\":\"session\",\"version\":3,\"id\":\"root\",\"cwd\":\"/tmp/\xff/project\",\"rlmDepth\":0}\n",
        )
        .unwrap();
        file.write_all(b"{\"type\":\"session_info\",\"name\":\"agent-\xff\"}\n")
            .unwrap();
        file.write_all(
            b"{\"type\":\"message\",\"id\":\"assistant-clean\",\"timestamp\":\"2026-08-08T00:00:01Z\",\"message\":{\"role\":\"assistant\",\"provider\":\"bad-\xff-provider\",\"model\":\"bad-\xff-model\",\"usage\":{\"input\":20,\"output\":8}}}\n",
        )
        .unwrap();
        file.flush().unwrap();

        let messages = parse_prime_agent_file(file.path());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].model_id, "unknown");
        assert_eq!(messages[0].provider_id, "prime-agent");
        assert!(messages[0].workspace_key.is_none());
        assert!(messages[0].agent.is_none());
    }

    #[test]
    fn parses_root_session_without_counting_child_attribution_records() {
        let file = session_file(
            r#"{"type":"session","version":3,"id":"root-1","timestamp":"2026-08-08T00:00:00.000Z","cwd":"/tmp/project","rlmDepth":0}
{"type":"session_info","id":"info","parentId":null,"timestamp":"2026-08-08T00:00:00.500Z","name":"My renamed thread"}
{"type":"message","id":"assistant-1","parentId":"info","timestamp":"2026-08-08T00:00:01.000Z","message":{"role":"assistant","provider":"anthropic","model":"claude-opus-5","responseId":"msg_provider_001","usage":{"input":100,"output":50,"cacheRead":20,"cacheWrite":10,"totalTokens":180}}}
{"type":"child_usage_attributed","id":"usage-1","parentId":"assistant-1","timestamp":"2026-08-08T00:00:02.000Z","targetId":"assistant-1","childUsage":{"input":500,"output":200,"cacheRead":0,"cacheWrite":0,"totalTokens":700},"aggregateUsage":{"input":600,"output":250,"cacheRead":20,"cacheWrite":10,"totalTokens":880},"origin":"spawn_task"}"#,
        );

        let messages = parse_prime_agent_file(file.path());

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.client, "prime-agent");
        assert_eq!(message.session_id, "root-1");
        assert_eq!(message.workspace_key.as_deref(), Some("/tmp/project"));
        assert_eq!(message.tokens.input, 100);
        assert_eq!(message.tokens.output, 50);
        assert_eq!(message.tokens.cache_read, 20);
        assert_eq!(message.tokens.cache_write, 10);
        assert_eq!(message.agent, None, "a root thread name is not an agent");
        assert_eq!(
            message.dedup_key.as_deref(),
            Some("prime-agent:response:msg_provider_001")
        );
    }

    #[test]
    fn attributes_rlm_child_messages_to_the_session_name() {
        let file = session_file(
            r#"{"type":"session","version":3,"id":"child-1","timestamp":"2026-08-08T00:00:00.000Z","cwd":"/tmp/project","parentSession":"/tmp/root.jsonl","rlmDepth":1}
{"type":"session_info","id":"info","parentId":null,"timestamp":"2026-08-08T00:00:00.500Z","name":"api-reviewer"}
{"type":"message","id":"assistant-1","parentId":"info","timestamp":"2026-08-08T00:00:01.000Z","message":{"role":"assistant","provider":"openai","model":"gpt-5.4","usage":{"input":40,"output":12,"cacheRead":8,"cacheWrite":0,"totalTokens":60}}}"#,
        );

        let messages = parse_prime_agent_file(file.path());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].agent.as_deref(), Some("api-reviewer"));
        assert_eq!(messages[0].provider_id, "openai");
        assert_eq!(messages[0].model_id, "gpt-5.4");
    }

    #[test]
    fn copied_fork_history_keeps_a_cross_session_dedup_key() {
        let original = session_file(
            r#"{"type":"session","version":3,"id":"root-1","timestamp":"2026-08-08T00:00:00.000Z","cwd":"/tmp/project","rlmDepth":0}
{"type":"message","id":"assistant-1","parentId":null,"timestamp":"2026-08-08T00:00:01.000Z","message":{"role":"assistant","provider":"anthropic","model":"claude-opus-5","responseId":"msg_provider_001","usage":{"input":100,"output":50,"cacheRead":20,"cacheWrite":10,"totalTokens":180}}}"#,
        );
        let fork = session_file(
            r#"{"type":"session","version":3,"id":"fork-2","timestamp":"2026-08-08T01:00:00.000Z","cwd":"/tmp/project","parentSession":"/tmp/root.jsonl","rlmDepth":0}
{"type":"message","id":"assistant-1","parentId":null,"timestamp":"2026-08-08T00:00:01.000Z","message":{"role":"assistant","provider":"anthropic","model":"claude-opus-5","responseId":"msg_provider_001","usage":{"input":100,"output":50,"cacheRead":20,"cacheWrite":10,"totalTokens":180}}}"#,
        );

        let original = parse_prime_agent_file(original.path());
        let fork = parse_prime_agent_file(fork.path());

        assert_eq!(original.len(), 1);
        assert_eq!(fork.len(), 1);
        assert_eq!(original[0].dedup_key, fork[0].dedup_key);
    }

    #[test]
    fn copied_fork_history_without_response_or_event_timestamp_still_deduplicates() {
        let original = session_file(
            r#"{"type":"session","version":3,"id":"root-1","timestamp":"2026-08-08T00:00:00.000Z","cwd":"/tmp/project","rlmDepth":0}
{"type":"message","id":"assistant-1","parentId":null,"message":{"role":"assistant","provider":"anthropic","model":"claude-opus-5","usage":{"input":100,"output":50,"cacheRead":20,"cacheWrite":10,"totalTokens":180}}}"#,
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
        let fork = session_file(
            r#"{"type":"session","version":3,"id":"fork-2","timestamp":"2026-08-08T01:00:00.000Z","cwd":"/tmp/project","parentSession":"/tmp/root.jsonl","rlmDepth":0}
{"type":"message","id":"assistant-1","parentId":null,"message":{"role":"assistant","provider":"anthropic","model":"claude-opus-5","usage":{"input":100,"output":50,"cacheRead":20,"cacheWrite":10,"totalTokens":180}}}"#,
        );

        let original = parse_prime_agent_file(original.path());
        let fork = parse_prime_agent_file(fork.path());

        assert_ne!(original[0].timestamp, fork[0].timestamp);
        assert_eq!(original[0].dedup_key, fork[0].dedup_key);
    }

    #[test]
    fn rejects_the_rlm_subagent_catalog_as_a_session() {
        let file = session_file(
            r#"{"type":"rlm_subagent","childId":"sub-deadbeef","sessionName":"worker","sessionFile":"/tmp/child.jsonl"}"#,
        );

        assert!(parse_prime_agent_file(file.path()).is_empty());
    }
}
