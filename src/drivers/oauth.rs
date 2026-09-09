use crate::{accounts, util};
use fabrials_runtime::credential_journal::{Recovery, Rotation, Scope};

type PendingQueue = fabrials_core::recovery::RecoveryQueue<String, serde_json::Value>;

pub fn token(provider: &str, alias: &str) -> Result<String, String> {
    let vault = accounts::lock_vault()?;
    let generation = vault
        .registry()?
        .accounts
        .into_iter()
        .find(|account| account.id == format!("{provider}/{alias}"))
        .ok_or("Provider account no longer exists")?
        .generation
        .unwrap_or_default();
    let environment = crate::app::data_dir().to_string_lossy().into_owned();
    let journal = Rotation::acquire(
        &crate::app::data_dir().join("credential-recovery"),
        Scope {
            environment: &environment,
            owner: "local",
            provider,
            alias,
        },
    )?;
    static QUEUE: std::sync::OnceLock<std::sync::Mutex<PendingQueue>> = std::sync::OnceLock::new();
    let mut queue = QUEUE
        .get_or_init(|| std::sync::Mutex::new(PendingQueue::new(64)))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let key = format!(
        "{}:{provider}/{alias}:{generation}",
        crate::app::data_dir().display()
    );
    if !queue.reserve(key.clone()) {
        return Err("Credential recovery capacity reached; retry later".into());
    }
    let result = token_with_recovery(provider, alias, &key, &mut queue, &journal, &vault);
    queue.release_reservation(&key);
    result
}

fn token_with_recovery(
    provider: &str,
    alias: &str,
    key: &String,
    queue: &mut PendingQueue,
    journal: &Rotation,
    vault: &accounts::Vault,
) -> Result<String, String> {
    let mut document =
        accounts::read_secret_document(provider, alias)?.ok_or("Provider credential missing")?;
    if let Some(pending) = queue.take_reserved(key) {
        if document == pending.expected && journal.rotated(&pending.replacement).is_err() {
            let token = valid_access(&pending.replacement, util::now_ms());
            queue.insert(key.clone(), pending, util::now_ms())?;
            return token;
        }
    }
    match journal.recover(&document)? {
        Recovery::Clean => {}
        Recovery::Interrupted => {
            return valid_access(&document, util::now_ms()).map_err(|_| {
                "Previous Provider refresh was interrupted; authorize this account again".into()
            })
        }
        Recovery::Replacement(replacement) => {
            if accounts::read_secret_document(provider, alias)?.as_ref() != Some(&document) {
                return Err("Provider account changed during recovery".into());
            }
            if vault
                .write_secret(provider, alias, &replacement.to_string())
                .is_err()
            {
                return valid_access(&replacement, util::now_ms());
            }
            journal.complete()?;
            document = replacement;
        }
    }
    if let Some(key) = document
        .get("api_key")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        return Ok(key.into());
    }
    let now = util::now_ms();
    if document
        .get("expires_at_ms")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        <= now + 120_000
    {
        journal.begin(&document)?;
        let refreshed = match match provider {
            "codex" => fabrials_providers::codex::auth::Client::new()?.refresh(&document, now),
            "nous" => fabrials_providers::nous::Client::new(None)?.refresh(&document, now),
            _ => return Err("Unsupported OAuth provider".into()),
        } {
            Ok(refreshed) => refreshed,
            Err(error) => return valid_access(&document, now).map_err(|_| error),
        };
        let mut replacement = document.clone();
        replacement
            .as_object_mut()
            .ok_or("Invalid Provider credential")?
            .extend(
                refreshed
                    .as_object()
                    .ok_or("Invalid Provider refresh")?
                    .clone(),
            );
        if journal.rotated(&replacement).is_err() {
            let token = valid_access(&replacement, now);
            let expiry = replacement["expires_at_ms"]
                .as_i64()
                .ok_or("Provider expiry missing")?;
            queue.insert(
                key.clone(),
                fabrials_core::recovery::PendingCredential {
                    expected: document,
                    replacement,
                    expires_at_ms: expiry,
                },
                now,
            )?;
            return token;
        }
        if accounts::read_secret_document(provider, alias)?.as_ref() != Some(&document)
            || !vault
                .registry()?
                .accounts
                .iter()
                .any(|a| a.id == format!("{provider}/{alias}"))
        {
            return Err("Provider account changed during refresh".into());
        }
        if vault
            .write_secret(provider, alias, &replacement.to_string())
            .is_ok()
        {
            journal.complete()?;
        }
        document = replacement;
    }
    valid_access(&document, util::now_ms())
}

fn valid_access(document: &serde_json::Value, now: i64) -> Result<String, String> {
    fabrials_providers::oauth::valid_access(document, now)
        .ok_or("Provider authorization unavailable or expired; sign in again".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_pending_access_is_never_served() {
        let grant = serde_json::json!({"access_token":"fixture","refresh_token":"next-generation","expires_at_ms":100});
        assert_eq!(valid_access(&grant, 99).unwrap(), "fixture");
        assert!(valid_access(&grant, 100).is_err());
        assert!(valid_access(&grant, 101).is_err());
    }
}
