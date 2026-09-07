//! Local Grok credential access with complete-document rotation recovery.
use fabrials_core::recovery::{PendingCredential, RecoveryQueue};
use fabrials_runtime::credential_journal::{Recovery, Rotation, Scope};
use serde_json::Value;
use std::io::Read;
use std::sync::{Mutex, OnceLock};

struct FormHttp;
impl fabrials_oauth_grok::TokenHttp for FormHttp {
    fn post_form(&self, url: &str, body: &str) -> Result<(u16, String), String> {
        let response = crate::http::Request::post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()?;
        Ok((response.status, response.body))
    }
}

fn read(alias: Option<&str>) -> Result<Option<Value>, String> {
    let bytes = match alias {
        Some(alias) => return crate::accounts::read_secret_document("grok", alias),
        None => match std::fs::File::open(crate::creds::expand("~/.grok/auth.json")) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "Grok authorization unavailable")?;
                Some(bytes)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("Grok authorization unavailable".into()),
        },
    };
    bytes
        .map(|bytes| {
            if bytes.len() > 1024 * 1024 {
                return Err("Grok authorization too large".into());
            }
            serde_json::from_slice(&bytes).map_err(|_| "Invalid Grok authorization".into())
        })
        .transpose()
}

fn save(alias: Option<&str>, value: &Value, vault: &crate::accounts::Vault) -> Result<(), String> {
    let text = serde_json::to_string(value).map_err(|_| "Invalid authorization")?;
    match alias {
        Some(alias) => vault.write_secret("grok", alias, &text),
        None => fabrials_runtime::files::atomic_write_private(
            &crate::creds::expand("~/.grok/auth.json"),
            text.as_bytes(),
        )
        .map_err(|_| "Could not persist Grok authorization".into()),
    }
}

pub fn grok(alias: Option<&str>) -> Result<Option<String>, String> {
    let vault = crate::accounts::lock_vault()?;
    let generation = if let Some(alias) = alias {
        vault
            .registry()?
            .accounts
            .into_iter()
            .find(|account| account.id == format!("grok/{alias}"))
            .ok_or("Grok account no longer exists")?
            .generation
            .unwrap_or_default()
    } else {
        String::new()
    };
    if read(alias)?.is_none() {
        return if alias.is_none() {
            Ok(None)
        } else {
            Err("Grok credential missing".into())
        };
    }
    let environment = crate::app::data_dir().to_string_lossy().into_owned();
    let journal = Rotation::acquire(
        &crate::app::data_dir().join("credential-recovery"),
        Scope {
            environment: &environment,
            owner: "local",
            provider: "grok",
            alias: alias.unwrap_or("~/.grok/auth.json"),
        },
    )?;
    // Keep a RAM fallback as well if the durable journal itself becomes unwritable
    // after redemption. Its durable in-flight marker still prevents replay on restart.
    static QUEUE: OnceLock<Mutex<RecoveryQueue<String, Value>>> = OnceLock::new();
    let mut queue = QUEUE
        .get_or_init(|| Mutex::new(RecoveryQueue::new(64)))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let key = format!(
        "{environment}:grok:{}:{generation}",
        alias.unwrap_or("~/.grok/auth.json")
    );
    if !queue.reserve(key.clone()) {
        return Err("Credential recovery capacity reached".into());
    }
    let result = (|| {
        let mut document = read(alias)?.ok_or("Grok credential missing")?;
        if let Some(pending) = queue.take_reserved(&key) {
            if document == pending.expected && journal.rotated(&pending.replacement).is_err() {
                let token = offline_token(&pending.replacement);
                queue.insert(key.clone(), pending, crate::util::now_ms())?;
                return token
                    .map(Some)
                    .ok_or("Credential persistence unavailable".into());
            }
        }
        match journal.recover(&document)? {
            Recovery::Clean => {}
            Recovery::Interrupted => {
                return offline_token(&document).map(Some).ok_or(
                    "Previous Grok refresh was interrupted; authorize this account again".into(),
                )
            }
            Recovery::Replacement(replacement) => {
                if read(alias)?.as_ref() != Some(&document) {
                    return Err("Grok account changed during recovery".into());
                }
                if save(alias, &replacement, &vault).is_err() {
                    return offline_token(&replacement)
                        .map(Some)
                        .ok_or("Grok credential persistence unavailable".into());
                }
                journal.complete()?;
                document = replacement;
            }
        }
        let expected = document.clone();
        let token = fabrials_oauth_grok::ensure_access_token(
            &mut document,
            crate::util::now_ms(),
            &JournaledHttp {
                journal: &journal,
                expected: &expected,
            },
        )
        .ok_or("Grok login required")?;
        if document != expected {
            if journal.rotated(&document).is_err() {
                queue.insert(
                    key.clone(),
                    PendingCredential {
                        expected,
                        replacement: document,
                        expires_at_ms: 0,
                    },
                    crate::util::now_ms(),
                )?;
                return Ok(Some(token));
            }
            if read(alias)?.as_ref() != Some(&expected) {
                return Err("Grok account changed during refresh".into());
            }
            if save(alias, &document, &vault).is_ok() {
                journal.complete()?;
            }
        }
        Ok(Some(token))
    })();
    queue.release_reservation(&key);
    result
}

fn offline_token(document: &Value) -> Option<String> {
    fabrials_oauth_grok::ensure_access_token(
        &mut document.clone(),
        crate::util::now_ms(),
        &OfflineHttp,
    )
}
struct JournaledHttp<'a> {
    journal: &'a Rotation,
    expected: &'a Value,
}
impl fabrials_oauth_grok::TokenHttp for JournaledHttp<'_> {
    fn post_form(&self, url: &str, body: &str) -> Result<(u16, String), String> {
        self.journal.begin(self.expected)?;
        fabrials_oauth_grok::TokenHttp::post_form(&FormHttp, url, body)
    }
}

struct OfflineHttp;
impl fabrials_oauth_grok::TokenHttp for OfflineHttp {
    fn post_form(&self, _: &str, _: &str) -> Result<(u16, String), String> {
        Err("Persistence retry only".into())
    }
}
