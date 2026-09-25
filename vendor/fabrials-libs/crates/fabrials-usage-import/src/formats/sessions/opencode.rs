// Adapted from Tokscale 0d621cae343af9b057fb4a38c84d089524ac4378.
// Copyright (c) 2025 Junho Yeo. MIT; see TOKSCALE-LICENSE.
//! OpenCode session parser
//!
//! Parses messages from:
//! - SQLite database (OpenCode 1.2+): ~/.local/share/opencode/opencode.db
//! - Legacy JSON files: ~/.local/share/opencode/storage/message/
//!
//! The SQLite message schema — and the driver that reads it — is shared with
//! the other clients that adopted it; see [`super::opencode_schema`]. This
//! module keeps OpenCode's own legacy JSON file parser and its JSON-to-SQLite
//! migration cache.

// The message payload type and the SQLite driver are shared with every other
// client that adopted OpenCode's schema.
use super::opencode_schema::{
    parse_opencode_schema_sqlite, reported_cost, set_workspace_from_root, OpenCodeSchemaConfig,
    OpenCodeSchemaMessage as OpenCodeMessage,
};
use super::utils::read_file_or_none;
use super::{normalize_opencode_agent_name, UnifiedMessage};
use crate::formats::{provider_identity, TokenBreakdown};
#[cfg(test)]
use rusqlite::Connection;
use std::path::Path;

pub fn parse_opencode_sqlite(db_path: &Path) -> Vec<UnifiedMessage> {
    parse_opencode_schema_sqlite(db_path, OpenCodeSchemaConfig::opencode())
}

pub fn parse_opencode_file(path: &Path) -> Option<UnifiedMessage> {
    let data = read_file_or_none(path)?;
    let mut bytes = data;

    let msg: OpenCodeMessage = simd_json::from_slice(&mut bytes).ok()?;

    // OpenCode JSON files (v1) always carry an explicit role, so require it to
    // be "assistant" here. Missing-role acceptance (is_assistant) is reserved
    // for the v2 `session_message` SQLite path, whose SQL already filters
    // `type = 'assistant'`; applying it to files would count a role-less or
    // malformed file as assistant usage (previously it was skipped when the
    // required `role` field failed to deserialize).
    if msg.role.as_deref() != Some("assistant") {
        return None;
    }

    let workspace_root = msg
        .path
        .as_ref()
        .and_then(|path| path.root.as_deref())
        .map(str::to_string);
    // Resolve model + provider before moving any fields out of `msg`, since
    // both borrow the whole struct to fall back onto the nested `model` object.
    let model_id = msg.resolve_model_id()?;
    let provider_id = msg
        .resolve_provider_id()
        .unwrap_or_else(|| "unknown".to_string());
    let provider_id = provider_identity::canonical_provider(&provider_id).unwrap_or(provider_id);

    let tokens = msg.tokens?;
    // Legacy JSON files carry a complete `cache` object; a missing or partial
    // one has always dropped the message rather than counting it as zero.
    let cache = tokens.cache?;
    let (cache_read, cache_write) = (cache.read?, cache.write?);
    let time = msg.time?;
    let agent_or_mode = msg.mode.or(msg.agent);
    let agent = agent_or_mode.map(|a| normalize_opencode_agent_name(&a));

    let session_id = msg.session_id.unwrap_or_else(|| "unknown".to_string());

    // Embedded message ids are globally stable across the legacy JSON and
    // SQLite representations, so keep them unnamespaced for overlap and fork
    // dedup. A filename is only unique inside its session directory; make the
    // no-id fallback path-scoped so same-named files in separate sessions do
    // not silently collapse (#1198). Canonicalization also keeps one physical
    // file reached through two path spellings on one key. If a non-UTF-8 path
    // cannot be represented, leave the key absent: retaining both candidates
    // is safer than inventing a lossy identity that can undercount.
    let dedup_key = msg.id.or_else(|| legacy_json_path_dedup_key(path));
    let cost = reported_cost(msg.cost).unwrap_or(0.0);

    let mut unified = UnifiedMessage::new_with_agent(
        "opencode",
        model_id,
        provider_id,
        session_id,
        time.created as i64,
        TokenBreakdown {
            input: tokens.input.max(0),
            output: tokens.output.max(0),
            cache_read: cache_read.max(0),
            cache_write: cache_write.max(0),
            reasoning: tokens.reasoning.unwrap_or(0).max(0),
        },
        cost,
        agent,
    );
    unified.duration_ms = time.completed.and_then(|completed| {
        let duration = completed - time.created;
        (duration.is_finite() && duration > 0.0).then_some(duration as i64)
    });
    unified.dedup_key = dedup_key;
    set_workspace_from_root(&mut unified, workspace_root.as_deref());
    // OpenCode computes per-message cost at request time from its own pricing
    // data (models.dev), so a positive `cost` is authoritative and must survive
    // tokscale's LiteLLM repricing pass. A zero cost usually means OpenCode
    // itself had no pricing for the model — leave it `Unknown` so
    // `apply_pricing_if_available` can still estimate.
    if unified.cost > 0.0 {
        unified.mark_provider_reported_cost();
    }
    Some(unified)
}

