//! Adopting a grok-bridge install's state keeps its origin.

use fabrials_agent_host::journal::JOURNAL_FILE_NAME;
use fabrials_agent_host::state::{self, ORIGIN_FILE_NAME};
use fabrials_agent_host::workspace::WORKSPACES_FILE_NAME;
use fabrials_agent_host::{MigrationOutcome, migrate_legacy_state};

const INSTALL: &str = "00112233445566778899aabbccddeeff";
const PORT: u16 = 23_456;

/// A legacy directory laid out the way grok-bridge wrote it.
fn legacy_install(root: &std::path::Path) -> std::path::PathBuf {
    let legacy = root.join("grok-bridge");
    std::fs::create_dir(&legacy).expect("legacy dir");
    std::fs::write(
        legacy.join(ORIGIN_FILE_NAME),
        format!("{{\n  \"installId\": \"{INSTALL}\",\n  \"port\": {PORT}\n}}"),
    )
    .expect("origin");
    std::fs::write(legacy.join(WORKSPACES_FILE_NAME), r#"{"entries":[]}"#).expect("workspaces");
    std::fs::write(legacy.join(JOURNAL_FILE_NAME), r#"{"legacy":"journal"}"#).expect("journal");
    for leftover in ["host.lock", "control.sock", "control.pipe", "notes.txt"] {
        std::fs::write(legacy.join(leftover), "x").expect("leftover");
    }
    legacy
}

#[test]
fn a_grok_bridge_install_keeps_its_id_and_port_and_a_second_call_is_a_no_op() {
    let root = tempfile::tempdir().expect("tempdir");
    let legacy = legacy_install(root.path());
    let new_dir = root.path().join("spanreed").join("agent");

    let outcome = migrate_legacy_state(&new_dir, &legacy).expect("migrate");
    assert_eq!(
        outcome,
        MigrationOutcome::Migrated {
            install_id: INSTALL.to_owned(),
            port: PORT,
            files: vec![WORKSPACES_FILE_NAME, JOURNAL_FILE_NAME, ORIGIN_FILE_NAME],
        }
    );

    let identity = state::load_or_create(&new_dir).expect("identity");
    assert_eq!(identity.install_id, INSTALL);
    assert_eq!(identity.port, PORT);
    for name in [WORKSPACES_FILE_NAME, JOURNAL_FILE_NAME] {
        assert_eq!(
            std::fs::read(new_dir.join(name)).expect("copied"),
            std::fs::read(legacy.join(name)).expect("legacy"),
            "{name} is copied verbatim"
        );
    }
    let mut names: Vec<String> = std::fs::read_dir(&new_dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![JOURNAL_FILE_NAME, ORIGIN_FILE_NAME, WORKSPACES_FILE_NAME],
        "the lock, control socket and other files stay behind"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |path: &std::path::Path| {
            std::fs::metadata(path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(&new_dir), 0o700);
        for name in &names {
            assert_eq!(mode(&new_dir.join(name)), 0o600, "{name} is owner-only");
        }
    }

    // Idempotent: the new directory is initialised, so nothing is touched even
    // when the legacy state changes afterwards.
    std::fs::write(new_dir.join(JOURNAL_FILE_NAME), r#"{"new":"journal"}"#).expect("journal");
    std::fs::write(
        legacy.join(ORIGIN_FILE_NAME),
        r#"{"installId":"ffffffffffffffffffffffffffffffff","port":24000}"#,
    )
    .expect("rewrite legacy");
    assert_eq!(
        migrate_legacy_state(&new_dir, &legacy).expect("second call"),
        MigrationOutcome::AlreadyInitialised
    );
    assert_eq!(
        std::fs::read_to_string(new_dir.join(JOURNAL_FILE_NAME)).expect("journal"),
        r#"{"new":"journal"}"#
    );
    assert_eq!(
        state::load_or_create(&new_dir).expect("identity").port,
        PORT
    );
}

#[test]
fn nothing_to_adopt_leaves_the_new_directory_alone() {
    let root = tempfile::tempdir().expect("tempdir");
    let new_dir = root.path().join("agent");
    assert_eq!(
        migrate_legacy_state(&new_dir, &root.path().join("missing")).expect("migrate"),
        MigrationOutcome::NoLegacyState
    );
    assert!(
        !new_dir.exists(),
        "no directory is created without legacy state"
    );
}

#[test]
fn files_already_in_the_new_directory_are_not_overwritten() {
    let root = tempfile::tempdir().expect("tempdir");
    let legacy = legacy_install(root.path());
    let new_dir = root.path().join("agent");
    std::fs::create_dir(&new_dir).expect("new dir");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&new_dir, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    }
    std::fs::write(new_dir.join(WORKSPACES_FILE_NAME), r#"{"entries":["new"]}"#)
        .expect("workspaces");

    let outcome = migrate_legacy_state(&new_dir, &legacy).expect("migrate");
    assert_eq!(
        outcome,
        MigrationOutcome::Migrated {
            install_id: INSTALL.to_owned(),
            port: PORT,
            files: vec![JOURNAL_FILE_NAME, ORIGIN_FILE_NAME],
        }
    );
    assert_eq!(
        std::fs::read_to_string(new_dir.join(WORKSPACES_FILE_NAME)).expect("workspaces"),
        r#"{"entries":["new"]}"#
    );
}

#[test]
fn a_malformed_legacy_identity_is_reported() {
    let root = tempfile::tempdir().expect("tempdir");
    let legacy = root.path().join("grok-bridge");
    std::fs::create_dir(&legacy).expect("legacy");
    std::fs::write(legacy.join(ORIGIN_FILE_NAME), "not json").expect("origin");
    let new_dir = root.path().join("agent");
    assert!(migrate_legacy_state(&new_dir, &legacy).is_err());
    assert!(!new_dir.join(ORIGIN_FILE_NAME).exists());
}
