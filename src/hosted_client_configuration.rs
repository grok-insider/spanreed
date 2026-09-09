//! Reviewed hosted client configuration. Only an owner-verified relay key is
//! accepted; upstream OAuth credentials never enter this configuration flow.
use crate::{
    codex_session_move::{client_config, read_regular, session_home},
    remote_workspace::{request, RemoteOperation},
};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Serialize, Clone)]
pub struct HostedClientReview {
    pub id: String,
    pub client: String,
    pub path: String,
    pub account_id: String,
    pub model: String,
    pub endpoint: String,
}
struct Pending {
    view: HostedClientReview,
    owner: String,
    before: Option<String>,
    after: String,
    key: String,
    expires: Instant,
}
fn pending() -> &'static Mutex<Option<Pending>> {
    static VALUE: OnceLock<Mutex<Option<Pending>>> = OnceLock::new();
    VALUE.get_or_init(|| Mutex::new(None))
}
fn account(owner: &str, id: &str, key: &str) -> Result<(), String> {
    let dashboard = request(RemoteOperation::Dashboard, json!({}), Some(7), Some(owner))?;
    if !dashboard["accounts"].as_array().is_some_and(|accounts| {
        accounts
            .iter()
            .any(|a| a["id"] == id && a["provider"] == "codex")
    }) {
        return Err("Refresh and select a hosted Codex account".into());
    }
    let key_hash = format!("{:x}", Sha256::digest(key.as_bytes()));
    if !dashboard["keys"].as_array().is_some_and(|keys| {
        keys.iter()
            .any(|k| k["key_hash"] == key_hash && k["enabled"] == true)
    }) {
        return Err("Use an enabled proxy key belonging to this Fabrials account".into());
    }
    Ok(())
}
fn verify_key(endpoint: &str, key: &str, model: &str) -> Result<(), String> {
    let response = crate::http::Request::get(format!("{endpoint}/models"))
        .bearer(key)
        .send_limited(2 * 1024 * 1024)?;
    if response.status != 200 {
        return Err("The proxy key cannot read this account's models. Review its account, provider, route and model permissions.".into());
    }
    if !response
        .json()
        .and_then(|j| j["data"].as_array().cloned())
        .is_some_and(|models| models.iter().any(|m| m["id"] == model))
    {
        return Err("Choose a model from this account's current catalog".into());
    }
    Ok(())
}
fn opencode(
    before: Option<&str>,
    alias: &str,
    endpoint: &str,
    key: &str,
    model: &str,
) -> Result<String, String> {
    let mut doc = match before {
        Some(text) => crate::setup::jsonc::parse(text)?,
        None => json!({"$schema":"https://opencode.ai/config.json"}),
    };
    let providers = doc
        .as_object_mut()
        .ok_or("OpenCode configuration must be an object")?
        .entry("provider")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("OpenCode providers must be an object")?;
    let name = format!("fabrials-codex-{alias}");
    if providers.contains_key(&name) {
        return Err("This hosted OpenCode connection exists; edit it in OpenCode".into());
    }
    let addition = json!({"npm":"@ai-sdk/openai-compatible","name":format!("Fabrials Codex · {alias}"),"options":{"baseURL":endpoint,"apiKey":key},"models":{model:{"name":model}}});
    providers.insert(name.clone(), addition.clone());
    match before {
        Some(text) => String::from_utf8(crate::setup::jsonc::edit(text, &name, &addition, &doc)?)
            .map_err(|_| "Invalid OpenCode output".into()),
        None => serde_json::to_string_pretty(&doc).map_err(|_| "Invalid OpenCode output".into()),
    }
}
fn opencode_path() -> Result<PathBuf, String> {
    let path = crate::creds::env("OPENCODE_CONFIG")
        .map(|p| crate::creds::expand(&p))
        .unwrap_or_else(|| crate::creds::config_home().join("opencode/opencode.json"));
    let other = path.with_extension(if path.extension().is_some_and(|e| e == "jsonc") {
        "json"
    } else {
        "jsonc"
    });
    if path.exists() && other.exists() {
        return Err(
            "Both JSON and JSONC configurations exist; choose one in OpenCode first".into(),
        );
    }
    Ok(if !path.exists() && other.exists() {
        other
    } else {
        path
    })
}
pub fn preview(
    owner: &str,
    client: &str,
    alias: &str,
    key: &str,
    model: &str,
) -> Result<HostedClientReview, String> {
    if !matches!(client, "codex" | "opencode")
        || !crate::accounts::valid_alias(alias)
        || alias.len() > 40
        || model.trim() != model
        || model.is_empty()
        || model.len() > 256
        || model.chars().any(char::is_control)
        || !(16..=512).contains(&key.len())
        || key.chars().any(char::is_whitespace)
        || key.starts_with("eyJ")
    {
        return Err("Choose a client, hosted account, model and ai-relay proxy key".into());
    }
    let id = format!("codex/{alias}");
    account(owner, &id, key)?;
    let endpoint = format!("https://ai.fabrials.com/acct/{alias}/codex/v1");
    verify_key(&endpoint, key, model)?;
    let path = if client == "codex" {
        session_home().join("config.toml")
    } else {
        opencode_path()?
    };
    let before = read_regular(&path)?;
    let after = if client == "codex" {
        let text = client_config(before.as_deref().unwrap_or(""), alias, key)?;
        let mut doc = text
            .parse::<toml_edit::Document>()
            .map_err(|_| "Invalid Codex configuration")?;
        doc["model"] = toml_edit::value(model);
        doc.to_string()
    } else {
        opencode(before.as_deref(), alias, &endpoint, key, model)?
    };
    let view = HostedClientReview {
        id: fabrials_runtime::accounting::new_request_id(),
        client: client.into(),
        path: path.to_string_lossy().into_owned(),
        account_id: id,
        model: model.into(),
        endpoint,
    };
    *pending()
        .lock()
        .map_err(|_| "Configuration review unavailable")? = Some(Pending {
        view: view.clone(),
        owner: owner.into(),
        before,
        after,
        key: key.into(),
        expires: Instant::now() + Duration::from_secs(300),
    });
    Ok(view)
}
pub fn apply(owner: &str, id: &str) -> Result<String, String> {
    let mut slot = pending()
        .lock()
        .map_err(|_| "Configuration review unavailable")?;
    if slot
        .as_ref()
        .is_none_or(|p| p.view.id != id || p.owner != owner || p.expires < Instant::now())
    {
        return Err("Review expired or account changed; preview again".into());
    }
    let plan = slot.take().ok_or("Review unavailable")?;
    account(owner, &plan.view.account_id, &plan.key)?;
    verify_key(&plan.view.endpoint, &plan.key, &plan.view.model)?;
    let path = PathBuf::from(&plan.view.path);
    if plan.view.client == "opencode" && opencode_path()? != path {
        return Err("OpenCode configuration path changed; preview again".into());
    }
    if plan.view.client == "codex" && session_home().join("config.toml") != path {
        return Err("Codex home changed; preview again".into());
    }
    let _lock = fabrials_runtime::file_set::FileSet::acquire_wait(
        path.parent().ok_or("Invalid client path")?,
    )?;
    if read_regular(&path)? != plan.before {
        return Err("Client configuration changed; preview again".into());
    }
    if let Some(before) = &plan.before {
        fabrials_runtime::files::atomic_write_private(
            &crate::app::data_dir()
                .join("backups")
                .join(format!("{}-{}.config", plan.view.client, id)),
            before.as_bytes(),
        )
        .map_err(|_| "Could not back up client configuration")?;
    }
    if read_regular(&path)? != plan.before {
        return Err("Client configuration changed during backup; preview again".into());
    }
    fabrials_runtime::files::atomic_write_private(&path, plan.after.as_bytes())
        .map_err(|_| "Could not save client configuration")?;
    Ok(if plan.view.client == "codex" {
        "Configuration saved. Start a new Codex session; profile, project or command-line overrides may take precedence.".into()
    } else {
        format!(
            "Connection saved. Restart OpenCode and select fabrials-codex-{}/{} in /models.",
            plan.view.account_id.trim_start_matches("codex/"),
            plan.view.model
        )
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hosted_opencode_preserves_comments_default_model_and_unrelated_providers() {
        let original="{\n// keep this comment\n\"model\":\"other/model\",\"provider\":{\"other\":{\"name\":\"Other\"}}}";
        let after = opencode(
            Some(original),
            "work",
            "https://ai.fabrials.com/acct/work/codex/v1",
            "fixture-key",
            "fixture-model",
        )
        .unwrap();
        assert!(after.contains("// keep this comment"));
        let doc = crate::setup::jsonc::parse(&after).unwrap();
        assert_eq!(doc["model"], "other/model");
        assert_eq!(doc["provider"]["other"]["name"], "Other");
        assert!(opencode(Some(&after), "work", "fixture", "fixture", "model").is_err());
    }
}
