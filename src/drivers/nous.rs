use crate::{accounts, util};
use fabrials_providers::nous::{device_flow::Progress, Client};
use fabrials_runtime::credential_journal::{Recovery, Rotation, Scope};

pub fn login_add(name: Option<&str>) -> Result<String, String> {
    let alias = name
        .map(str::to_string)
        .unwrap_or_else(|| accounts::unique_alias("nous", "portal"));
    let login = crate::account_login::begin_nous(alias)?;
    eprintln!(
        "Open {}\nAuthorization code: {}",
        login.device.verification_uri, login.device.user_code
    );
    let mut interval = login.device.interval;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(interval));
        match crate::account_login::poll_nous(&login.id)? {
            Progress::Pending { retry_after_secs } => interval = retry_after_secs,
            Progress::Connected => return Ok(format!("Connected nous/{}", login.alias)),
        }
    }
}

type PendingQueue = fabrials_core::recovery::RecoveryQueue<String, serde_json::Value>;

pub fn token(alias: &str) -> Result<String, String> {
    let vault = accounts::lock_vault()?;
    let generation = vault
        .registry()?
        .accounts
        .into_iter()
        .find(|account| account.id == format!("nous/{alias}"))
        .ok_or("Nous account no longer exists")?
        .generation
        .unwrap_or_default();
    let environment = crate::app::data_dir().to_string_lossy().into_owned();
    let journal = Rotation::acquire(
        &crate::app::data_dir().join("credential-recovery"),
        Scope {
            environment: &environment,
            owner: "local",
            provider: "nous",
            alias,
        },
    )?;
    static QUEUE: std::sync::OnceLock<std::sync::Mutex<PendingQueue>> = std::sync::OnceLock::new();
    let mut queue = QUEUE
        .get_or_init(|| std::sync::Mutex::new(PendingQueue::new(64)))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let key = format!(
        "{}:nous/{alias}:{generation}",
        crate::app::data_dir().display()
    );
    if !queue.reserve(key.clone()) {
        return Err("Credential recovery capacity reached; retry later".into());
    }
    let result = token_with_recovery(alias, &key, &mut queue, &journal, &vault);
    queue.release_reservation(&key);
    result
}

fn token_with_recovery(
    alias: &str,
    key: &String,
    queue: &mut PendingQueue,
    journal: &Rotation,
    vault: &accounts::Vault,
) -> Result<String, String> {
    let mut document =
        accounts::read_secret_document("nous", alias)?.ok_or("Nous credential missing")?;
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
                "Previous Nous refresh was interrupted; authorize this account again".into()
            })
        }
        Recovery::Replacement(replacement) => {
            if accounts::read_secret_document("nous", alias)?.as_ref() != Some(&document) {
                return Err("Nous account changed during recovery".into());
            }
            if vault
                .write_secret("nous", alias, &replacement.to_string())
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
        let client = Client::new(None)?;
        journal.begin(&document)?;
        let refreshed = match client.refresh(&document, now) {
            Ok(refreshed) => refreshed,
            Err(error) => return valid_access(&document, now).map_err(|_| error),
        };
        let mut replacement = document.clone();
        replacement
            .as_object_mut()
            .ok_or("Invalid Nous credential")?
            .extend(refreshed.as_object().ok_or("Invalid Nous refresh")?.clone());
        if journal.rotated(&replacement).is_err() {
            let token = valid_access(&replacement, now);
            let expiry = replacement["expires_at_ms"]
                .as_i64()
                .ok_or("Nous expiry missing")?;
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
        if accounts::read_secret_document("nous", alias)?.as_ref() != Some(&document)
            || !vault
                .registry()?
                .accounts
                .iter()
                .any(|a| a.id == format!("nous/{alias}"))
        {
            return Err("Nous account changed during refresh".into());
        }
        if vault
            .write_secret("nous", alias, &replacement.to_string())
            .is_ok()
        {
            journal.complete()?;
        }
        document = replacement;
    }
    valid_access(&document, util::now_ms())
}

fn valid_access(document: &serde_json::Value, now: i64) -> Result<String, String> {
    fabrials_providers::nous::valid_access(document, now)
        .ok_or("Nous authorization unavailable or expired; sign in again".into())
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
