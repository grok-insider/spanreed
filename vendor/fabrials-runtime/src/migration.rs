//! Credential-bearing migration boundary. Only hosts hold approved reviews.
use fabrials_accounts::transfer::ApiKeyTransfer;
use fabrials_core::migration::{MigrationAction, MigrationReview};
use std::collections::HashSet;
pub mod client;
pub mod wire;

/// Classify source metadata without ever treating an OAuth grant as a static key.
pub fn credential_kind(document: &serde_json::Value) -> fabrials_core::migration::CredentialKind {
    use fabrials_core::migration::CredentialKind;
    if fabrials_accounts::transfer::contains_oauth_material(document) {
        return CredentialKind::OAuth;
    }
    if fabrials_accounts::transfer::ApiKey::from_document(document).is_ok() {
        CredentialKind::ApiKey
    } else {
        CredentialKind::Unavailable
    }
}

/// Bind a transport payload to a host-owned review. This does not authorize the
/// sender: the adapter must authenticate the transfer and commit atomically.
pub fn validate_transfer<'a>(
    review: &MigrationReview,
    destination_environment: &str,
    entries: &'a [ApiKeyTransfer],
) -> Result<Vec<&'a ApiKeyTransfer>, &'static str> {
    if destination_environment.is_empty()
        || review.destination_environment != destination_environment
        || review.source_environment.is_empty()
        || review.source_environment == destination_environment
        || review.items.is_empty()
        || review.items.len() > 32
    {
        return Err("Migration environment or review is invalid");
    }
    let expected = review
        .items
        .iter()
        .filter(|item| item.action == MigrationAction::CopyApiKey)
        .count();
    if entries.len() != expected {
        return Err("Transfer does not match reviewed API keys");
    }
    let mut source_ids = HashSet::new();
    let mut destination_ids = HashSet::new();
    let mut approved = Vec::with_capacity(expected);
    for item in &review.items {
        let valid_name = |name: &str| {
            !name.is_empty()
                && name.len() <= 128
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        };
        if !valid_name(&item.provider)
            || !valid_name(&item.target_alias)
            || item.source_generation.is_empty()
            || !item
                .source_id
                .strip_prefix(&format!("{}/", item.provider))
                .is_some_and(valid_name)
            || !source_ids.insert(&item.source_id)
            || !destination_ids.insert((&item.provider, &item.target_alias))
        {
            return Err("Migration account identity is invalid");
        }
        if item.action == MigrationAction::AuthorizeOAuth {
            continue;
        }
        let mut matching = entries
            .iter()
            .filter(|entry| entry.source_id == item.source_id);
        let entry = matching.next().ok_or("Reviewed API key is missing")?;
        if matching.next().is_some()
            || entry.source_generation != item.source_generation
            || entry.provider != item.provider
            || entry.target_alias != item.target_alias
        {
            return Err("Transfer account changed since review");
        }
        approved.push(entry);
    }
    Ok(approved)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ImportReceipt {
    fingerprint: String,
    account_ids: Vec<String>,
}

/// The review and import ID must come from the authenticated host session, never
/// directly from an unauthenticated request. Only static keys are persisted here.
pub fn import_files(
    root: &std::path::Path,
    destination_environment: &str,
    import_id: &str,
    review: &MigrationReview,
    entries: &[ApiKeyTransfer],
) -> Result<Vec<String>, String> {
    import_files_using(
        root,
        destination_environment,
        import_id,
        review,
        entries,
        || Ok(()),
    )
}

fn import_files_using(
    root: &std::path::Path,
    destination_environment: &str,
    import_id: &str,
    review: &MigrationReview,
    entries: &[ApiKeyTransfer],
    before_commit: impl FnOnce() -> Result<(), String>,
) -> Result<Vec<String>, String> {
    use crate::file_set::{Change, FileSet};
    use sha2::{Digest, Sha256};
    if import_id.is_empty()
        || import_id.len() > 128
        || !import_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err("Invalid import identity".into());
    }
    let approved = validate_transfer(review, destination_environment, entries)?;
    let serialized = serde_json::to_vec(&(review, entries)).map_err(|_| "Invalid transfer")?;
    let fingerprint = hex::encode(Sha256::digest(&serialized));
    let files = FileSet::acquire_wait(root)?;
    let receipt_path = format!("imports/{import_id}.json");
    fn read(path: &std::path::Path, max: u64) -> Result<Option<Vec<u8>>, String> {
        use std::io::Read;
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Import storage unavailable".into()),
        };
        let mut bytes = Vec::new();
        file.take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Import storage unavailable")?;
        if bytes.len() as u64 > max {
            return Err("Import storage exceeds size limit".into());
        }
        Ok(Some(bytes))
    }
    if let Some(bytes) = read(&root.join(&receipt_path), 64 * 1024)? {
        let receipt: ImportReceipt =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid import receipt")?;
        return if receipt.fingerprint == fingerprint {
            Ok(receipt.account_ids)
        } else {
            Err("Import identity was already used for another transfer".into())
        };
    }
    let mut registry: fabrials_accounts::Registry =
        match read(&root.join("index.json"), 8 * 1024 * 1024)? {
            Some(bytes) => {
                serde_json::from_slice(&bytes).map_err(|_| "Invalid account registry")?
            }
            None => Default::default(),
        };
    let mut names = HashSet::new();
    for account in &registry.accounts {
        if account.id != format!("{}/{}", account.provider, account.alias)
            || !names.insert((&account.provider, &account.alias))
        {
            return Err("Invalid account registry identity".into());
        }
        for alias in &account.aliases {
            if !names.insert((&account.provider, alias)) {
                return Err("Ambiguous account alias".into());
            }
        }
    }
    let mut changes = Vec::new();
    let mut account_ids = Vec::new();
    for entry in approved {
        let id = format!("{}/{}", entry.provider, entry.target_alias);
        if registry
            .accounts
            .iter()
            .any(|a| a.alias == entry.target_alias || a.aliases.contains(&entry.target_alias))
            || registry.removed.contains_key(&id)
        {
            return Err("Destination alias changed or was removed since review".into());
        }
        let path = format!("{}/{}.json", entry.provider, entry.target_alias);
        match std::fs::symlink_metadata(root.join(&path)) {
            Ok(_) => return Err("Destination credential already exists".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Destination credential unavailable".into()),
        }
        let mut account = fabrials_accounts::Account::new(&entry.provider, &entry.target_alias)?;
        account.generation = Some(crate::accounting::new_request_id());
        account.active = false;
        registry.accounts.push(account);
        changes.push(Change {
            path,
            contents: Some(serde_json::json!({"api_key":entry.api_key.expose()}).to_string()),
        });
        account_ids.push(id);
    }
    let receipt = ImportReceipt {
        fingerprint,
        account_ids: account_ids.clone(),
    };
    let index = serde_json::to_string_pretty(&registry).map_err(|_| "Invalid account registry")?;
    if index.len() > 8 * 1024 * 1024 {
        return Err("Account registry exceeds size limit".into());
    }
    changes.push(Change {
        path: "index.json".into(),
        contents: Some(index),
    });
    changes.push(Change {
        path: receipt_path,
        contents: Some(serde_json::to_string(&receipt).map_err(|_| "Invalid import receipt")?),
    });
    before_commit()?;
    files.commit(changes)?;
    Ok(account_ids)
}

