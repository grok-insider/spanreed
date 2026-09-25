//! Account lifecycle across the vault, migration, API keys, notifications and
//! model discovery, in an isolated HOME. It sits at the crate root because
//! it exercises modules above `accounts`.

use crate::accounts::test_support::*;
use crate::accounts::{
    put_secret, read_secret_document, remove, remove_if_current, replace_secret, routing_registry,
};
use std::fs;

#[test]
fn account_lifecycle_in_isolated_process() {
    let root = std::env::temp_dir().join(format!(
        "spanreed-vault-{}",
        fabrials_fabric::accounting::new_request_id()
    ));
    fs::create_dir_all(&root).unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "lifecycle_tests::account_lifecycle_fixture",
            "--nocapture",
        ])
        .env_clear()
        .env("HOME", &root)
        .env("USERPROFILE", &root)
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("APPDATA", root.join("config"))
        .env("LOCALAPPDATA", root.join("data"))
        .env("SPANREED_OFFLINE", "1")
        .env("SPANREED_VAULT_FIXTURE", "1")
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
#[ignore = "Runs in an isolated HOME through account_lifecycle_in_isolated_process"]
fn account_lifecycle_fixture() {
    assert_eq!(std::env::var("SPANREED_VAULT_FIXTURE").unwrap(), "1");
    {
        let vault = lock_vault().unwrap();
        let ghost = new_account("grok", "disconnected").unwrap();
        let mut registry = vault.registry().unwrap();
        registry
            .removed
            .insert(ghost.id.clone(), "fixture-disconnection".into());
        registry.accounts.push(ghost);
        vault.commit(&registry, Vec::new()).unwrap();
    }
    assert!(
        get_secret_using("grok", "disconnected", |_, _| panic!(
            "must not revive keyring credential"
        ))
        .is_none()
    );
    assert_eq!(
        crate::migration::candidates().unwrap()[0].credential_kind,
        fabrials_types::migration::CredentialKind::Unavailable
    );
    {
        let vault = lock_vault().unwrap();
        let mut registry = vault.registry().unwrap();
        registry
            .accounts
            .retain(|account| account.id != "grok/disconnected");
        vault.commit(&registry, Vec::new()).unwrap();
    }

    assert!(!crate::notifications::settings().unwrap().reset_expiry);
    assert_eq!(
        crate::notifications::deliver_outputs(&[], |_, _| panic!("notifications default off"))
            .unwrap(),
        0
    );
    crate::notifications::set_enabled(true).unwrap();
    assert!(crate::notifications::settings().unwrap().reset_expiry);
    crate::notifications::set_enabled(false).unwrap();
    let original = serde_json::json!({"access_token":"fixture-old"});
    let replacement = serde_json::json!({"access_token":"fixture-new"});
    {
        let vault = lock_vault().unwrap();
        let inactive = new_account("nous", "migration-fixture").unwrap();
        vault
            .register_with_activation(inactive, &original.to_string(), None, false)
            .unwrap();
        assert!(
            !vault
                .registry()
                .unwrap()
                .accounts
                .iter()
                .find(|a| a.id == "nous/migration-fixture")
                .unwrap()
                .active
        );
    }
    remove("nous/migration-fixture").unwrap();
    let account = new_account("nous", "work").unwrap();
    {
        let vault = lock_vault().unwrap();
        vault
            .register(account.clone(), &original.to_string(), None)
            .unwrap();
        vault
            .register(account.clone(), &original.to_string(), None)
            .unwrap();
        assert!(fabrials_store_sqlite::file_set::FileSet::acquire(&dir()).is_err());
    }
    let journal = rotation("nous", "work").unwrap();
    journal.begin(&original).unwrap();
    journal.rotated(&replacement).unwrap();
    drop(journal);
    let moved = rename("nous", "work", "personal").unwrap();
    assert_eq!(moved.generation, account.generation);
    assert_eq!(
        read_secret_document("nous", "personal").unwrap(),
        Some(replacement.clone())
    );
    assert!(!file_secret_path("nous", "work").exists());
    assert!(replace_secret(&account, &original.to_string()).is_err());
    let journal = rotation("nous", "personal").unwrap();
    journal.begin(&replacement).unwrap();
    drop(journal);
    assert!(rename("nous", "personal", "other").is_err());
    remove("nous/personal").unwrap();
    assert!(!file_secret_path("nous", "personal").exists());
    let vault = lock_vault().unwrap();
    assert!(
        vault
            .register(moved.clone(), &replacement.to_string(), None)
            .is_err()
    );
    let removal = vault.registry().unwrap().removed.get(&moved.id).cloned();
    let fresh = new_account("nous", "personal").unwrap();
    vault
        .register(fresh, &original.to_string(), removal.as_ref())
        .unwrap();
    drop(vault);
    assert!(replace_secret(&moved, &replacement.to_string()).is_err());
    assert_eq!(routing_registry().unwrap().accounts.len(), 1);
    let api_account = crate::account_keys::add("openai", "api", "fixture-api-key").unwrap();
    assert!(api_account.active);
    assert!(crate::account_keys::add("openai", "api", "replacement-key").is_err());
    assert_eq!(
        read_secret_document("openai", "api").unwrap().unwrap()["api_key"],
        "fixture-api-key"
    );
    assert!(crate::account_keys::add("unsupported", "other", "fixture").is_err());
    assert_eq!(routing_registry().unwrap().accounts.len(), 2);
    assert!(crate::app::accounts::models("openai/does-not-exist").is_err());
    replace_authorization(
        &api_account,
        "fixture-reauth-generation",
        r#"{"api_key":"fixture-new-grant"}"#,
    )
    .unwrap();
    put_secret("openai", "api", r#"{"api_key":"fixture-rotated-grant"}"#).unwrap();
    replace_authorization(
        &api_account,
        "fixture-reauth-generation",
        r#"{"api_key":"fixture-new-grant"}"#,
    )
    .unwrap();
    assert_eq!(
        read_secret_document("openai", "api").unwrap().unwrap()["api_key"],
        "fixture-rotated-grant"
    );
    assert!(
        replace_authorization(
            &api_account,
            "different-attempt",
            r#"{"api_key":"fixture-stale"}"#
        )
        .is_err()
    );
    assert!(
        crate::account_keys::replace(
            &api_account.id,
            api_account.generation.as_deref(),
            "stale-key"
        )
        .is_err()
    );
    assert!(remove_if_current(&api_account.id, api_account.generation.as_deref()).is_err());
    let current = routing_registry()
        .unwrap()
        .accounts
        .into_iter()
        .find(|a| a.id == api_account.id)
        .unwrap();
    crate::account_keys::replace(
        &current.id,
        current.generation.as_deref(),
        "replacement-api-key",
    )
    .unwrap();
    assert_eq!(
        read_secret_document("openai", "api").unwrap().unwrap()["api_key"],
        "replacement-api-key"
    );
    assert!(remove_if_current(&current.id, current.generation.as_deref()).is_err());
    let current = routing_registry()
        .unwrap()
        .accounts
        .into_iter()
        .find(|a| a.id == api_account.id)
        .unwrap();
    remove_if_current(&current.id, current.generation.as_deref()).unwrap();
    assert!(read_secret_document("openai", "api").unwrap().is_none());
    assert!(
        crate::account_keys::replace(&current.id, current.generation.as_deref(), "removed-key")
            .is_err()
    );
    let recreated = crate::account_keys::add("openai", "api", "recreated-api-key").unwrap();
    let candidate = crate::migration::candidates()
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == recreated.id)
        .unwrap();
    assert_eq!(
        candidate.credential_kind,
        fabrials_types::migration::CredentialKind::ApiKey
    );
    assert_eq!(Some(candidate.generation), recreated.generation);
    assert_ne!(recreated.generation, current.generation);
    assert!(
        lock_vault()
            .unwrap()
            .registry()
            .unwrap()
            .removed
            .contains_key(&recreated.id)
    );
    remove_if_current(&recreated.id, recreated.generation.as_deref()).unwrap();
    let oauth = routing_registry()
        .unwrap()
        .accounts
        .into_iter()
        .find(|a| a.provider == "nous")
        .unwrap();
    assert!(
        crate::account_keys::replace(&oauth.id, oauth.generation.as_deref(), "api-key").is_err()
    );
    assert_eq!(
        read_secret_document(&oauth.provider, &oauth.alias)
            .unwrap()
            .unwrap(),
        original
    );
}
