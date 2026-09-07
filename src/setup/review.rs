//! Reviewed client configuration writes; full source documents stay in the host.
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub warnings: Vec<String>,
    pub id: String,
    pub path: String,
    pub provider_id: String,
    #[cfg_attr(feature = "contracts", ts(type = "unknown"))]
    pub addition: Value,
    pub client: ConfigurationClient,
    pub operation: Operation,
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigurationClient {
    Opencode,
    Grok,
}
impl ConfigurationClient {
    fn as_str(self) -> &'static str {
        match self {
            Self::Opencode => "opencode",
            Self::Grok => "grok",
        }
    }
}
struct Pending {
    view: Preview,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
    path: PathBuf,
    expires: Instant,
    context: Option<(String, Option<String>, String)>,
}
fn pending() -> &'static Mutex<Option<Pending>> {
    static PENDING: OnceLock<Mutex<Option<Pending>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(None))
}
fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => return Err(
            "Configuration must be a regular file; edit managed or linked configurations manually"
                .into(),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot inspect client configuration".into()),
        _ => {}
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "Cannot open client configuration")?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read client configuration")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Client configuration exceeds 2 MiB".into());
    }
    Ok(Some(bytes))
}
#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Create,
    Update,
    Remove,
}
fn refers_to(value: &Value, prefix: &str, allowed: Option<&str>) -> bool {
    match value {
        Value::String(text) => text.starts_with(prefix) && Some(text.as_str()) != allowed,
        Value::Array(values) => values.iter().any(|value| refers_to(value, prefix, allowed)),
        Value::Object(values) => values
            .values()
            .any(|value| refers_to(value, prefix, allowed)),
        _ => false,
    }
}
fn prepare(
    path: PathBuf,
    bind: &str,
    provider: &str,
    alias: &str,
    model: &str,
) -> Result<Pending, String> {
    prepare_change(path, bind, provider, alias, model, Operation::Create)
}
fn prepare_change(
    path: PathBuf,
    bind: &str,
    provider: &str,
    alias: &str,
    model: &str,
    operation: Operation,
) -> Result<Pending, String> {
    let address: std::net::SocketAddr = bind.parse().map_err(|_| "Invalid proxy address")?;
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err("Use a nonzero loopback proxy port".into());
    }
    if !matches!(provider, "grok" | "nous" | "openai") || !crate::accounts::valid_alias(alias) {
        return Err("Select a managed provider account".into());
    }
    if operation != Operation::Remove
        && (model.trim() != model
            || model.is_empty()
            || model.len() > 256
            || model.chars().any(char::is_control))
    {
        return Err("Enter a valid model ID".into());
    }
    let path = if path.extension().is_some_and(|ext| ext == "json")
        && path.with_extension("jsonc").exists()
    {
        if path.exists() {
            return Err(
                "Both JSON and JSONC configurations exist; choose one in OpenCode first".into(),
            );
        }
        path.with_extension("jsonc")
    } else {
        path
    };
    let before = read(&path)?;
    let mut root: Value = match &before {
        Some(bytes) => super::jsonc::parse(
            std::str::from_utf8(bytes).map_err(|_| "Configuration is not UTF-8")?,
        )?,
        None => json!({"$schema":"https://opencode.ai/config.json"}),
    };
    let object = root
        .as_object_mut()
        .ok_or("Client configuration must be an object")?;
    let provider_id = format!("spanreed-{provider}-{alias}");
    let prefix = format!("{provider_id}/");
    let allowed = format!("{prefix}{model}");
    if operation != Operation::Create
        && object
            .iter()
            .filter(|(key, _)| key.as_str() != "provider")
            .any(|(_, value)| {
                refers_to(
                    value,
                    &prefix,
                    if operation == Operation::Update {
                        Some(&allowed)
                    } else {
                        None
                    },
                )
            })
    {
        return Err(
            "Change model references to this connection before removing or replacing its models"
                .into(),
        );
    }
    let providers = object
        .entry("provider")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("Client provider configuration must be an object")?;
    match (operation, providers.get(&provider_id)) {
        (Operation::Create, Some(_)) => {
            return Err("This connection exists; choose Update existing connection".into())
        }
        (Operation::Update | Operation::Remove, None) => {
            return Err("Connection not found; choose Create connection".into())
        }
        (Operation::Update | Operation::Remove, Some(existing)) => {
            validate_managed_connection(existing, provider, alias)?
        }
        _ => {}
    }
    let route = if provider == "grok" {
        String::new()
    } else {
        format!("/{provider}")
    };
    let addition = json!({"npm":"@ai-sdk/openai-compatible", "name":format!("Spanreed · {provider}/{alias}"), "options":{"baseURL":format!("http://{address}/acct/{alias}{route}/v1"),"apiKey":"spanreed-local"}, "models":{model:{"name":model}}});
    let addition = if operation == Operation::Remove {
        providers.remove(&provider_id);
        Value::Null
    } else {
        providers.insert(provider_id.clone(), addition.clone());
        addition
    };
    let after = if let Some(bytes) = before.as_ref() {
        super::jsonc::edit(
            std::str::from_utf8(bytes).map_err(|_| "Configuration is not UTF-8")?,
            &provider_id,
            &addition,
            &root,
        )?
    } else {
        let mut bytes =
            serde_json::to_vec_pretty(&root).map_err(|_| "Cannot encode client configuration")?;
        bytes.push(b'\n');
        bytes
    };
    if after.len() as u64 > MAX_BYTES {
        return Err("Resulting client configuration exceeds 2 MiB".into());
    }
    Ok(Pending {
        view: Preview {
            id: fabrials_runtime::accounting::new_request_id(),
            path: path.display().to_string(),
            provider_id,
            addition,
            warnings: Vec::new(),
            client: ConfigurationClient::Opencode,
            operation,
        },
        before,
        after,
        path,
        expires: Instant::now() + Duration::from_secs(300),
        context: None,
    })
}
fn validate_managed_connection(value: &Value, provider: &str, alias: &str) -> Result<(), String> {
    let invalid = "Connection contains custom settings; edit it in OpenCode to preserve them";
    let object = value.as_object().ok_or(invalid)?;
    if object.len() != 4
        || value["npm"] != "@ai-sdk/openai-compatible"
        || value["name"] != format!("Spanreed · {provider}/{alias}")
    {
        return Err(invalid.into());
    }
    let options = value["options"].as_object().ok_or(invalid)?;
    if options.len() != 2 || value["options"]["apiKey"] != "spanreed-local" {
        return Err(invalid.into());
    }
    let url = reqwest::Url::parse(value["options"]["baseURL"].as_str().ok_or(invalid)?)
        .map_err(|_| invalid)?;
    let route = if provider == "grok" {
        String::new()
    } else {
        format!("/{provider}")
    };
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .is_none_or(|ip| !ip.is_loopback())
        || url.port_or_known_default() == Some(0)
        || url.path() != format!("/acct/{alias}{route}/v1")
    {
        return Err(invalid.into());
    }
    let models = value["models"].as_object().ok_or(invalid)?;
    if models.is_empty()
        || models.iter().any(|(id, metadata)| {
            metadata.as_object().is_none_or(|fields| fields.len() != 1) || metadata["name"] != *id
        })
    {
        return Err(invalid.into());
    }
    Ok(())
}
fn prepare_grok(path: PathBuf, bind: &str, alias: &str) -> Result<Pending, String> {
    let address: std::net::SocketAddr = bind.parse().map_err(|_| "Invalid proxy address")?;
    if !address.ip().is_loopback() || address.port() == 0 || !crate::accounts::valid_alias(alias) {
        return Err("Select a managed account and a nonzero loopback proxy port".into());
    }
    let before = read(&path)?;
    let text = before.as_deref().unwrap_or_default();
    let mut document = std::str::from_utf8(text)
        .map_err(|_| "Grok configuration is not UTF-8")?
        .parse::<toml_edit::Document>()
        .map_err(|_| "Invalid Grok TOML configuration; no changes made")?;
    if document.get("endpoints").is_none() {
        document["endpoints"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let endpoints = document["endpoints"]
        .as_table_like_mut()
        .ok_or("Grok endpoints must be a TOML table")?;
    let endpoint = format!("http://{address}/acct/{alias}/v1");
    if let Some(value) = endpoints.get("cli_chat_proxy_base_url") {
        if value.as_str().is_none() {
            return Err("Grok chat endpoint must be a string".into());
        }
        if value.as_str() == Some(endpoint.as_str()) {
            return Err("Grok Build is already configured for this connection".into());
        }
    }
    let mut value = toml_edit::Value::from(endpoint.clone());
    if let Some(previous) = endpoints
        .get("cli_chat_proxy_base_url")
        .and_then(toml_edit::Item::as_value)
    {
        *value.decor_mut() = previous.decor().clone();
    }
    if let Some(existing) = endpoints.get_mut("cli_chat_proxy_base_url") {
        *existing = toml_edit::Item::Value(value);
    } else {
        endpoints.insert("cli_chat_proxy_base_url", toml_edit::Item::Value(value));
    }
    let after = document.to_string().into_bytes();
    if after.len() as u64 > MAX_BYTES {
        return Err("Resulting client configuration exceeds 2 MiB".into());
    }
    Ok(Pending {
        view: Preview {
            id: fabrials_runtime::accounting::new_request_id(),
            path: path.display().to_string(),
            provider_id: format!("grok/{alias}"),
            addition: json!({"endpoints":{"cli_chat_proxy_base_url":endpoint}}),
            warnings: super::grok_environment::warnings(),
            client: ConfigurationClient::Grok,
            operation: Operation::Update,
        },
        before,
        after,
        path,
        expires: Instant::now() + Duration::from_secs(300),
        context: None,
    })
}
pub fn preview_grok(alias: &str) -> Result<Preview, String> {
    let status = crate::desktop_runtime::status()?;
    if status.state != crate::desktop_runtime::ProxyState::Running {
        return Err("Start the local proxy before configuring a client".into());
    }
    let account =
        crate::accounts::get(&format!("grok/{alias}")).ok_or("Account no longer exists")?;
    let home = std::env::var_os("GROK_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::creds::expand("~/.grok"));
    let mut plan = prepare_grok(home.join("config.toml"), &status.bind, alias)?;
    plan.context = Some((account.id, account.generation, status.bind));
    let view = plan.view.clone();
    *pending()
        .lock()
        .map_err(|_| "Configuration review unavailable")? = Some(plan);
    Ok(view)
}
pub fn preview_opencode(provider: &str, alias: &str, model: &str) -> Result<Preview, String> {
    preview_opencode_change(provider, alias, model, false)
}
pub fn preview_opencode_change(
    provider: &str,
    alias: &str,
    model: &str,
    update: bool,
) -> Result<Preview, String> {
    preview_operation(
        provider,
        alias,
        model,
        if update {
            Operation::Update
        } else {
            Operation::Create
        },
    )
}
pub fn preview_opencode_remove(provider: &str, alias: &str) -> Result<Preview, String> {
    preview_operation(provider, alias, "", Operation::Remove)
}
fn preview_operation(
    provider: &str,
    alias: &str,
    model: &str,
    operation: Operation,
) -> Result<Preview, String> {
    let status = crate::desktop_runtime::status()?;
    if status.state != crate::desktop_runtime::ProxyState::Running {
        return Err("Start the local proxy before configuring a client".into());
    }
    let account =
        crate::accounts::get(&format!("{provider}/{alias}")).ok_or("Account no longer exists")?;
    let hit = super::detect::scan().opencode;
    let path = super::detect::opencode_config_write_path(&hit);
    let mut plan = if operation == Operation::Create {
        prepare(path, &status.bind, provider, alias, model)?
    } else {
        prepare_change(path, &status.bind, provider, alias, model, operation)?
    };
    plan.context = Some((account.id, account.generation, status.bind));
    let view = plan.view.clone();
    *pending()
        .lock()
        .map_err(|_| "Configuration review unavailable")? = Some(plan);
    Ok(view)
}
fn apply(plan: Pending, backup_dir: &Path) -> Result<Option<String>, String> {
    if plan.view.client == ConfigurationClient::Opencode {
        let sibling = if plan.path.extension().is_some_and(|ext| ext == "jsonc") {
            plan.path.with_extension("json")
        } else {
            plan.path.with_extension("jsonc")
        };
        if sibling.exists() {
            return Err(
                "Another OpenCode configuration appeared; review the active configuration again"
                    .into(),
            );
        }
    }
    if plan.expires < Instant::now() {
        return Err("Review expired; preview the changes again".into());
    }
    let parent = plan
        .path
        .parent()
        .ok_or("Invalid client configuration path")?;
    let _lease = fabrials_runtime::file_set::FileSet::acquire_wait(parent)?;
    if read(&plan.path)? != plan.before {
        return Err("Configuration changed since review; preview it again".into());
    }
    let backup = if let Some(bytes) = &plan.before {
        let extension = if plan.view.client == ConfigurationClient::Grok {
            "toml"
        } else {
            "json"
        };
        let path = backup_dir.join(format!(
            "{}-{}.{}",
            plan.view.client.as_str(),
            plan.view.id,
            extension
        ));
        fabrials_runtime::files::atomic_write_private(&path, bytes)
            .map_err(|_| "Cannot back up client configuration")?;
        Some(path.display().to_string())
    } else {
        None
    };
    if read(&plan.path)? != plan.before {
        return Err("Configuration changed during backup; preview it again".into());
    }
    fabrials_runtime::files::atomic_write_private(&plan.path, &plan.after)
        .map_err(|_| "Cannot save client configuration")?;
    Ok(backup)
}
pub fn apply_configuration(id: &str) -> Result<Option<String>, String> {
    let mut slot = pending()
        .lock()
        .map_err(|_| "Configuration review unavailable")?;
    if slot.as_ref().is_none_or(|plan| plan.view.id != id) {
        return Err("Review unavailable; preview the changes again".into());
    }
    let plan = slot.take().ok_or("Review unavailable")?;
    let (account_id, generation, bind) =
        plan.context.as_ref().ok_or("Review context unavailable")?;
    let account = crate::accounts::get(account_id).ok_or("Account removed; preview again")?;
    let status = crate::desktop_runtime::status()?;
    if account.generation != *generation
        || status.state != crate::desktop_runtime::ProxyState::Running
        || status.bind != *bind
    {
        return Err("Account or proxy changed; preview the changes again".into());
    }
    apply(plan, &super::paths::backup_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires an explicit OpenCode executable; uses isolated synthetic configuration"]
    fn installed_opencode_accepts_reviewed_jsonc_lifecycle() {
        let executable = std::env::var_os("SPANREED_OPENCODE_TEST_BIN")
            .expect("Set SPANREED_OPENCODE_TEST_BIN to an absolute executable path");
        assert!(Path::new(&executable).is_absolute());
        let root = std::env::temp_dir().join(format!(
            "spanreed-opencode-client-{}",
            fabrials_runtime::accounting::new_request_id()
        ));
        let config = root.join("config/opencode");
        std::fs::create_dir_all(&config).unwrap();
        let path = config.join("opencode.jsonc");
        std::fs::write(&path, "{\n // keep user preferences\n \"model\": \"other/kept\",\n \"provider\": {\"other\": {\"name\": \"Unrelated\"}},\n}\n").unwrap();
        for operation in [Operation::Create, Operation::Update, Operation::Remove] {
            let model = if operation == Operation::Create {
                "first"
            } else {
                "second"
            };
            let plan = prepare_change(
                path.clone(),
                "127.0.0.1:18736",
                "nous",
                "work",
                model,
                operation,
            )
            .unwrap();
            apply(plan, &root.join("backups")).unwrap();
            let output = std::process::Command::new(&executable)
                .args(["--pure", "debug", "config"])
                .env_clear()
                .env("PATH", "/run/current-system/sw/bin:/usr/bin:/bin")
                .env("HOME", &root)
                .env("XDG_CONFIG_HOME", root.join("config"))
                .env("XDG_DATA_HOME", root.join("data"))
                .env("XDG_CACHE_HOME", root.join("cache"))
                .env("XDG_STATE_HOME", root.join("state"))
                .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
                .env("OPENCODE_DISABLE_MODELS_FETCH", "1")
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let resolved: Value = serde_json::from_slice(&output.stdout)
                .expect("OpenCode debug config must emit JSON");
            assert_eq!(resolved["model"], "other/kept");
            assert_eq!(resolved["provider"]["other"]["name"], "Unrelated");
            let connection = &resolved["provider"]["spanreed-nous-work"];
            if operation == Operation::Remove {
                assert!(connection.is_null());
            } else {
                assert_eq!(connection["models"][model]["name"], model);
                assert_eq!(connection["options"]["apiKey"], "spanreed-local");
                assert_eq!(connection["npm"], "@ai-sdk/openai-compatible");
                assert_eq!(
                    connection["options"]["baseURL"],
                    "http://127.0.0.1:18736/acct/work/nous/v1"
                );
            }
            assert!(std::fs::read_to_string(&path)
                .unwrap()
                .contains("// keep user preferences"));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn jsonc_edits_preserve_surrounding_comments_and_exact_backup() {
        let root = std::env::temp_dir().join(format!(
            "spanreed-jsonc-review-{}",
            fabrials_runtime::accounting::new_request_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("opencode.jsonc");
        let original="{\n  // preferred model\n  \"model\": \"other/model\", // keep inline\n  \"provider\": {\n    /* unrelated */ \"other\": {\"name\": \"Keep\"},\n  },\n}\n";
        std::fs::write(&path, original).unwrap();
        let create = prepare(path.clone(), "127.0.0.1:18736", "nous", "work", "first").unwrap();
        let backup = apply(create, &root.join("backups")).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(backup).unwrap(), original);
        for operation in [Operation::Update, Operation::Remove] {
            let plan = prepare_change(
                path.clone(),
                "127.0.0.1:18737",
                "nous",
                "work",
                "second",
                operation,
            )
            .unwrap();
            apply(plan, &root.join("backups")).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            for comment in ["// preferred model", "// keep inline", "/* unrelated */"] {
                assert!(text.contains(comment));
            }
            let value = super::super::jsonc::parse(&text).unwrap();
            assert_eq!(value["model"], "other/model");
            assert_eq!(value["provider"]["other"]["name"], "Keep");
        }
        assert!(super::super::jsonc::parse("{a:1}").is_err());
        assert!(super::super::jsonc::parse("{\"a\":1 \"b\":2}").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn removal_rejects_referenced_models_and_preserves_other_providers() {
        let root = std::env::temp_dir().join(format!(
            "spanreed-remove-review-{}",
            fabrials_runtime::accounting::new_request_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("opencode.json");
        let plan = prepare(path.clone(), "127.0.0.1:18736", "grok", "work", "old-model").unwrap();
        apply(plan, &root.join("backups")).unwrap();
        let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        document["agent"] = json!({"build":{"model":"spanreed-grok-work/old-model"}});
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(prepare_change(
            path.clone(),
            "127.0.0.1:18736",
            "grok",
            "work",
            "",
            Operation::Remove
        )
        .is_err());
        assert!(prepare_change(
            path.clone(),
            "127.0.0.1:18736",
            "grok",
            "work",
            "new-model",
            Operation::Update
        )
        .is_err());
        assert!(prepare_change(
            path.clone(),
            "127.0.0.1:18737",
            "grok",
            "work",
            "old-model",
            Operation::Update
        )
        .is_ok());
        document["agent"]["build"]["model"] = json!("other/model");
        document["provider"]["other"] = json!({"name":"Keep"});
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        let before = std::fs::read(&path).unwrap();
        let removal = prepare_change(
            path.clone(),
            "127.0.0.1:18736",
            "grok",
            "work",
            "",
            Operation::Remove,
        )
        .unwrap();
        assert_eq!(removal.view.operation, Operation::Remove);
        assert!(removal.view.addition.is_null());
        let backup = apply(removal, &root.join("backups")).unwrap().unwrap();
        assert_eq!(std::fs::read(backup).unwrap(), before);
        document["provider"]
            .as_object_mut()
            .unwrap()
            .remove("spanreed-grok-work");
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(path).unwrap()).unwrap(),
            document
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn update_managed_connection_preserves_other_settings_and_rejects_customization() {
        let root = std::env::temp_dir().join(format!(
            "spanreed-update-review-{}",
            fabrials_runtime::accounting::new_request_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("opencode.json");
        std::fs::write(&path,serde_json::to_vec(&json!({"model":"other/default","provider":{"other":{"options":{"apiKey":"private-unrelated"}}}})).unwrap()).unwrap();
        let create = prepare(path.clone(), "127.0.0.1:18736", "nous", "work", "old-model").unwrap();
        apply(create, &root.join("backups")).unwrap();
        let before = std::fs::read(&path).unwrap();
        let update = prepare_change(
            path.clone(),
            "[::1]:18737",
            "nous",
            "work",
            "new-model",
            Operation::Update,
        )
        .unwrap();
        assert_eq!(update.view.operation, Operation::Update);
        assert!(!serde_json::to_string(&update.view)
            .unwrap()
            .contains("private-unrelated"));
        let backup = apply(update, &root.join("backups")).unwrap().unwrap();
        assert_eq!(std::fs::read(backup).unwrap(), before);
        let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(document["model"], "other/default");
        assert_eq!(
            document["provider"]["other"]["options"]["apiKey"],
            "private-unrelated"
        );
        assert_eq!(
            document["provider"]["spanreed-nous-work"]["options"]["baseURL"],
            "http://[::1]:18737/acct/work/nous/v1"
        );
        assert_eq!(
            document["provider"]["spanreed-nous-work"]["models"],
            json!({"new-model":{"name":"new-model"}})
        );
        document["provider"]["spanreed-nous-work"]["options"]["headers"] = json!({"custom":"keep"});
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(prepare_change(
            path.clone(),
            "127.0.0.1:18736",
            "nous",
            "work",
            "model",
            Operation::Update
        )
        .is_err());
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap(),
            document
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn grok_review_preserves_comments_models_and_private_backup() {
        let root = std::env::temp_dir().join(format!(
            "spanreed-grok-review-{}",
            fabrials_runtime::accounting::new_request_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let original = "# user configuration\n[models]\ndefault = \"keep-model\" # keep model\n[endpoints]\n# endpoint comment\ncli_chat_proxy_base_url = \"https://example.invalid/v1\" # preserve this\ndeployment_key = \"synthetic-secret\"\n";
        std::fs::write(&path, original).unwrap();
        let plan = prepare_grok(path.clone(), "[::1]:18736", "work").unwrap();
        assert_eq!(plan.view.client, ConfigurationClient::Grok);
        assert!(!serde_json::to_string(&plan.view)
            .unwrap()
            .contains("synthetic-secret"));
        let backup = apply(plan, &root.join("backups")).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("# user configuration"));
        assert!(written.contains("# preserve this"));
        assert!(written.contains("# endpoint comment"));
        let document: toml::Value = toml::from_str(&written).unwrap();
        assert_eq!(document["models"]["default"].as_str(), Some("keep-model"));
        assert_eq!(
            document["endpoints"]["deployment_key"].as_str(),
            Some("synthetic-secret")
        );
        assert_eq!(
            document["endpoints"]["cli_chat_proxy_base_url"].as_str(),
            Some("http://[::1]:18736/acct/work/v1")
        );
        assert!(prepare_grok(path.clone(), "[::1]:18736", "work").is_err());
        std::fs::write(&path, "endpoints = 42").unwrap();
        assert!(prepare_grok(path.clone(), "127.0.0.1:18736", "work").is_err());
        std::fs::write(&path, "[invalid").unwrap();
        assert!(prepare_grok(path, "127.0.0.1:18736", "work").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_expired_reviews_invalid_routes_and_non_json_configuration() {
        let root = std::env::temp_dir().join(format!(
            "spanreed-review-guards-{}",
            fabrials_runtime::accounting::new_request_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("opencode.json");
        for (bind, provider, alias, model) in [
            ("0.0.0.0:18736", "grok", "work", "model"),
            ("127.0.0.1:0", "grok", "work", "model"),
            ("127.0.0.1:18736", "unknown", "work", "model"),
            ("127.0.0.1:18736", "grok", "../work", "model"),
            ("127.0.0.1:18736", "grok", "work", ""),
        ] {
            assert!(prepare(path.clone(), bind, provider, alias, model).is_err());
        }
        let mut plan = prepare(path.clone(), "[::1]:18736", "nous", "work", "model").unwrap();
        assert_eq!(
            plan.view.addition["options"]["baseURL"],
            "http://[::1]:18736/acct/work/nous/v1"
        );
        plan.expires = Instant::now() - Duration::from_secs(1);
        assert!(apply(plan, &root.join("backups")).is_err());
        assert!(!path.exists());
        std::fs::write(&path, b"{ malformed").unwrap();
        assert!(prepare(path.clone(), "127.0.0.1:18736", "grok", "work", "model").is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::write(path.with_extension("jsonc"), b"{ /* keep comments */ }").unwrap();
        let jsonc = prepare(path.clone(), "127.0.0.1:18736", "grok", "work", "model").unwrap();
        assert_eq!(jsonc.path, path.with_extension("jsonc"));
        assert!(String::from_utf8(jsonc.after)
            .unwrap()
            .contains("/* keep comments */"));
        assert!(!path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reviewed_write_preserves_settings_and_rejects_changed_source() {
        let root = std::env::temp_dir().join(format!(
            "spanreed-review-{}",
            fabrials_runtime::accounting::new_request_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("opencode.json");
        let original = br#"{"plugin":["keep"],"model":"original/model","provider":{"other":{"options":{"apiKey":"synthetic-secret"}}}}"#;
        std::fs::write(&path, original).unwrap();
        let plan = prepare(
            path.clone(),
            "127.0.0.1:18736",
            "grok",
            "work",
            "chosen-model",
        )
        .unwrap();
        assert!(!serde_json::to_string(&plan.view)
            .unwrap()
            .contains("synthetic-secret"));
        std::fs::write(&path, b"{}").unwrap();
        assert!(apply(plan, &root.join("backups")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{}");
        std::fs::write(&path, original).unwrap();
        let plan = prepare(
            path.clone(),
            "127.0.0.1:18736",
            "grok",
            "work",
            "chosen-model",
        )
        .unwrap();
        let backup = apply(plan, &root.join("backups")).unwrap().unwrap();
        assert_eq!(std::fs::read(backup).unwrap(), original);
        let result: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(result["model"], "original/model");
        assert_eq!(result["plugin"], json!(["keep"]));
        assert_eq!(
            result["provider"]["other"]["options"]["apiKey"],
            "synthetic-secret"
        );
        assert_eq!(
            result["provider"]["spanreed-grok-work"]["models"]["chosen-model"]["name"],
            "chosen-model"
        );
        assert!(prepare(path, "127.0.0.1:18736", "grok", "work", "chosen-model").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