#[cfg(test)]
mod file_tests {
    use super::*;
    use fabrials_accounts::transfer::ApiKey;
    use fabrials_core::migration::MigrationItem;
    #[test]
    fn import_replays_interrupted_batch_and_never_recreates_deleted_accounts() {
        let root = std::env::temp_dir().join(format!(
            "fabrials-import-{}",
            crate::accounting::new_request_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let item = MigrationItem {
            source_id: "nous/source".into(),
            source_generation: "g1".into(),
            provider: "nous".into(),
            target_alias: "destination".into(),
            action: MigrationAction::CopyApiKey,
        };
        let review = MigrationReview {
            source_environment: "remote".into(),
            destination_environment: "local".into(),
            items: vec![item],
        };
        let entries = vec![ApiKeyTransfer {
            source_id: "nous/source".into(),
            source_generation: "g1".into(),
            provider: "nous".into(),
            target_alias: "destination".into(),
            api_key: ApiKey::new("synthetic-key".into()).unwrap(),
        }];
        // Fail the receipt write after committing the journal and account files.
        assert!(
            import_files_using(&root, "local", "request-one", &review, &entries, || {
                std::fs::write(root.join("imports"), b"block receipt directory").unwrap();
                Ok(())
            })
            .is_err()
        );
        assert!(root.join(".transaction.json").exists());
        std::fs::remove_file(root.join("imports")).unwrap();
        assert_eq!(
            import_files(&root, "local", "request-one", &review, &entries).unwrap(),
            vec!["nous/destination"]
        );
        assert!(!root.join(".transaction.json").exists());
        let registry: fabrials_accounts::Registry =
            serde_json::from_slice(&std::fs::read(root.join("index.json")).unwrap()).unwrap();
        assert_eq!(registry.accounts.len(), 1);
        assert!(!registry.accounts[0].active);
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("nous/destination.json")).unwrap())
                .unwrap();
        assert_eq!(stored, serde_json::json!({"api_key":"synthetic-key"}));
        let mut changed = entries.clone();
        changed[0].api_key = ApiKey::new("different-key".into()).unwrap();
        assert!(import_files(&root, "local", "request-one", &review, &changed).is_err());
        assert!(import_files(&root, "local", "request-two", &review, &entries).is_err());
        let files = crate::file_set::FileSet::acquire(&root).unwrap();
        files
            .commit(vec![
                crate::file_set::Change {
                    path: "index.json".into(),
                    contents: Some("{\"accounts\":[]}".into()),
                },
                crate::file_set::Change {
                    path: "nous/destination.json".into(),
                    contents: None,
                },
            ])
            .unwrap();
        drop(files);
        assert_eq!(
            import_files(&root, "local", "request-one", &review, &entries).unwrap(),
            vec!["nous/destination"]
        );
        assert!(!root.join("nous/destination.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabrials_accounts::transfer::ApiKey;
    use fabrials_core::migration::MigrationItem;
    #[test]
    fn transfer_cannot_replace_reviewed_identity_or_include_oauth_grants() {
        let review = MigrationReview {
            source_environment: "source".into(),
            destination_environment: "destination".into(),
            items: vec![
                MigrationItem {
                    source_id: "nous/api".into(),
                    source_generation: "g1".into(),
                    provider: "nous".into(),
                    target_alias: "copied".into(),
                    action: MigrationAction::CopyApiKey,
                },
                MigrationItem {
                    source_id: "grok/oauth".into(),
                    source_generation: "g2".into(),
                    provider: "grok".into(),
                    target_alias: "fresh".into(),
                    action: MigrationAction::AuthorizeOAuth,
                },
            ],
        };
        let entry = ApiKeyTransfer {
            source_id: "nous/api".into(),
            source_generation: "g1".into(),
            provider: "nous".into(),
            target_alias: "copied".into(),
            api_key: ApiKey::new("synthetic".into()).unwrap(),
        };
        assert_eq!(
            validate_transfer(&review, "destination", std::slice::from_ref(&entry))
                .unwrap()
                .len(),
            1
        );
        assert!(validate_transfer(&review, "different", std::slice::from_ref(&entry)).is_err());
        assert!(validate_transfer(&review, "destination", &[]).is_err());
        assert!(
            validate_transfer(&review, "destination", &[entry.clone(), entry.clone()]).is_err()
        );
        for field in ["source", "generation", "provider", "alias"] {
            let mut changed = entry.clone();
            match field {
                "source" => changed.source_id = "grok/oauth".into(),
                "generation" => changed.source_generation = "g2".into(),
                "provider" => changed.provider = "openai".into(),
                _ => changed.target_alias = "overwrite".into(),
            };
            assert!(validate_transfer(&review, "destination", &[changed]).is_err());
        }
        let mut invalid = review.clone();
        invalid.items[1].target_alias = "../escape".into();
        assert!(validate_transfer(&invalid, "destination", &[entry]).is_err());
    }
}