fn legacy_json_path_dedup_key(path: &Path) -> Option<String> {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical
        .to_str()
        .map(|path| format!("legacy-json-path:{path}"))
}

// =============================================================================
// Migration cache: skip redundant legacy JSON scanning after full migration
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_opencode_sqlite_db(db_path: &Path) -> Connection {
        let conn = Connection::open(db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                data TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    /// Build a database shaped like OpenCode v2 (`opencode-next.db`): an empty
    /// `message` table plus the `session_message` + `session` tables that hold
    /// the real per-message data. Mirrors the columns tokscale actually reads.
    fn create_opencode_v2_sqlite_db(db_path: &Path) -> Connection {
        let conn = Connection::open(db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL,
                title TEXT
            );
            CREATE TABLE session_message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                type TEXT NOT NULL,
                data TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    /// Build a database shaped like current OpenCode v2: `session_v2` carries
    /// metadata while `session_message` holds the assistant usage payloads.
    fn create_opencode_session_v2_sqlite_db(db_path: &Path) -> Connection {
        let conn = Connection::open(db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session_v2 (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL,
                title TEXT
            );
            CREATE TABLE session_message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                type TEXT NOT NULL,
                data TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    /// A representative v2 assistant payload: no `role` field, model + provider
    /// nested under `$.model`, integer timestamps.
    const V2_ASSISTANT_DATA: &str = r#"{
        "time": { "created": 1783882279705, "completed": 1783882279943 },
        "agent": "build",
        "model": { "id": "claude-sonnet-4", "providerID": "anthropic", "variant": "default" },
        "content": [],
        "finish": "stop",
        "cost": 0.0123,
        "tokens": {
            "input": 5519,
            "output": 20,
            "reasoning": 23,
            "cache": { "read": 100, "write": 50 }
        }
    }"#;

    #[test]
    fn test_deserialize_v2_message_resolves_nested_model() {
        let mut bytes = V2_ASSISTANT_DATA.as_bytes().to_vec();
        let msg: OpenCodeMessage = simd_json::from_slice(&mut bytes).unwrap();

        assert_eq!(msg.role, None, "v2 payloads carry no role field");
        assert!(msg.is_assistant(), "missing role defaults to assistant");
        assert_eq!(msg.resolve_model_id().as_deref(), Some("claude-sonnet-4"));
        assert_eq!(msg.resolve_provider_id().as_deref(), Some("anthropic"));
        assert_eq!(msg.agent.as_deref(), Some("build"));
    }

    #[test]
    fn test_top_level_model_id_takes_precedence_over_nested() {
        let json = r#"{
            "role": "assistant",
            "modelID": "top-level-model",
            "providerID": "top-level-provider",
            "model": { "id": "nested-model", "providerID": "nested-provider" },
            "tokens": { "input": 1, "output": 1, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            "time": { "created": 1700000000000.0 }
        }"#;
        let mut bytes = json.as_bytes().to_vec();
        let msg: OpenCodeMessage = simd_json::from_slice(&mut bytes).unwrap();

        assert_eq!(msg.resolve_model_id().as_deref(), Some("top-level-model"));
        assert_eq!(
            msg.resolve_provider_id().as_deref(),
            Some("top-level-provider")
        );
    }

    #[test]
    fn test_parse_v2_session_message_reads_tokens_and_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("opencode-next.db");

        let conn = create_opencode_v2_sqlite_db(&db_path);
        conn.execute(
            "INSERT INTO session (id, directory) VALUES (?1, ?2)",
            rusqlite::params!["ses_v2", "/Users/alice/opencode-v2-repo"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["msg_v2_001", "ses_v2", "assistant", V2_ASSISTANT_DATA],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(messages.len(), 1, "v2 assistant row should be parsed");
        let msg = &messages[0];
        assert_eq!(msg.model_id, "claude-sonnet-4");
        assert_eq!(msg.provider_id, "anthropic");
        assert_eq!(msg.tokens.input, 5519);
        assert_eq!(msg.tokens.output, 20);
        assert_eq!(msg.tokens.reasoning, 23);
        assert_eq!(msg.tokens.cache_read, 100);
        assert_eq!(msg.tokens.cache_write, 50);
        assert_eq!(msg.duration_ms, Some(238));
        assert_eq!(
            msg.workspace_key.as_deref(),
            Some("/Users/alice/opencode-v2-repo"),
            "workspace should come from session.directory"
        );
        assert_eq!(msg.workspace_label.as_deref(), Some("opencode-v2-repo"));
        assert_eq!(
            msg.dedup_key.as_deref(),
            Some("msg_v2_001"),
            "v2 dedup_key falls back to the session_message row id"
        );
        assert_eq!(
            msg.cost_source,
            crate::formats::sessions::CostSource::ProviderReported
        );
    }
    #[test]
    fn test_parse_v2_session_v2_message_reads_tokens_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("opencode.db");
        let conn = create_opencode_session_v2_sqlite_db(&db_path);
        conn.execute(
            "INSERT INTO session_v2 (id, directory, title) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                "ses_current_v2",
                "/Users/alice/current-opencode-repo",
                "Current OpenCode session"
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "msg_current_v2",
                "ses_current_v2",
                "assistant",
                V2_ASSISTANT_DATA
            ],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(messages.len(), 1, "current session_v2 rows should parse");
        let msg = &messages[0];
        assert_eq!(
            msg.workspace_key.as_deref(),
            Some("/Users/alice/current-opencode-repo")
        );
        assert_eq!(
            msg.workspace_label.as_deref(),
            Some("current-opencode-repo")
        );
        assert_eq!(
            msg.session_title.as_deref(),
            Some("Current OpenCode session")
        );
        assert_eq!(msg.dedup_key.as_deref(), Some("msg_current_v2"));
    }

    #[test]
    fn test_parse_v2_session_message_without_metadata_table() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session_message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                type TEXT NOT NULL,
                data TEXT NOT NULL
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "msg_without_metadata",
                "ses_without_metadata",
                "assistant",
                V2_ASSISTANT_DATA
            ],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(
            messages.len(),
            1,
            "usage should parse without session metadata"
        );
        assert_eq!(messages[0].workspace_key, None);
        assert_eq!(messages[0].session_title, None);
        assert_eq!(
            messages[0].dedup_key.as_deref(),
            Some("msg_without_metadata")
        );
    }

    #[test]
    fn test_parse_v2_skips_non_assistant_and_tokenless_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("opencode-next.db");

        let conn = create_opencode_v2_sqlite_db(&db_path);
        let user_data = r#"{ "time": { "created": 1783882279705 }, "content": [] }"#;
        let tokenless = r#"{ "time": { "created": 1783882279705 }, "model": { "id": "m", "providerID": "p" } }"#;
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["msg_ok", "ses_v2", "assistant", V2_ASSISTANT_DATA],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["msg_user", "ses_v2", "user", user_data],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["msg_synthetic", "ses_v2", "synthetic", V2_ASSISTANT_DATA],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["msg_no_tokens", "ses_v2", "assistant", tokenless],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(
            messages.len(),
            1,
            "only the assistant row with tokens should parse"
        );
        assert_eq!(messages[0].dedup_key.as_deref(), Some("msg_ok"));
    }

    #[test]
    fn test_parse_v2_negative_tokens_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("opencode-next.db");

        let conn = create_opencode_v2_sqlite_db(&db_path);
        let negative = r#"{
            "time": { "created": 1783882279705 },
            "model": { "id": "claude-sonnet-4", "providerID": "anthropic" },
            "cost": -1.0,
            "tokens": { "input": -100, "output": -50, "reasoning": -25, "cache": { "read": -200, "write": -10 } }
        }"#;
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["msg_neg", "ses_v2", "assistant", negative],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(messages.len(), 1);
        let msg = &messages[0];
        assert_eq!(msg.tokens.input, 0);
        assert_eq!(msg.tokens.output, 0);
        assert_eq!(msg.tokens.reasoning, 0);
        assert_eq!(msg.tokens.cache_read, 0);
        assert_eq!(msg.tokens.cache_write, 0);
        assert!(msg.cost >= 0.0);
    }

    #[test]
    fn test_parse_v2_deduplicates_forked_session_message_history() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("opencode-next.db");

        let conn = create_opencode_v2_sqlite_db(&db_path);
        // Same payload copied into a forked session must collapse to one entry.
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["root_row", "root_session", "assistant", V2_ASSISTANT_DATA],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["fork_row", "fork_session", "assistant", V2_ASSISTANT_DATA],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(
            messages.len(),
            1,
            "forked copies of the same assistant turn collapse inside v2 parsing"
        );
    }

    #[test]
    fn test_distinct_embedded_ids_are_not_merged_despite_fingerprint_collision() {
        // Two genuinely different assistant messages can share every fingerprint
        // field (timestamp, model, tokens, cost, agent). When both carry an
        // embedded `$.id` and the ids DIFFER, they are distinct messages -- not
        // fork copies -- and must be kept separate rather than collapsed.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("opencode-next.db");
        let conn = create_opencode_v2_sqlite_db(&db_path);

        let payload = |id: &str| {
            format!(
                r#"{{
                    "id": "{id}",
                    "time": {{ "created": 1783882279705, "completed": 1783882279943 }},
                    "agent": "build",
                    "model": {{ "id": "claude-sonnet-4", "providerID": "anthropic" }},
                    "cost": 0.0123,
                    "tokens": {{ "input": 10, "output": 5, "reasoning": 0, "cache": {{ "read": 0, "write": 0 }} }}
                }}"#
            )
        };

        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["row_a", "ses_v2", "assistant", payload("msg_a")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["row_b", "ses_v2", "assistant", payload("msg_b")],
        )
        .unwrap();
        // A true fork of msg_a (same embedded id, different session/row) must
        // still collapse into msg_a rather than becoming a third entry.
        conn.execute(
            "INSERT INTO session_message (id, session_id, type, data) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["row_a_fork", "fork_session", "assistant", payload("msg_a")],
        )
        .unwrap();
        drop(conn);

        let mut dedup_keys: Vec<String> = parse_opencode_sqlite(&db_path)
            .into_iter()
            .filter_map(|m| m.dedup_key)
            .collect();
        dedup_keys.sort();
        assert_eq!(
            dedup_keys,
            vec!["msg_a".to_string(), "msg_b".to_string()],
            "distinct embedded ids stay separate; a same-id fork collapses"
        );
    }

    #[test]
    fn test_parse_opencode_structure() {
        let json = r#"{
            "id": "msg_123",
            "sessionID": "ses_456",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.05,
            "tokens": {
                "input": 1000,
                "output": 500,
                "reasoning": 100,
                "cache": { "read": 200, "write": 50 }
            },
            "time": { "created": 1700000000000.0 }
        }"#;

        let mut bytes = json.as_bytes().to_vec();
        let msg: OpenCodeMessage = simd_json::from_slice(&mut bytes).unwrap();

        assert_eq!(msg.model_id, Some("claude-sonnet-4".to_string()));
        assert_eq!(msg.tokens.unwrap().input, 1000);
        assert_eq!(msg.agent, None);
    }

    #[test]
    fn test_parse_opencode_with_agent() {
        let json = r#"{
            "id": "msg_123",
            "sessionID": "ses_456",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "agent": "OmO",
            "cost": 0.05,
            "tokens": {
                "input": 1000,
                "output": 500,
                "reasoning": 100,
                "cache": { "read": 200, "write": 50 }
            },
            "time": { "created": 1700000000000.0 }
        }"#;

        let mut bytes = json.as_bytes().to_vec();
        let msg: OpenCodeMessage = simd_json::from_slice(&mut bytes).unwrap();

        assert_eq!(msg.agent, Some("OmO".to_string()));
    }

    /// Verify negative token values are clamped to 0 (defense-in-depth for PR #147)
    #[test]
    fn test_negative_values_clamped_to_zero() {
        use std::io::Write;

        let json = r#"{
            "id": "msg_negative",
            "sessionID": "ses_negative",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": -0.05,
            "tokens": {
                "input": -100,
                "output": -50,
                "reasoning": -25,
                "cache": { "read": -200, "write": -10 }
            },
            "time": { "created": 1700000000000.0 }
        }"#;

        let mut temp_file = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        temp_file.write_all(json.as_bytes()).unwrap();

        let result = parse_opencode_file(temp_file.path());
        assert!(result.is_some(), "Should parse file with negative values");

        let msg = result.unwrap();
        assert_eq!(msg.tokens.input, 0, "Negative input should be clamped to 0");
        assert_eq!(
            msg.tokens.output, 0,
            "Negative output should be clamped to 0"
        );
        assert_eq!(
            msg.tokens.cache_read, 0,
            "Negative cache_read should be clamped to 0"
        );
        assert_eq!(
            msg.tokens.cache_write, 0,
            "Negative cache_write should be clamped to 0"
        );
        assert_eq!(
            msg.tokens.reasoning, 0,
            "Negative reasoning should be clamped to 0"
        );
        assert!(
            msg.cost >= 0.0,
            "Negative cost should be clamped to 0.0, got {}",
            msg.cost
        );
    }

    #[test]
    fn test_parse_opencode_file_requires_explicit_assistant_role() {
        use std::io::Write;
        // Regression: making `role` optional for the v2 SQLite path must NOT
        // loosen file parsing. A file without a `role` (or a non-assistant one)
        // is not assistant usage and must be skipped -- the missing-role =>
        // assistant shortcut applies only to the type-filtered session_message
        // SQLite query, never to JSON files.
        let role_less = r#"{
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "tokens": { "input": 10, "output": 5, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            "time": { "created": 1700000000000.0 }
        }"#;
        let mut f1 = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        f1.write_all(role_less.as_bytes()).unwrap();
        assert!(
            parse_opencode_file(f1.path()).is_none(),
            "a role-less OpenCode JSON file must not be counted as assistant usage"
        );

        let user_role = r#"{
            "role": "user",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "tokens": { "input": 10, "output": 5, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            "time": { "created": 1700000000000.0 }
        }"#;
        let mut f2 = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        f2.write_all(user_role.as_bytes()).unwrap();
        assert!(
            parse_opencode_file(f2.path()).is_none(),
            "a non-assistant OpenCode JSON file must be skipped"
        );
    }

    /// JSON dedup_key uses msg.id when present
    #[test]
    fn test_dedup_key_from_json_message_id() {
        use std::io::Write;

        let json = r#"{
            "id": "msg_dedup_001",
            "sessionID": "ses_001",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.01,
            "tokens": {
                "input": 100,
                "output": 50,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "time": { "created": 1700000000000.0 }
        }"#;

        let mut temp_file = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        temp_file.write_all(json.as_bytes()).unwrap();

        let msg = parse_opencode_file(temp_file.path()).expect("Should parse");
        assert_eq!(
            msg.dedup_key,
            Some("msg_dedup_001".to_string()),
            "dedup_key should use msg.id from JSON"
        );
    }

    #[test]
    fn test_parse_opencode_file_sets_duration_from_completed_time() {
        use std::io::Write;

        let json = r#"{
            "id": "msg_timed",
            "sessionID": "ses_001",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.01,
            "tokens": {
                "input": 100,
                "output": 50,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "time": { "created": 1700000000000.0, "completed": 1700000001234.0 }
        }"#;

        let mut temp_file = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        temp_file.write_all(json.as_bytes()).unwrap();

        let msg = parse_opencode_file(temp_file.path()).expect("Should parse");
        assert_eq!(msg.duration_ms, Some(1234));
    }

    /// JSON dedup_key falls back to a path-scoped identity when msg.id is absent.
    #[test]
    fn test_dedup_key_falls_back_to_canonical_file_path() {
        let json = r#"{
            "sessionID": "ses_001",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.01,
            "tokens": {
                "input": 100,
                "output": 50,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "time": { "created": 1700000000000.0 }
        }"#;

        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("msg_fallback_999.json");
        std::fs::write(&file_path, json).unwrap();

        let msg = parse_opencode_file(&file_path).expect("Should parse");
        assert_eq!(
            msg.dedup_key,
            legacy_json_path_dedup_key(&file_path),
            "an id-less message must use the file's canonical location"
        );
        assert!(msg
            .dedup_key
            .as_deref()
            .is_some_and(|key| key.starts_with("legacy-json-path:")));
    }

    #[test]
    fn same_named_idless_files_in_different_sessions_have_distinct_keys() {
        let json = r#"{
            "sessionID": "embedded-session-is-not-the-fallback",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "tokens": {
                "input": 100,
                "output": 50,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "time": { "created": 1700000000000.0 }
        }"#;
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("session-a/same-name.json");
        let second = root.path().join("session-b/same-name.json");
        std::fs::create_dir_all(first.parent().unwrap()).unwrap();
        std::fs::create_dir_all(second.parent().unwrap()).unwrap();
        std::fs::write(&first, json).unwrap();
        std::fs::write(&second, json).unwrap();

        let first = parse_opencode_file(&first).unwrap();
        let second = parse_opencode_file(&second).unwrap();

        assert_ne!(first.dedup_key, second.dedup_key);
        assert!(first.dedup_key.is_some());
        assert!(second.dedup_key.is_some());
    }

    /// Non-assistant messages are skipped (no dedup_key produced)
    #[test]
    fn test_dedup_key_skips_non_assistant() {
        let json = r#"{
            "id": "msg_user_001",
            "sessionID": "ses_001",
            "role": "user",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "tokens": {
                "input": 100,
                "output": 50,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "time": { "created": 1700000000000.0 }
        }"#;

        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("msg_user_001.json");
        std::fs::write(&file_path, json).unwrap();

        let result = parse_opencode_file(&file_path);
        assert!(result.is_none(), "User messages should be skipped");
    }

    /// SQLite dedup_key falls back to the database row id when the message has no embedded id.
    #[test]
    fn test_sqlite_dedup_key_falls_back_to_row_id() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_opencode.db");

        let conn = create_opencode_sqlite_db(&db_path);

        let data_json = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.05,
            "tokens": {
                "input": 1000,
                "output": 500,
                "reasoning": 0,
                "cache": { "read": 200, "write": 50 }
            },
            "time": { "created": 1700000000000.0 }
        }"#;

        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["msg_sqlite_001", "ses_001", data_json],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].dedup_key,
            Some("msg_sqlite_001".to_string()),
            "SQLite dedup_key should fall back to the row id when no embedded id exists"
        );
        assert_eq!(messages[0].model_id, "claude-sonnet-4");
        assert_eq!(messages[0].tokens.input, 1000);
    }

    #[test]
    fn test_parse_opencode_file_marks_positive_cost_as_provider_reported() {
        use std::io::Write;
        let json = r#"{
            "id": "msg_cost_001",
            "sessionID": "ses_cost",
            "role": "assistant",
            "modelID": "z-ai/glm-4.6",
            "providerID": "openrouter",
            "cost": 0.0025158,
            "tokens": {
                "input": 2675,
                "output": 28,
                "reasoning": 1,
                "cache": { "read": 7700, "write": 0 }
            },
            "time": { "created": 1765915142201.0 }
        }"#;

        let mut temp_file = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        temp_file.write_all(json.as_bytes()).unwrap();

        let msg = parse_opencode_file(temp_file.path()).expect("Should parse");
        assert_eq!(
            msg.cost_source,
            crate::formats::sessions::CostSource::ProviderReported,
            "positive embedded cost must survive the LiteLLM repricing pass"
        );
        assert!((msg.cost - 0.0025158).abs() < 1e-12);
    }

    #[test]
    fn test_parse_opencode_file_keeps_zero_cost_unknown_for_estimation() {
        use std::io::Write;
        let json = r#"{
            "id": "msg_cost_002",
            "sessionID": "ses_cost",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.0,
            "tokens": {
                "input": 1000,
                "output": 500,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "time": { "created": 1700000000000.0 }
        }"#;

        let mut temp_file = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        temp_file.write_all(json.as_bytes()).unwrap();

        let msg = parse_opencode_file(temp_file.path()).expect("Should parse");
        assert_eq!(
            msg.cost_source,
            crate::formats::sessions::CostSource::Unknown,
            "zero cost means OpenCode had no pricing — leave repricing enabled"
        );
    }

    #[test]
    fn test_parse_opencode_sqlite_marks_positive_cost_as_provider_reported() {
        let dir = tempfile::TempDir::new().unwrap();
        let db_path = dir.path().join("test_opencode_cost.db");
        let conn = create_opencode_sqlite_db(&db_path);

        let costed = r#"{
            "role": "assistant",
            "modelID": "z-ai/glm-4.6",
            "providerID": "openrouter",
            "cost": 0.0025158,
            "tokens": {
                "input": 2675,
                "output": 28,
                "reasoning": 1,
                "cache": { "read": 7700, "write": 0 }
            },
            "time": { "created": 1765915142201.0 }
        }"#;
        let free = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.0,
            "tokens": {
                "input": 1000,
                "output": 500,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "time": { "created": 1700000000000.0 }
        }"#;

        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["msg_costed", "ses_cost", costed],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["msg_free", "ses_cost", free],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(messages.len(), 2);

        let costed_msg = messages
            .iter()
            .find(|m| m.dedup_key.as_deref() == Some("msg_costed"))
            .unwrap();
        assert_eq!(
            costed_msg.cost_source,
            crate::formats::sessions::CostSource::ProviderReported
        );

        let free_msg = messages
            .iter()
            .find(|m| m.dedup_key.as_deref() == Some("msg_free"))
            .unwrap();
        assert_eq!(
            free_msg.cost_source,
            crate::formats::sessions::CostSource::Unknown
        );
    }

    #[test]
    fn test_parse_opencode_file_uses_explicit_path_root_as_workspace() {
        let json = r#"{
            "id": "msg_workspace_001",
            "sessionID": "ses_001",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.01,
            "tokens": {
                "input": 100,
                "output": 50,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "time": { "created": 1700000000000.0 },
            "path": { "root": "/Users/alice/opencode-json-repo" }
        }"#;

        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("msg_workspace_001.json");
        std::fs::write(&file_path, json).unwrap();

        let msg = parse_opencode_file(&file_path).expect("Should parse");
        assert_eq!(
            msg.workspace_key.as_deref(),
            Some("/Users/alice/opencode-json-repo")
        );
        assert_eq!(msg.workspace_label.as_deref(), Some("opencode-json-repo"));
    }

    #[test]
    fn test_parse_opencode_file_ignores_non_object_path_without_rejecting_message() {
        let json = r#"{
            "id": "msg_path_string_001",
            "sessionID": "ses_001",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.01,
            "tokens": {
                "input": 100,
                "output": 50,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "time": { "created": 1700000000000.0 },
            "path": "/Users/alice/not-object"
        }"#;

        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("msg_path_string_001.json");
        std::fs::write(&file_path, json).unwrap();

        let msg = parse_opencode_file(&file_path).expect("Should parse");
        assert_eq!(msg.workspace_key, None);
        assert_eq!(msg.workspace_label, None);
    }

    #[test]
    fn test_parse_opencode_sqlite_uses_session_directory_as_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_opencode.db");

        let conn = create_opencode_sqlite_db(&db_path);
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL,
                title TEXT
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session (id, directory) VALUES (?1, ?2)",
            rusqlite::params!["ses_001", "/Users/alice/opencode-sqlite-repo"],
        )
        .unwrap();

        let data_json = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.05,
            "tokens": {
                "input": 1000,
                "output": 500,
                "reasoning": 0,
                "cache": { "read": 200, "write": 50 }
            },
            "time": { "created": 1700000000000.0 }
        }"#;

        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["msg_sqlite_workspace", "ses_001", data_json],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].workspace_key.as_deref(),
            Some("/Users/alice/opencode-sqlite-repo")
        );
        assert_eq!(
            messages[0].workspace_label.as_deref(),
            Some("opencode-sqlite-repo")
        );
    }

    #[test]
    fn test_parse_opencode_sqlite_legacy_fallback_uses_path_root_when_session_table_missing() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_opencode.db");

        let conn = create_opencode_sqlite_db(&db_path);

        let data_json = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.05,
            "tokens": {
                "input": 1000,
                "output": 500,
                "reasoning": 0,
                "cache": { "read": 200, "write": 50 }
            },
            "time": { "created": 1700000000000.0 },
            "path": { "root": "/Users/alice/legacy-fallback-repo" }
        }"#;

        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["msg_sqlite_legacy_workspace", "ses_001", data_json],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].workspace_key.as_deref(),
            Some("/Users/alice/legacy-fallback-repo")
        );
        assert_eq!(
            messages[0].workspace_label.as_deref(),
            Some("legacy-fallback-repo")
        );
        assert_eq!(messages[0].tokens.input, 1000);
    }

    #[test]
    fn test_parse_opencode_sqlite_duplicate_workspace_conflict_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_opencode.db");

        let conn = create_opencode_sqlite_db(&db_path);
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                directory TEXT NOT NULL,
                title TEXT
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session (id, directory) VALUES (?1, ?2)",
            rusqlite::params!["ses_root", "/Users/alice/root-workspace"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session (id, directory) VALUES (?1, ?2)",
            rusqlite::params!["ses_fork", "/Users/alice/fork-workspace"],
        )
        .unwrap();

        let data_json = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.05,
            "tokens": {
                "input": 1000,
                "output": 500,
                "reasoning": 0,
                "cache": { "read": 200, "write": 50 }
            },
            "time": { "created": 1700000000000.0, "completed": 1700000000500.0 },
            "mode": "build"
        }"#;

        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["z_root_copy", "ses_root", data_json],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["a_fork_copy", "ses_fork", data_json],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].workspace_key, None);
        assert_eq!(messages[0].workspace_label, None);
        assert_eq!(messages[0].tokens.input, 1000);
    }

    /// SQLite prefers the embedded message id when present so JSON/SQLite overlap keeps deduplicating.
    #[test]
    fn test_sqlite_dedup_key_prefers_embedded_message_id() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_opencode.db");

        let conn = create_opencode_sqlite_db(&db_path);

        let valid = r#"{
            "id": "embedded_msg_001",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "tokens": { "input": 100, "output": 50, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            "time": { "created": 1700000000000.0 }
        }"#;

        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["row_msg_001", "ses_001", valid],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].dedup_key,
            Some("embedded_msg_001".to_string()),
            "SQLite dedup_key should prefer the embedded message id for cross-source overlap"
        );
    }

    /// SQLite skips rows without tokens or with non-assistant role
    #[test]
    fn test_sqlite_skips_invalid_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_opencode.db");

        let conn = create_opencode_sqlite_db(&db_path);

        let valid = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "tokens": { "input": 100, "output": 50, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            "time": { "created": 1700000000000.0 }
        }"#;

        let user_msg = r#"{
            "role": "user",
            "modelID": "claude-sonnet-4",
            "time": { "created": 1700000000000.0 }
        }"#;

        let no_tokens = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "time": { "created": 1700000000000.0 }
        }"#;

        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["msg_valid", "ses_001", valid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["msg_user", "ses_001", user_msg],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["msg_no_tokens", "ses_001", no_tokens],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(
            messages.len(),
            1,
            "Should only parse valid assistant message"
        );
        assert_eq!(messages[0].dedup_key, Some("msg_valid".to_string()));
    }

    /// Forked SQLite sessions should not count copied history more than once.
    #[test]
    fn test_sqlite_deduplicates_forked_history_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_opencode.db");
        let conn = create_opencode_sqlite_db(&db_path);

        let root_msg = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.05,
            "tokens": {
                "input": 1000,
                "output": 500,
                "reasoning": 25,
                "cache": { "read": 200, "write": 50 }
            },
            "time": { "created": 1700000000000.0, "completed": 1700000000500.0 },
            "mode": "build"
        }"#;

        let new_msg = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.08,
            "tokens": {
                "input": 1300,
                "output": 650,
                "reasoning": 40,
                "cache": { "read": 100, "write": 0 }
            },
            "time": { "created": 1700000001000.0, "completed": 1700000001500.0 },
            "mode": "build"
        }"#;

        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["root_row", "root_session", root_msg],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["fork_copy_row", "fork_session", root_msg],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["fork_new_row", "fork_session", new_msg],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(
            messages.len(),
            2,
            "Forked copies of the same assistant history should collapse inside SQLite parsing"
        );
        assert_eq!(messages[0].tokens.input, 1000);
        assert_eq!(messages[1].tokens.input, 1300);
    }

    /// Same-timestamp messages with different payloads should remain distinct.
    #[test]
    fn test_sqlite_same_timestamp_distinct_payloads_survive() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_opencode.db");
        let conn = create_opencode_sqlite_db(&db_path);

        let first = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.05,
            "tokens": {
                "input": 1000,
                "output": 500,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "time": { "created": 1700000000000.0, "completed": 1700000000100.0 },
            "mode": "build"
        }"#;

        let second = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "cost": 0.05,
            "tokens": {
                "input": 1500,
                "output": 750,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "time": { "created": 1700000000000.0, "completed": 1700000000100.0 },
            "mode": "build"
        }"#;

        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["row_one", "session_one", first],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["row_two", "session_two", second],
        )
        .unwrap();
        drop(conn);

        let messages = parse_opencode_sqlite(&db_path);
        assert_eq!(
            messages.len(),
            2,
            "Distinct assistant calls should survive even when they share the same creation timestamp"
        );
    }

    /// Cross-source dedup: matching IDs between SQLite and JSON should deduplicate
    #[test]
    fn test_cross_source_dedup_by_message_id() {
        use std::collections::HashSet;

        let dir = tempfile::tempdir().unwrap();

        // --- SQLite source ---
        let db_path = dir.path().join("opencode.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                data TEXT NOT NULL
            );",
        )
        .unwrap();

        let shared_data_json = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "tokens": { "input": 500, "output": 200, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            "time": { "created": 1700000000000.0 }
        }"#;
        let sqlite_only_data_json = r#"{
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "tokens": { "input": 700, "output": 250, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            "time": { "created": 1700000001000.0 }
        }"#;

        // Insert two messages into SQLite
        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["msg_shared_001", "ses_001", shared_data_json],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)",
            rusqlite::params!["msg_sqlite_only", "ses_001", sqlite_only_data_json],
        )
        .unwrap();
        drop(conn);

        // --- JSON source ---
        let json_dir = dir.path().join("json");
        std::fs::create_dir_all(&json_dir).unwrap();

        // Duplicate of SQLite msg_shared_001
        let json_shared = r#"{
            "id": "msg_shared_001",
            "sessionID": "ses_001",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "tokens": { "input": 500, "output": 200, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            "time": { "created": 1700000000000.0 }
        }"#;
        std::fs::write(json_dir.join("msg_shared_001.json"), json_shared).unwrap();

        // JSON-only message (not in SQLite)
        let json_only = r#"{
            "id": "msg_json_only",
            "sessionID": "ses_001",
            "role": "assistant",
            "modelID": "claude-sonnet-4",
            "providerID": "anthropic",
            "tokens": { "input": 100, "output": 50, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            "time": { "created": 1700000000000.0 }
        }"#;
        std::fs::write(json_dir.join("msg_json_only.json"), json_only).unwrap();

        // --- Simulate the dedup logic from lib.rs ---
        let sqlite_messages = parse_opencode_sqlite(&db_path);
        assert_eq!(sqlite_messages.len(), 2);

        // Build seen set from SQLite (same as lib.rs)
        let mut seen: HashSet<String> = HashSet::new();
        for msg in &sqlite_messages {
            if let Some(ref key) = msg.dedup_key {
                seen.insert(key.clone());
            }
        }
        assert_eq!(seen.len(), 2);

        // Parse JSON files
        let json_msg_shared = parse_opencode_file(&json_dir.join("msg_shared_001.json")).unwrap();
        let json_msg_only = parse_opencode_file(&json_dir.join("msg_json_only.json")).unwrap();

        // Filter JSON through seen set (same logic as lib.rs)
        let json_messages = vec![json_msg_shared, json_msg_only];
        let deduped: Vec<UnifiedMessage> = json_messages
            .into_iter()
            .filter(|msg| {
                msg.dedup_key
                    .as_ref()
                    .is_none_or(|key| seen.insert(key.clone()))
            })
            .collect();

        // msg_shared_001 should be filtered (duplicate), msg_json_only should survive
        assert_eq!(
            deduped.len(),
            1,
            "Only the JSON-only message should survive dedup"
        );
        assert_eq!(
            deduped[0].dedup_key,
            Some("msg_json_only".to_string()),
            "Surviving message should be the JSON-only one"
        );

        // Total unique messages = 2 from SQLite + 1 from JSON
        let total = sqlite_messages.len() + deduped.len();
        assert_eq!(total, 3, "Should have 3 unique messages total");
    }

    // -------------------------------------------------------------------------
    // Migration cache tests
    // -------------------------------------------------------------------------

    // =========================================================================
    // Incremental SQLite scan
    // =========================================================================
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    #[ignore] // Run manually with: cargo test integration -- --ignored
    fn test_parse_real_sqlite_db() {
        let home = std::env::var("HOME").unwrap();
        let db_path = PathBuf::from(format!("{}/.local/share/opencode/opencode.db", home));

        if !db_path.exists() {
            println!("Skipping: OpenCode database not found at {:?}", db_path);
            return;
        }

        let messages = parse_opencode_sqlite(&db_path);
        println!("Parsed {} messages from SQLite", messages.len());

        if !messages.is_empty() {
            let first = &messages[0];
            println!(
                "First message: model={}, provider={}, tokens={:?}",
                first.model_id, first.provider_id, first.tokens
            );
        }

        assert!(
            !messages.is_empty(),
            "Expected to parse some messages from SQLite"
        );
    }
}
