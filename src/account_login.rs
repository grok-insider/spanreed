//! Native account authorization. Only user codes and opaque flow IDs reach the GUI.
use fabrials_providers::device_flow::{DeviceFlow, PollResult, Progress};
use std::sync::{Mutex, OnceLock};

struct Login {
    id: String,
    account: crate::accounts::Account,
    flow: DeviceFlow,
    expected_removal: Option<String>,
    replace: bool,
    activate_first: bool,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
pub struct LoginView {
    pub id: String,
    pub alias: String,
    pub provider: String,
    pub device: fabrials_providers::device_flow::DeviceView,
}

fn session() -> &'static Mutex<Option<Login>> {
    static SESSION: OnceLock<Mutex<Option<Login>>> = OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(None))
}

pub fn begin_nous(alias: String) -> Result<LoginView, String> {
    begin("nous", alias)
}

pub fn begin(provider: &str, alias: String) -> Result<LoginView, String> {
    begin_inner(provider, alias, false, true)
}

pub fn reauthorize(id: &str) -> Result<LoginView, String> {
    let (provider, alias) = crate::accounts::parse_id(id)?;
    begin_inner(&provider, alias, true, true)
}

pub fn begin_inactive(provider: &str, alias: String) -> Result<LoginView, String> {
    begin_inner(provider, alias, false, false)
}

fn begin_inner(
    provider: &str,
    alias: String,
    replace: bool,
    activate_first: bool,
) -> Result<LoginView, String> {
    begin_reviewed(provider, alias, replace, activate_first, |_| Ok(()))
}

pub(crate) fn begin_reviewed(
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
    let mut session = session()
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
        crate::accounts::Account::new(provider, &alias)?
    };
    before_begin(&account)?;
    let authorization = match provider {
        "grok" => fabrials_providers::grok::device::Client::new()?.begin()?,
        "codex" => fabrials_providers::codex::auth::Client::new()?.begin()?,
        _ => fabrials_providers::nous::Client::new(None)?.begin()?,
    };
    let flow = DeviceFlow::new(authorization, crate::util::now_ms());
    let id = fabrials_runtime::accounting::new_request_id();
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

pub fn poll_nous(id: &str) -> Result<Progress, String> {
    poll(id)
}

pub fn poll(id: &str) -> Result<Progress, String> {
    let mut session = session()
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
                PollResult::Authorized(tokens) => Ok(PollResult::Authorized(
                    crate::drivers::grok::tokens_to_auth_json(&tokens),
                )),
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

pub fn cancel_nous(id: &str) -> Result<(), String> {
    cancel(id)
}

pub fn cancel(id: &str) -> Result<(), String> {
    let mut session = session()
        .lock()
        .map_err(|_| "Authorization state unavailable")?;
    if session.as_ref().is_some_and(|login| login.id == id) {
        *session = None;
    }
    Ok(())
}

pub fn verification_url(id: &str) -> Result<String, String> {
    let session = session()
        .lock()
        .map_err(|_| "Authorization state unavailable")?;
    session
        .as_ref()
        .filter(|login| login.id == id)
        .map(|login| login.flow.view().verification_uri)
        .ok_or("Authorization no longer available".into())
}
