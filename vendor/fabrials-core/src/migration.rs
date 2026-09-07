//! Secret-free review contracts for explicit account migration between environments.
use crate::ProviderDescriptor;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialKind {
    ApiKey,
    #[serde(rename = "oauth")]
    OAuth,
    Unavailable,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationCandidate {
    pub id: String,
    pub provider: String,
    pub alias: String,
    pub generation: String,
    pub credential_kind: CredentialKind,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationSelection {
    pub source_id: String,
    pub target_alias: String,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MigrationAction {
    CopyApiKey,
    AuthorizeOAuth,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationItem {
    pub source_id: String,
    pub source_generation: String,
    pub provider: String,
    pub target_alias: String,
    pub action: MigrationAction,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationReview {
    pub source_environment: String,
    pub destination_environment: String,
    pub items: Vec<MigrationItem>,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MigrationDirection {
    LocalToHosted,
    HostedToLocal,
}
impl MigrationDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalToHosted => "localToHosted",
            Self::HostedToLocal => "hostedToLocal",
        }
    }
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationInvitation {
    pub id: String,
    pub secret: String,
    pub expires_at_ms: i64,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationSessionView {
    pub id: String,
    pub owner_id: String,
    pub direction: MigrationDirection,
    pub hosted_environment: String,
    pub local_environment: Option<String>,
    pub phase: String,
    pub review: Option<MigrationReview>,
    pub revision: Option<String>,
    pub expires_at_ms: i64,
}

fn valid_alias(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

/// Inputs are host-owned snapshots; callers must recheck generations and destination
/// uniqueness when committing. A review grants no authority to redeem OAuth grants.
pub fn review(
    source: &str,
    destination: &str,
    candidates: &[MigrationCandidate],
    selection: &[MigrationSelection],
    destination_providers: &[ProviderDescriptor],
    occupied_ids: &[String],
) -> Result<MigrationReview, String> {
    if source.is_empty() || destination.is_empty() || source == destination {
        return Err("Choose two different environments".into());
    }
    if selection.is_empty() || selection.len() > 32 {
        return Err("Select between one and 32 accounts".into());
    }
    let mut seen_source = HashSet::new();
    let mut seen_target = HashSet::new();
    let mut items = Vec::with_capacity(selection.len());
    for selected in selection {
        if !seen_source.insert(&selected.source_id) {
            return Err("Account selected more than once".into());
        }
        let mut matching = candidates.iter().filter(|a| a.id == selected.source_id);
        let account = matching.next().ok_or("Selected account no longer exists")?;
        if matching.next().is_some()
            || account.generation.is_empty()
            || account.id != format!("{}/{}", account.provider, account.alias)
            || !valid_alias(&account.provider)
            || !valid_alias(&account.alias)
        {
            return Err("Source account identity is invalid; reload accounts".into());
        }
        if !valid_alias(&selected.target_alias) {
            return Err(
                "Destination alias must contain letters, digits, underscores or hyphens".into(),
            );
        }
        let target = format!("{}/{}", account.provider, selected.target_alias);
        if occupied_ids.contains(&target) || !seen_target.insert(target) {
            return Err("Destination account already exists; choose another alias".into());
        }
        let provider = destination_providers
            .iter()
            .find(|p| p.id == account.provider)
            .ok_or("Provider is unavailable in the destination environment")?;
        let action = match account.credential_kind {
            CredentialKind::ApiKey if provider.capabilities.api_key => MigrationAction::CopyApiKey,
            CredentialKind::OAuth if provider.capabilities.oauth => MigrationAction::AuthorizeOAuth,
            CredentialKind::Unavailable => {
                return Err("Selected account needs authorization before migration".into())
            }
            _ => return Err("Destination does not support this authorization type".into()),
        };
        items.push(MigrationItem {
            source_id: account.id.clone(),
            source_generation: account.generation.clone(),
            provider: account.provider.clone(),
            target_alias: selected.target_alias.clone(),
            action,
        });
    }
    Ok(MigrationReview {
        source_environment: source.into(),
        destination_environment: destination.into(),
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(alias: &str, kind: CredentialKind) -> MigrationCandidate {
        MigrationCandidate {
            id: format!("nous/{alias}"),
            provider: "nous".into(),
            alias: alias.into(),
            generation: "generation-a".into(),
            credential_kind: kind,
        }
    }
    fn providers() -> Vec<ProviderDescriptor> {
        vec![ProviderDescriptor {
            id: "nous".into(),
            name: "Nous".into(),
            capabilities: crate::Capabilities {
                api_key: true,
                oauth: true,
                ..Default::default()
            },
        }]
    }
    #[test]
    fn mixed_selection_requires_independent_oauth_and_preserves_generations() {
        let candidates = vec![
            candidate("api", CredentialKind::ApiKey),
            candidate("oauth", CredentialKind::OAuth),
        ];
        let selection = vec![
            MigrationSelection {
                source_id: "nous/oauth".into(),
                target_alias: "new-oauth".into(),
            },
            MigrationSelection {
                source_id: "nous/api".into(),
                target_alias: "new-api".into(),
            },
        ];
        let result = review(
            "local-install",
            "hosted-install",
            &candidates,
            &selection,
            &providers(),
            &[],
        )
        .unwrap();
        assert_eq!(result.items[0].action, MigrationAction::AuthorizeOAuth);
        assert_eq!(result.items[1].action, MigrationAction::CopyApiKey);
        assert_eq!(result.items[0].source_generation, "generation-a");
        assert_eq!(result.items[0].target_alias, "new-oauth");
    }
    #[test]
    fn rejects_overwrites_duplicates_stale_identities_and_unsupported_grants() {
        let candidates = vec![
            candidate("one", CredentialKind::ApiKey),
            candidate("two", CredentialKind::ApiKey),
        ];
        let selected = MigrationSelection {
            source_id: "nous/one".into(),
            target_alias: "target".into(),
        };
        assert!(review(
            "local",
            "hosted",
            &candidates,
            std::slice::from_ref(&selected),
            &providers(),
            &["nous/target".into()]
        )
        .is_err());
        assert!(review(
            "local",
            "hosted",
            &candidates,
            &[selected.clone(), selected.clone()],
            &providers(),
            &[]
        )
        .is_err());
        assert!(review(
            "local",
            "hosted",
            &candidates,
            &[
                selected.clone(),
                MigrationSelection {
                    source_id: "nous/two".into(),
                    target_alias: "target".into()
                }
            ],
            &providers(),
            &[]
        )
        .is_err());
        let mut malformed = candidates.clone();
        malformed[0].alias = "other".into();
        assert!(review(
            "local",
            "hosted",
            &malformed,
            std::slice::from_ref(&selected),
            &providers(),
            &[]
        )
        .is_err());
        let mut unsupported = providers();
        unsupported[0].capabilities.api_key = false;
        assert!(review(
            "local",
            "hosted",
            &candidates,
            std::slice::from_ref(&selected),
            &unsupported,
            &[]
        )
        .is_err());
        assert!(review(
            "local",
            "local",
            &candidates,
            &[selected],
            &providers(),
            &[]
        )
        .is_err());
    }
}

/// Providers supported by both Fabrials account vaults for reviewed migration.
pub fn destination_providers() -> Vec<ProviderDescriptor> {
    ["grok", "nous", "openai"].into_iter().map(|id| ProviderDescriptor {
        id: id.into(), name: id.into(), capabilities: crate::Capabilities {
            api_key: true, oauth: matches!(id, "grok" | "nous"), ..Default::default()
        },
    }).collect()
}
