//! Credential-bearing migration boundary. Only hosts hold approved reviews.
use fabrials_accounts::transfer::ApiKeyTransfer;
use fabrials_types::migration::{MigrationAction, MigrationReview};
use std::collections::HashSet;
pub mod client;
pub mod wire;

/// Classify source metadata without ever treating an OAuth grant as a static key.
pub fn credential_kind(document: &serde_json::Value) -> fabrials_types::migration::CredentialKind {
    use fabrials_types::migration::CredentialKind;
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

#[cfg(test)]
mod tests {
    use super::*;
    use fabrials_accounts::transfer::ApiKey;
    use fabrials_types::migration::MigrationItem;
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
