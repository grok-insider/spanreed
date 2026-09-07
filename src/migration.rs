//! Local source inventory for reviewed migration. No credential material crosses IPC.
use fabrials_core::migration::CredentialKind;
pub use fabrials_core::migration::{MigrationCandidate, MigrationSelection};
use fabrials_runtime::migration::credential_kind;
use serde_json::Value;
pub mod session;

pub fn candidates() -> Result<Vec<MigrationCandidate>, String> {
    let vault = crate::accounts::lock_vault()?;
    let registry = vault.registry()?;
    registry
        .accounts
        .iter()
        .map(|account| {
            // Removal markers outlive re-enrollment to reject stale logins. The
            // current registry entry and its committed document define availability.
            let document =
                crate::accounts::read_secret_document(&account.provider, &account.alias)?;
            let kind = document
                .as_ref()
                .map(credential_kind)
                .unwrap_or(CredentialKind::Unavailable);
            Ok(MigrationCandidate {
                id: account.id.clone(),
                provider: account.provider.clone(),
                alias: account.alias.clone(),
                generation: account.generation.clone().unwrap_or_default(),
                credential_kind: kind,
            })
        })
        .collect()
}
/// Called by the authenticated transfer worker after the user approves a review.
/// Intentionally not exposed as a renderer command.
pub fn prepare_transfer(
    source_environment: &str,
    review: &fabrials_core::migration::MigrationReview,
) -> Result<Vec<fabrials_accounts::transfer::ApiKeyTransfer>, String> {
    let vault = crate::accounts::lock_vault()?;
    let registry = vault.registry()?;
    prepare_from_registry(
        source_environment,
        review,
        &registry,
        &crate::accounts::read_secret_document,
    )
}
fn prepare_from_registry(
    source_environment: &str,
    review: &fabrials_core::migration::MigrationReview,
    registry: &crate::accounts::Registry,
    read: &impl Fn(&str, &str) -> Result<Option<Value>, String>,
) -> Result<Vec<fabrials_accounts::transfer::ApiKeyTransfer>, String> {
    use fabrials_accounts::transfer::{ApiKey, ApiKeyTransfer};
    use fabrials_core::migration::MigrationAction;
    if source_environment.is_empty() || review.source_environment != source_environment {
        return Err("Migration source environment changed".into());
    }
    let mut entries = Vec::new();
    for item in &review.items {
        let account = registry
            .accounts
            .iter()
            .find(|account| account.id == item.source_id)
            .ok_or("Migration source account no longer exists")?;
        if account.provider != item.provider
            || account.generation.as_deref() != Some(item.source_generation.as_str())
        {
            return Err("Migration source account changed; review it again".into());
        }
        if item.action == MigrationAction::AuthorizeOAuth {
            continue;
        }
        let document = read(&account.provider, &account.alias)?
            .ok_or("Migration source credential unavailable")?;
        let api_key = ApiKey::from_document(&document)?;
        entries.push(ApiKeyTransfer {
            source_id: account.id.clone(),
            source_generation: item.source_generation.clone(),
            provider: account.provider.clone(),
            target_alias: item.target_alias.clone(),
            api_key,
        });
    }
    fabrials_runtime::migration::validate_transfer(
        review,
        &review.destination_environment,
        &entries,
    )?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_preparation_skips_oauth_and_refuses_replaced_keys() {
        use fabrials_core::migration::{MigrationAction, MigrationItem, MigrationReview};
        let mut registry = crate::accounts::Registry::default();
        let api = crate::accounts::Account::new("nous", "api").unwrap();
        let oauth = crate::accounts::Account::new("grok", "oauth").unwrap();
        let review = MigrationReview {
            source_environment: "local".into(),
            destination_environment: "hosted".into(),
            items: vec![
                MigrationItem {
                    source_id: api.id.clone(),
                    source_generation: api.generation.clone().unwrap(),
                    provider: "nous".into(),
                    target_alias: "copied".into(),
                    action: MigrationAction::CopyApiKey,
                },
                MigrationItem {
                    source_id: oauth.id.clone(),
                    source_generation: oauth.generation.clone().unwrap(),
                    provider: "grok".into(),
                    target_alias: "new-grant".into(),
                    action: MigrationAction::AuthorizeOAuth,
                },
            ],
        };
        registry.accounts = vec![api, oauth];
        registry
            .removed
            .insert("nous/api".into(), "previous-removal".into());
        let load = |provider: &str, alias: &str| {
            assert_eq!((provider, alias), ("nous", "api"));
            Ok(Some(serde_json::json!({"api_key":"synthetic-key"})))
        };
        let result = prepare_from_registry("local", &review, &registry, &load).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].api_key.expose(), "synthetic-key");
        registry.accounts[0].generation = Some("replaced".into());
        assert!(
            prepare_from_registry("local", &review, &registry, &|_, _| panic!(
                "changed credential must not be read"
            ))
            .is_err()
        );
    }
    #[test]
    fn opaque_or_mixed_documents_cannot_be_classified_for_key_copy() {
        assert_eq!(
            credential_kind(&serde_json::json!({"api_key":"synthetic"})),
            CredentialKind::ApiKey
        );
        assert_eq!(
            credential_kind(&serde_json::json!({"key":"synthetic"})),
            CredentialKind::ApiKey
        );
        for value in [
            serde_json::json!({"auth":{"key":"access","refresh_token":"refresh"}}),
            serde_json::json!({"api_key":"synthetic","refresh_token":"refresh"}),
            serde_json::json!({"type":"oauth","key":"access"}),
        ] {
            assert_eq!(credential_kind(&value), CredentialKind::OAuth);
        }
        for value in [
            serde_json::json!({"auth":{"key":"opaque"}}),
            serde_json::json!({"api_key":" "}),
            serde_json::json!({"api_key":"one","key":"two"}),
        ] {
            assert_eq!(credential_kind(&value), CredentialKind::Unavailable);
        }
    }
}
