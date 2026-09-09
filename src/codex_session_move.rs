//! Exclusive handoff of a file-backed Codex grant. The server cancellation
//! receipt, never a timeout or an absent receipt, authorizes local restoration.
use crate::remote_workspace::{request, RemoteOperation};
use fabrials_runtime::{file_set::FileSet, files::atomic_write_private};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

mod processes;

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Serialize, Deserialize, Clone)]
pub struct SessionMoveView {
    pub id: String,
    pub state: String,
    pub alias: String,
    pub source_path: String,
    pub config_path: String,
    pub endpoint: String,
    pub account_id: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    view: SessionMoveView,
    owner: String,
    original_auth: Option<String>,
    original_config: Option<String>,
    replacement_config: String,
    document: Option<Value>,
    key_hash: String,
}
fn root() -> PathBuf {
    crate::app::data_dir().join("codex-session-move")
}
pub(crate) fn probe_lock() -> Result<FileSet, String> {
    FileSet::acquire_wait(&root())
}
pub(crate) fn retired() -> bool {
    root().join("retired.json").exists()
}
fn journal_path() -> PathBuf {
    root().join("session.json")
}
fn save_at(storage: &Path, journal: &Journal) -> Result<(), String> {
    atomic_write_private(
        &storage.join("session.json"),
        &serde_json::to_vec(journal).map_err(|_| "Invalid session recovery record")?,
    )
    .map_err(|_| "Could not save session recovery record".into())
}
fn save(journal: &Journal) -> Result<(), String> {
    save_at(&root(), journal)
}
fn load(owner: &str, id: Option<&str>) -> Result<Journal, String> {
    let journal: Journal =
        serde_json::from_str(&read_regular(&journal_path())?.ok_or("No saved session move")?)
            .map_err(|_| "Invalid session recovery record")?;
    if journal.owner != owner || id.is_some_and(|id| id != journal.view.id) {
        return Err("Refresh the connected account and session move before continuing".into());
    }
    Ok(journal)
}
pub(crate) fn read_regular(path: &Path) -> Result<Option<String>, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() && metadata.len() <= 1024 * 1024 => {
            std::fs::read_to_string(path)
                .map(Some)
                .map_err(|_| "Could not read client file".into())
        }
        Ok(_) => Err(
            "Client file is managed, linked, or too large; configure it through its owner".into(),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Could not inspect client file".into()),
    }
}
fn write(path: &Path, text: &str) -> Result<(), String> {
    atomic_write_private(path, text.as_bytes())
        .map_err(|_| "Could not write client file; recover this session move".into())
}
fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("Could not retire client file; recover this session move".into()),
    }
    #[cfg(unix)]
    std::fs::File::open(path.parent().ok_or("Invalid client path")?)
        .and_then(|f| f.sync_all())
        .map_err(|_| "Could not persist client file removal")?;
    Ok(())
}
pub(crate) fn client_config(original: &str, alias: &str, key: &str) -> Result<String, String> {
    let mut doc = original
        .parse::<toml_edit::Document>()
        .map_err(|_| "Fix Codex config.toml before moving the session")?;
    if doc.get("profile").is_some() {
        return Err("Remove the default Codex profile override before moving the session".into());
    }
    let name = format!("fabrials_{alias}");
    if doc
        .get("model_providers")
        .and_then(|p| p.get(&name))
        .is_some()
    {
        return Err(
            "Codex provider configuration already exists; choose another account name".into(),
        );
    }
    doc["model_provider"] = toml_edit::value(&name);
    let mut provider = toml_edit::Table::new();
    provider["name"] = toml_edit::value("Fabrials Codex");
    provider["base_url"] =
        toml_edit::value(format!("https://ai.fabrials.com/acct/{alias}/codex/v1"));
    provider["wire_api"] = toml_edit::value("responses");
    provider["experimental_bearer_token"] = toml_edit::value(key);
    provider["requires_openai_auth"] = toml_edit::value(false);
    provider["supports_websockets"] = toml_edit::value(true);
    if doc.get("model_providers").is_none() {
        doc["model_providers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let providers = doc["model_providers"]
        .as_table_mut()
        .ok_or("Codex model_providers must be a TOML table")?;
    providers[&name] = toml_edit::Item::Table(provider);
    Ok(doc.to_string())
}
pub(crate) fn session_home() -> PathBuf {
    crate::creds::env("CODEX_HOME")
        .map(|p| crate::creds::expand(&p))
        .unwrap_or_else(|| crate::creds::expand("~/.codex"))
}
fn exclusive_source(source: &Path) -> Result<(), String> {
    processes::ensure_idle()?;
    processes::ensure_no_keyring()?;
    for path in [
        crate::creds::expand("~/.codex/auth.json"),
        crate::creds::config_home().join("codex/auth.json"),
    ] {
        if path != source && read_regular(&path)?.is_some() {
            return Err(
                "Multiple Codex credential files exist; use independent hosted authorization"
                    .into(),
            );
        }
    }
    // A second managed grant may have been copied from the CLI. Without a
    // verifiable grant lineage it cannot safely remain an independent refresher.
    if !crate::accounts::list_provider("codex").is_empty() {
        return Err(
            "Managed local Codex authorizations exist; use independent hosted authorization".into(),
        );
    }
    for path in [
        crate::creds::data_home().join("opencode/auth.json"),
        crate::creds::config_home().join("opencode/auth.json"),
    ] {
        if let Some(raw) = read_regular(&path)? {
            let data: Value = serde_json::from_str(&raw)
                .map_err(|_| "Could not inspect OpenCode authorizations")?;
            if data.as_object().is_none()
                || data.as_object().is_some_and(|m| {
                    m.iter().any(|(k, v)| {
                        matches!(k.as_str(), "openai" | "codex") && v["type"] == "oauth"
                    })
                })
            {
                return Err("OpenCode has an OAuth authorization that may share this grant; use independent hosted authorization".into());
            }
        }
    }
    Ok(())
}
pub fn current(owner: &str) -> Result<Option<SessionMoveView>, String> {
    let _lock = probe_lock()?;
    if !journal_path().exists() {
        return Ok(None);
    }
    Ok(Some(load(owner, None)?.view))
}
pub fn preview(owner: &str, alias: &str) -> Result<SessionMoveView, String> {
    let _lock = probe_lock()?;
    if journal_path().exists() {
        return Err("Recover or cancel the saved session move before starting another".into());
    }
    if !fabrials_accounts::valid_alias(alias) || alias.len() > 40 {
        return Err("Choose a valid account name of at most 40 characters".into());
    }
    let capabilities = request(
        RemoteOperation::SyncCapabilities,
        json!({}),
        None,
        Some(owner),
    )?;
    if capabilities["codex_session_transfer_v1"] != true {
        return Err(
            "This relay does not support recoverable session moves; use independent authorization"
                .into(),
        );
    }
    let home = session_home();
    let source = home.join("auth.json");
    let config = home.join("config.toml");
    exclusive_source(&source)?;
    let original_auth = read_regular(&source)?
        .ok_or("No file-backed Codex session; use independent hosted authorization")?;
    let auth: Value =
        serde_json::from_str(&original_auth).map_err(|_| "Invalid Codex authorization file")?;
    let document = auth
        .get("tokens")
        .cloned()
        .ok_or("Codex OAuth session unavailable")?;
    if !document["refresh_token"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
    {
        return Err("No renewable Codex session; use independent hosted authorization".into());
    }
    fabrials_providers::codex::auth::token_document(document.clone(), crate::util::now_ms(), None)?;
    let original_config = read_regular(&config)?;
    let mut random = [0u8; 32];
    getrandom::getrandom(&mut random)
        .map_err(|_| "Secure randomness unavailable; session was not moved")?;
    let key = format!(
        "spanreed_{}",
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let key_hash = format!("{:x}", Sha256::digest(key.as_bytes()));
    let replacement_config = client_config(original_config.as_deref().unwrap_or(""), alias, &key)?;
    let view = SessionMoveView {
        id: fabrials_runtime::accounting::new_request_id(),
        state: "prepared".into(),
        alias: alias.into(),
        source_path: source.to_string_lossy().into_owned(),
        config_path: config.to_string_lossy().into_owned(),
        endpoint: format!("https://ai.fabrials.com/acct/{alias}/codex/v1"),
        account_id: None,
    };
    save(&Journal {
        view: view.clone(),
        owner: owner.into(),
        original_auth: Some(original_auth),
        original_config,
        replacement_config,
        document: Some(document),
        key_hash,
    })?;
    Ok(view)
}
#[cfg(target_os = "windows")]
fn quarantine_path(journal: &Journal) -> PathBuf {
    Path::new(&journal.view.source_path)
        .with_file_name(format!(".spanreed-retired-{}.json", journal.view.id))
}
#[cfg(target_os = "windows")]
fn retire_source(journal: &Journal) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = Path::new(&journal.view.source_path)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let target: Vec<u16> = quarantine_path(journal)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // WRITE_THROUGH retires the discoverable name before the hosted import.
    // The quarantine file is never a credential source and is removed on resolution.
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0x8) } == 0 {
        return Err("Could not durably retire Codex authorization; recover the saved move".into());
    }
    Ok(())
}
#[cfg(not(target_os = "windows"))]
fn retire_source(journal: &Journal) -> Result<(), String> {
    remove(Path::new(&journal.view.source_path))
}
fn clear_quarantine(_journal: &Journal) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    remove(&quarantine_path(_journal))?;
    Ok(())
}
fn retire(storage: &Path, journal: &mut Journal) -> Result<(), String> {
    let source = Path::new(&journal.view.source_path);
    let config = Path::new(&journal.view.config_path);
    if read_regular(source)? != journal.original_auth
        || read_regular(config)? != journal.original_config
    {
        return Err("Client files changed after review; cancel and preview again".into());
    }
    journal.view.state = "retiring".into();
    save_at(storage, journal)?;
    write(
        &storage.join("retired.json"),
        &json!({"id":journal.view.id}).to_string(),
    )?;
    write(config, &journal.replacement_config)?;
    retire_source(journal)?;
    journal.view.state = "retired".into();
    save_at(storage, journal)
}
fn completed(storage: &Path, journal: &mut Journal, response: &Value) -> Result<(), String> {
    let account = response["account_id"]
        .as_str()
        .filter(|id| *id == format!("codex/{}", journal.view.alias))
        .ok_or("Invalid completed session receipt; keep the source retired")?;
    journal.view.state = "completed".into();
    journal.view.account_id = Some(account.into());
    // Once ownership is durable on the server no rollback may resurrect OAuth.
    journal.original_auth = None;
    journal.document = None;
    journal.original_config = None;
    save_at(storage, journal)?;
    clear_quarantine(journal)
}
fn restore(storage: &Path, journal: &mut Journal) -> Result<(), String> {
    let source = Path::new(&journal.view.source_path);
    let config = Path::new(&journal.view.config_path);
    let auth = read_regular(source)?;
    let current_config = read_regular(config)?;
    if auth.is_some() && auth != journal.original_auth {
        return Err("Codex authorization changed; restoration requires manual recovery".into());
    }
    if current_config != journal.original_config
        && current_config.as_deref() != Some(journal.replacement_config.as_str())
    {
        return Err("Codex configuration changed; restoration requires manual recovery".into());
    }
    if let Some(original) = &journal.original_config {
        write(config, original)?;
    } else {
        remove(config)?;
    }
    if let Some(original) = &journal.original_auth {
        write(source, original)?;
    }
    clear_quarantine(journal)?;
    remove(&storage.join("retired.json"))?;
    journal.view.state = "cancelled".into();
    journal.document = None;
    journal.original_auth = None;
    save_at(storage, journal)
}
pub fn apply(owner: &str, id: &str) -> Result<SessionMoveView, String> {
    let _lock = probe_lock()?;
    let mut journal = load(owner, Some(id))?;
    if journal.view.state != "prepared" {
        return Err("Use recovery for this saved session move".into());
    }
    exclusive_source(Path::new(&journal.view.source_path))?;
    // Validate the installation session before touching local files.
    request(
        RemoteOperation::SyncCapabilities,
        json!({}),
        None,
        Some(owner),
    )?;
    retire(&root(), &mut journal)?;
    let response = request(
        RemoteOperation::ImportCodexSession,
        json!({"id":id,"alias":journal.view.alias,"document":journal.document,"virtual_key_hash":journal.key_hash,"source_retired":true}),
        None,
        Some(owner),
    );
    match response {
        Ok(value) if value["state"]=="completed"=>completed(&root(),&mut journal,&value)?,
        Ok(value) if value["state"]=="cancelled"=>restore(&root(),&mut journal)?,
        _=>return Err("Session retired locally; recover the saved move to confirm hosted ownership or safely restore it".into()),
    }
    Ok(journal.view)
}
pub fn recover(owner: &str, id: &str, cancel: bool) -> Result<SessionMoveView, String> {
    let _lock = probe_lock()?;
    let mut journal = load(owner, Some(id))?;
    if matches!(journal.view.state.as_str(), "completed" | "cancelled") {
        clear_quarantine(&journal)?;
        return Ok(journal.view);
    }
    let operation = if cancel {
        RemoteOperation::CancelCodexSession
    } else {
        RemoteOperation::CodexSessionStatus
    };
    let response = request(operation, json!({"id":id}), None, Some(owner))?;
    match response["state"].as_str() {
        Some("completed")=>completed(&root(),&mut journal,&response)?,
        Some("cancelled")=>{processes::ensure_idle()?;restore(&root(),&mut journal)?;},
        Some("absent")=>return Err("No receipt yet. Cancel and restore to fence any delayed import; keep the local session retired until confirmation".into()),
        _=>return Err("Invalid recovery receipt; keep the local session retired".into()),
    }
    Ok(journal.view)
}
pub fn dismiss(owner: &str, id: &str) -> Result<(), String> {
    let _lock = probe_lock()?;
    let journal = load(owner, Some(id))?;
    if !matches!(journal.view.state.as_str(), "completed" | "cancelled") {
        return Err("Resolve this session move before dismissing its receipt".into());
    }
    clear_quarantine(&journal)?;
    remove(&journal_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, Journal) {
        let storage = std::env::temp_dir().join(format!(
            "spanreed-move-fixture-{}",
            fabrials_runtime::accounting::new_request_id()
        ));
        std::fs::create_dir_all(&storage).unwrap();
        let source = storage.join("auth.json");
        let config = storage.join("config.toml");
        write(&source, "fixture-oauth").unwrap();
        write(&config, "fixture-original-config").unwrap();
        let journal = Journal {
            view: SessionMoveView {
                id: "fixture".into(),
                state: "prepared".into(),
                alias: "fixture".into(),
                source_path: source.to_string_lossy().into_owned(),
                config_path: config.to_string_lossy().into_owned(),
                endpoint: "fixture".into(),
                account_id: None,
            },
            owner: "fixture".into(),
            original_auth: Some("fixture-oauth".into()),
            original_config: Some("fixture-original-config".into()),
            replacement_config: "fixture-proxy-config".into(),
            document: Some(json!({"access_token":"fixture"})),
            key_hash: "fixture".into(),
        };
        (storage, journal)
    }
    #[test]
    fn interrupted_retirement_can_restore_but_completed_receipt_discards_oauth() {
        let (storage, mut journal) = fixture();
        retire(&storage, &mut journal).unwrap();
        assert!(!Path::new(&journal.view.source_path).exists());
        assert!(storage.join("retired.json").exists());
        // Simulate a restart after source retirement; all originals are durable.
        let mut recovered: Journal =
            serde_json::from_str(&std::fs::read_to_string(storage.join("session.json")).unwrap())
                .unwrap();
        restore(&storage, &mut recovered).unwrap();
        assert_eq!(
            read_regular(Path::new(&recovered.view.source_path))
                .unwrap()
                .as_deref(),
            Some("fixture-oauth")
        );
        assert_eq!(
            read_regular(Path::new(&recovered.view.config_path))
                .unwrap()
                .as_deref(),
            Some("fixture-original-config")
        );
        assert!(!storage.join("retired.json").exists());
        let (_, mut next) = fixture();
        completed(&storage, &mut next, &json!({"account_id":"codex/fixture"})).unwrap();
        let persisted = std::fs::read_to_string(storage.join("session.json")).unwrap();
        assert!(!persisted.contains("fixture-oauth"));
        assert!(next.document.is_none());
        std::fs::remove_dir_all(Path::new(&next.view.source_path).parent().unwrap()).unwrap();
        std::fs::remove_dir_all(storage).unwrap();
    }
    #[test]
    fn file_changes_prevent_retirement_or_rollback_overwrite() {
        let (storage, mut journal) = fixture();
        let config = PathBuf::from(&journal.view.config_path);
        write(&config, "user-change").unwrap();
        assert!(retire(&storage, &mut journal).is_err());
        assert_eq!(
            read_regular(Path::new(&journal.view.source_path)).unwrap(),
            journal.original_auth
        );
        write(&config, "fixture-original-config").unwrap();
        retire(&storage, &mut journal).unwrap();
        write(&config, "later-user-change").unwrap();
        assert!(restore(&storage, &mut journal).is_err());
        assert_eq!(
            read_regular(&config).unwrap().as_deref(),
            Some("later-user-change")
        );
        assert!(!Path::new(&journal.view.source_path).exists());
        std::fs::remove_dir_all(storage).unwrap();
    }
    #[test]
    fn configuration_preserves_unrelated_settings_and_uses_only_virtual_key() {
        let text = client_config(
            "# keep\nmodel = 'fixture-model'\n[projects.abc]\ntrust_level = 'trusted'\n",
            "fixture",
            "fixture-key",
        )
        .unwrap();
        assert!(text.contains("# keep"));
        let doc: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(doc["model"].as_str(), Some("fixture-model"));
        assert_eq!(
            doc["model_providers"]["fabrials_fixture"]["requires_openai_auth"].as_bool(),
            Some(false)
        );
        assert_eq!(
            doc["model_providers"]["fabrials_fixture"]["experimental_bearer_token"].as_str(),
            Some("fixture-key")
        );
        assert!(client_config(&text, "fixture", "other").is_err());
        assert!(client_config("profile='other'", "fixture", "key").is_err());
    }
}
