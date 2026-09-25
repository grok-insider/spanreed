//! Native account authorization. Only user codes and opaque flow IDs reach the GUI.
use fabrials_providers::device_flow::{DeviceFlow, PollResult, Progress};
use std::sync::Mutex;

struct Login {
    id: String,
    account: crate::accounts::Account,
    flow: DeviceFlow,
    expected_removal: Option<String>,
    replace: bool,
    activate_first: bool,
}

pub use spanreed_app::app::accounts::LoginView;
/// The one device authorization in progress. Owned by `AppContext`.
#[derive(Default)]
pub struct Logins {
    session: Mutex<Option<Login>>,
}

impl Logins {
    pub fn begin_nous(&self, alias: String) -> Result<LoginView, String> {
        self.begin("nous", alias)
    }

    pub fn begin(&self, provider: &str, alias: String) -> Result<LoginView, String> {
        self.begin_inner(provider, alias, false, true)
    }

    pub fn reauthorize(&self, id: &str) -> Result<LoginView, String> {
        let (provider, alias) = crate::accounts::parse_id(id)?;
        self.begin_inner(&provider, alias, true, true)
    }

    pub fn begin_inactive(&self, provider: &str, alias: String) -> Result<LoginView, String> {
        self.begin_inner(provider, alias, false, false)
    }

    fn begin_inner(
        &self,
        provider: &str,
        alias: String,
        replace: bool,
        activate_first: bool,
    ) -> Result<LoginView, String> {
        self.begin_reviewed(provider, alias, replace, activate_first, |_| Ok(()))
    }

    pub(crate) fn begin_reviewed(
        &self,
        provider: &str,
        alias: String,
        replace: bool,
        activate_first: bool,
        before_begin: impl FnOnce(&crate::accounts::Account) -> Result<(), String>,
    ) -> Result<LoginView, String> {
        if !matches!(provider, "nous" | "grok" | "codex") {
            return Err("Unsupported authorization provider".into());
        }
        if alias.len() > 128 || !crate::accounts::valid_alias(&alias) {
            return Err("Use letters, numbers, underscores or hyphens for the account name".into());
        }
        let mut session = self
            .session
            .lock()
            .map_err(|_| "Authorization state unavailable")?;
        if session
            .as_ref()
            .is_some_and(|login| !login.flow.expired(crate::util::now_ms()))
        {
            return Err("Finish or cancel the current authorization first".into());
        }
        let registry = crate::accounts::routing_registry()?;
        let account = if replace {
            registry
                .accounts
                .iter()
                .find(|account| account.id == format!("{provider}/{alias}"))
                .cloned()
                .ok_or("Account no longer exists")?
        } else {
            if registry
                .accounts
                .iter()
                .any(|account| account.alias == alias || account.aliases.contains(&alias))
            {
                return Err("That account name is already in use".into());
            }
            crate::accounts::new_account(provider, &alias)?
        };
        before_begin(&account)?;
        let authorization = match provider {
            "grok" => fabrials_providers::grok::device::Client::new()?.begin()?,
            "codex" => fabrials_providers::codex::auth::Client::new()?.begin()?,
            _ => fabrials_providers::nous::Client::new(None)?.begin()?,
        };
        let flow = DeviceFlow::new(authorization, crate::util::now_ms());
        let id = fabrials_fabric::accounting::new_request_id();
        let view = LoginView {
            id: id.clone(),
            alias: alias.clone(),
            provider: provider.into(),
            device: flow.view(),
        };
        let expected_removal = registry.removed.get(&account.id).cloned();
        *session = Some(Login {
            id,
            account,
            flow,
            expected_removal,
            replace,
            activate_first,
        });
        Ok(view)
    }

    pub fn poll_nous(&self, id: &str) -> Result<Progress, String> {
        self.poll(id)
    }

    pub fn poll(&self, id: &str) -> Result<Progress, String> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| "Authorization state unavailable")?;
        let login = session
            .as_mut()
            .filter(|login| login.id == id)
            .ok_or("Authorization no longer available")?;
        let now = crate::util::now_ms();
        let result = login.flow.advance(
            now,
            |code| match login.account.provider.as_str() {
                "grok" => match fabrials_providers::grok::device::Client::new()?.poll(code)? {
                    PollResult::Authorized(tokens) => {
                        Ok(PollResult::Authorized(grok_tokens_to_auth_json(&tokens)))
                    }
                    result => Ok(result),
                },
                "codex" => fabrials_providers::codex::auth::Client::new()?.poll(code, now),
                _ => fabrials_providers::nous::Client::new(None)?.poll(code, now),
            },
            |document| {
                if login.replace {
                    crate::accounts::replace_authorization(
                        &login.account,
                        &login.id,
                        &document.to_string(),
                    )
                } else {
                    crate::accounts::lock_vault()?.register_with_activation(
                        login.account.clone(),
                        &document.to_string(),
                        login.expected_removal.as_ref(),
                        login.activate_first,
                    )
                }
            },
        );
        if matches!(result, Ok(Progress::Connected)) {
            *session = None;
        }
        result
    }

    pub fn cancel_nous(&self, id: &str) -> Result<(), String> {
        self.cancel(id)
    }

    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| "Authorization state unavailable")?;
        if session.as_ref().is_some_and(|login| login.id == id) {
            *session = None;
        }
        Ok(())
    }

    pub fn verification_url(&self, id: &str) -> Result<String, String> {
        let session = self
            .session
            .lock()
            .map_err(|_| "Authorization state unavailable")?;
        session
            .as_ref()
            .filter(|login| login.id == id)
            .map(|login| login.flow.view().verification_uri)
            .ok_or("Authorization no longer available".into())
    }
}

pub(crate) const GROK_AUTH_JSON_ENTRY: &str =
    "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828";

/// Grok CLI `auth.json` document for freshly issued device-flow tokens.
pub(crate) fn grok_tokens_to_auth_json(tok: &serde_json::Value) -> serde_json::Value {
    let access = tok
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let refresh = tok.get("refresh_token").and_then(|v| v.as_str());
    let now = crate::util::now_ms();
    let expires_at = tok
        .get("expires_in")
        .and_then(|v| v.as_f64())
        .filter(|n| *n > 0.0)
        .map(|n| now.saturating_add((n.min(86400.0) as i64) * 1000))
        .or_else(|| crate::util::jwt_exp_ms(access))
        .unwrap_or(now + 3600 * 1000);
    let iso = crate::util::ms_to_iso(expires_at).unwrap_or_default();
    let mut entry = serde_json::json!({
        "key": access,
        "expires_at": iso,
        "oidc_client_id": fabrials_providers::grok::device::CLIENT_ID,
    });
    if let Some(rt) = refresh {
        entry["refresh_token"] = serde_json::json!(rt);
    }
    serde_json::json!({ GROK_AUTH_JSON_ENTRY: entry })
}
