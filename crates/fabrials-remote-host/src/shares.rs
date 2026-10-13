use fabrials_agent_host::server::HostState;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};
use tokio::sync::Mutex;

#[cfg(test)]
#[path = "publication_experiment.rs"]
mod publication_experiment;

const MAX_DOCUMENT_BYTES: usize = 2_000_000;
const MAX_ENTRIES: usize = 10_000;
const MAX_ENTRY_CHARS: usize = 200_000;
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PUBLICATIONS: usize = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ChangeDetection {
    #[default]
    FullExport,
    ContentDigest,
}

impl ChangeDetection {
    fn configured() -> Result<Self, String> {
        match std::env::var("SPANREED_REMOTE_EXPERIMENT_PUBLICATION_DETECTION").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("full-export") => Ok(Self::FullExport),
            Ok("content-digest") => Ok(Self::ContentDigest),
            _ => Err("invalid_experimental_publication_detection".into()),
        }
    }
}

#[derive(Clone, Copy)]
struct SourceFingerprint {
    generation: u64,
    revision: u64,
    digest: [u8; 32],
}

#[derive(Clone, Serialize, Deserialize)]
struct Grant {
    session_id: String,
    generation: u64,
    upload_token: String,
    workspace: PathBuf,
    title: String,
    revision: u64,
    fingerprint: String,
    stopped: bool,
}

impl Drop for Grant {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.upload_token.zeroize();
    }
}

pub struct Shares {
    host: Weak<HostState>,
    _owned_host: Option<Arc<HostState>>,
    pub(crate) publications: Option<Arc<crate::publications::Publications>>,
    pub(crate) direct_mode: bool,
    path: PathBuf,
    client: reqwest::Client,
    grants: Mutex<BTreeMap<String, Grant>>,
    sync_lock: Mutex<()>,
    change_detection: ChangeDetection,
    source_fingerprints: Mutex<BTreeMap<String, SourceFingerprint>>,
    #[cfg(test)]
    preview_only: bool,
}

impl Shares {
    pub(crate) fn direct(
        host: &Arc<HostState>,
        directory: &Path,
        public_key: &str,
    ) -> Result<Self, String> {
        let publications = match crate::publications::Publications::new(host, directory, public_key)
        {
            Ok(service) => Some(Arc::new(service)),
            Err(_) => {
                eprintln!(
                    "Publications are unavailable: repair this computer's private publication state and restart the host."
                );
                None
            }
        };
        Ok(Self {
            host: Arc::downgrade(host),
            _owned_host: None,
            publications,
            direct_mode: true,
            path: directory.join("remote-publications.json"),
            client: crate::http_client()?,
            grants: Mutex::new(BTreeMap::new()),
            sync_lock: Mutex::new(()),
            change_detection: ChangeDetection::FullExport,
            source_fingerprints: Mutex::new(BTreeMap::new()),
            #[cfg(test)]
            preview_only: false,
        })
    }
    pub fn new(
        host: Arc<HostState>,
        directory: PathBuf,
        client: reqwest::Client,
    ) -> Result<Self, String> {
        let path = directory.join("remote-publications.json");
        let grants: BTreeMap<String, Grant> = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.len() > 512 * 1024 {
                    return Err("share_storage_invalid".into());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o077 != 0 {
                        return Err("share_storage_not_private".into());
                    }
                }
                let bytes = zeroize::Zeroizing::new(
                    std::fs::read(&path).map_err(|_| "share_storage_unavailable")?,
                );
                if bytes.len() > 512 * 1024 {
                    return Err("share_storage_invalid".into());
                }
                serde_json::from_slice(&bytes).map_err(|_| "share_storage_invalid")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(_) => {
                return Err("share_storage_invalid".into());
            }
        };
        if grants.len() > MAX_PUBLICATIONS
            || grants.iter().any(|(id, grant)| {
                !opaque(id)
                    || !opaque(&grant.session_id)
                    || grant.generation == 0
                    || (!grant.stopped
                        && (!opaque(&grant.upload_token) || grant.upload_token.len() > 128))
            })
        {
            return Err("share_storage_invalid".into());
        }
        Ok(Self {
            host: Arc::downgrade(&host),
            _owned_host: Some(host),
            publications: None,
            direct_mode: false,
            path,
            client,
            grants: Mutex::new(grants),
            sync_lock: Mutex::new(()),
            change_detection: ChangeDetection::configured()?,
            source_fingerprints: Mutex::new(BTreeMap::new()),
            #[cfg(test)]
            preview_only: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn preview_only(mut self) -> Self {
        self.preview_only = true;
        self
    }

    async fn source(&self, session_id: &str) -> Result<(PathBuf, String), String> {
        if !opaque(session_id) {
            return Err("invalid_session".into());
        }
        let host = self.host.upgrade().ok_or("host_stopped")?;
        let session = host.session.lock().await;
        let live = session.sessions.get(session_id).ok_or("session_not_open")?;
        let workspace = session
            .workspaces
            .get(&live.workspace_id)
            .ok_or("workspace_not_enrolled")?;
        let name = label(&workspace.display_name);
        Ok((
            workspace.path.clone(),
            label(&format!("Grok session · {name}")),
        ))
    }

    pub async fn preview(&self, session_id: &str) -> Result<Value, String> {
        let (workspace, title) = self.source(session_id).await?;
        export(
            &fabrials_agent_host::session_catalog::grok_home(),
            &workspace,
            session_id,
            &title,
        )
    }

    pub async fn start(
        &self,
        session_id: &str,
        share_id: &str,
        generation: u64,
        upload_token: &str,
    ) -> Result<(), String> {
        #[cfg(test)]
        if self.preview_only {
            return Err("publication_unavailable_in_synthetic_fixture".into());
        }
        if !opaque(share_id) || generation == 0 || upload_token.len() > 128 || !opaque(upload_token)
        {
            return Err("invalid_share_grant".into());
        }
        let (workspace, title) = self.source(session_id).await?;
        let document = export(
            &fabrials_agent_host::session_catalog::grok_home(),
            &workspace,
            session_id,
            &title,
        )?;
        ensure_document(&document)?;
        let mut grants = self.grants.lock().await;
        if let Some(existing) = grants.get(share_id) {
            if existing.session_id != session_id
                || existing.generation != generation
                || existing.stopped
                || existing.upload_token != upload_token
            {
                return Err("share_grant_conflict".into());
            }
            return Ok(());
        }
        if grants.len() >= MAX_PUBLICATIONS {
            return Err("publication_history_full".into());
        }
        grants.insert(
            share_id.into(),
            Grant {
                session_id: session_id.into(),
                generation,
                upload_token: upload_token.into(),
                workspace,
                title,
                revision: 0,
                fingerprint: String::new(),
                stopped: false,
            },
        );
        if let Err(error) = persist(&self.path, &grants) {
            grants.remove(share_id);
            return Err(error);
        }
        Ok(())
    }

    pub async fn stop(&self, share_id: &str) -> Result<(), String> {
        let mut grants = self.grants.lock().await;
        let grant = grants.get_mut(share_id).ok_or("unknown_share")?;
        grant.stopped = true;
        use zeroize::Zeroize;
        grant.upload_token.zeroize();
        let result = persist(&self.path, &grants);
        drop(grants);
        self.source_fingerprints.lock().await.remove(share_id);
        result
    }

    pub async fn sync(&self) -> Result<(), String> {
        if self.direct_mode {
            return match &self.publications {
                Some(publications) => publications.sync().await,
                None => Ok(()),
            };
        }
        #[cfg(test)]
        if self.preview_only {
            return Ok(());
        }
        let Ok(_single) = self.sync_lock.try_lock() else {
            return Ok(());
        };
        let pending: Vec<(String, Grant)> = self
            .grants
            .lock()
            .await
            .iter()
            .filter(|(_, grant)| !grant.stopped)
            .map(|(id, grant)| (id.clone(), grant.clone()))
            .collect();
        let mut failed = None;
        for (share_id, previous) in pending {
            if let Err(error) = self.sync_one(&share_id, &previous).await {
                failed = Some(error);
            }
        }
        failed.map_or(Ok(()), Err)
    }

    async fn sync_one(&self, share_id: &str, previous: &Grant) -> Result<(), String> {
        self.sync_one_at(
            &fabrials_agent_host::session_catalog::grok_home(),
            share_id,
            previous,
        )
        .await
    }

    async fn sync_one_at(
        &self,
        home: &Path,
        share_id: &str,
        previous: &Grant,
    ) -> Result<(), String> {
        let Some((document, source_digest)) =
            self.changed_document(home, share_id, previous).await?
        else {
            return Ok(());
        };
        let bytes = ensure_document(&document)?;
        let fingerprint = format!("{:x}", Sha256::digest(&bytes));
        if fingerprint == previous.fingerprint {
            self.remember_source(
                share_id,
                previous.generation,
                previous.revision,
                source_digest,
            )
            .await;
            return Ok(());
        }
        {
            let mut grants = self.grants.lock().await;
            let grant = grants.get_mut(share_id).ok_or("unknown_share")?;
            if grant.stopped
                || grant.generation != previous.generation
                || grant.revision != previous.revision
            {
                return Ok(());
            }
            grant.revision = grant
                .revision
                .checked_add(1)
                .ok_or("share_revision_exhausted")?;
            persist(&self.path, &grants)?;
        }
        let revision = previous.revision + 1;
        let body = json!({"sessionId": previous.session_id, "generation": previous.generation, "revision": revision, "document":document});
        let encoded = serde_json::to_vec(&body).map_err(|_| "share_serialization_failed")?;
        if encoded.len() > 2_010_000 {
            return Err("share_document_too_large".into());
        }
        {
            let grants = self.grants.lock().await;
            if grants.get(share_id).is_none_or(|grant| {
                grant.stopped
                    || grant.generation != previous.generation
                    || grant.revision != revision
            }) {
                return Ok(());
            }
        }
        let response = self
            .client
            .put(format!(
                "https://fabrials.com/api/spanreed/remote/shares/{share_id}/revisions"
            ))
            .bearer_auth(&previous.upload_token)
            .header("content-type", "application/json")
            .body(encoded)
            .send()
            .await
            .map_err(|_| "share_upload_unavailable")?;
        if matches!(response.status().as_u16(), 401 | 403 | 404 | 410) {
            self.stop(share_id).await?;
            return Err("share_grant_revoked".into());
        }
        if !response.status().is_success() {
            return Err("share_upload_rejected".into());
        }
        let mut grants = self.grants.lock().await;
        if let Some(grant) = grants.get_mut(share_id)
            && !grant.stopped
            && grant.generation == previous.generation
            && grant.revision == revision
        {
            grant.fingerprint = fingerprint;
            persist(&self.path, &grants)?;
        }
        drop(grants);
        self.remember_source(share_id, previous.generation, revision, source_digest)
            .await;
        Ok(())
    }

    async fn changed_document(
        &self,
        home: &Path,
        share_id: &str,
        previous: &Grant,
    ) -> Result<Option<(Value, Option<[u8; 32]>)>, String> {
        if self.change_detection == ChangeDetection::FullExport {
            return export(
                home,
                &previous.workspace,
                &previous.session_id,
                &previous.title,
            )
            .map(|document| Some((document, None)));
        }
        let source = open_source(home, &previous.workspace, &previous.session_id)?;
        let mut bytes = Vec::new();
        std::io::Read::take(source, MAX_SOURCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "session_history_unavailable")?;
        if bytes.len() as u64 > MAX_SOURCE_BYTES {
            return Err("share_source_too_large".into());
        }
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if self
            .source_fingerprints
            .lock()
            .await
            .get(share_id)
            .is_some_and(|cached| {
                cached.generation == previous.generation
                    && cached.revision == previous.revision
                    && cached.digest == digest
            })
        {
            return Ok(None);
        }
        project(
            std::io::Cursor::new(bytes),
            &previous.workspace,
            &previous.title,
        )
        .map(|document| Some((document, Some(digest))))
    }

    async fn remember_source(
        &self,
        share_id: &str,
        generation: u64,
        revision: u64,
        digest: Option<[u8; 32]>,
    ) {
        let Some(digest) = digest else {
            return;
        };
        let grants = self.grants.lock().await;
        if grants.get(share_id).is_some_and(|grant| {
            !grant.stopped && grant.generation == generation && grant.revision == revision
        }) {
            self.source_fingerprints.lock().await.insert(
                share_id.into(),
                SourceFingerprint {
                    generation,
                    revision,
                    digest,
                },
            );
        }
    }
}

fn opaque(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn persist(path: &Path, grants: &BTreeMap<String, Grant>) -> Result<(), String> {
    static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), serial));
    let bytes = zeroize::Zeroizing::new(
        serde_json::to_vec(grants).map_err(|_| "share_storage_unavailable")?,
    );
    if bytes.len() > 512 * 1024 {
        return Err("share_storage_invalid".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| "share_storage_unavailable")?;
    let result = (|| {
        #[cfg(windows)]
        fabrials_agent_host::win_acl::set_owner_only(&temporary)
            .map_err(|_| "share_storage_unavailable")?;
        file.write_all(&bytes)
            .map_err(|_| "share_storage_unavailable")?;
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|_| "share_storage_unavailable")?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|_| "share_storage_unavailable")?;
        #[cfg(unix)]
        std::fs::File::open(path.parent().ok_or("share_storage_unavailable")?)
            .and_then(|file| file.sync_all())
            .map_err(|_| "share_storage_unavailable")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub(crate) fn ensure_document(document: &Value) -> Result<Vec<u8>, String> {
    let encoded = serde_json::to_vec(document).map_err(|_| "share_serialization_failed")?;
    if encoded.len() > MAX_DOCUMENT_BYTES {
        return Err("share_document_too_large".into());
    }
    Ok(encoded)
}

fn checked_text(text: &str) -> Result<&str, String> {
    if text.contains('\0') || text.encode_utf16().count() > MAX_ENTRY_CHARS {
        return Err("share_entry_too_large".into());
    }
    Ok(text)
}

pub(crate) fn label(text: &str) -> String {
    let mut units = 0;
    text.chars()
        .filter(|c| !c.is_control())
        .take_while(|c| {
            units += c.len_utf16();
            units <= 190
        })
        .collect::<String>()
}

pub(crate) fn export(
    home: &Path,
    workspace: &Path,
    session_id: &str,
    title: &str,
) -> Result<Value, String> {
    project(
        BufReader::new(open_source(home, workspace, session_id)?),
        workspace,
        title,
    )
}

pub(crate) struct VerifiedExport {
    pub document: Value,
    pub source_length: u64,
    pub source_fingerprint: String,
}

pub(crate) fn export_verified(
    home: &Path,
    workspace: &Path,
    session_id: &str,
    title: &str,
    previous_length: u64,
    previous_fingerprint: &str,
) -> Result<VerifiedExport, String> {
    let mut bytes = Vec::new();
    open_source(home, workspace, session_id)?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "session_history_unavailable")?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err("share_source_too_large".into());
    }
    let complete = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    bytes.truncate(complete);
    if previous_length > bytes.len() as u64
        || previous_length > 0
            && format!("{:x}", Sha256::digest(&bytes[..previous_length as usize]))
                != previous_fingerprint
    {
        return Err("publication_source_changed".into());
    }
    let source_length = bytes.len() as u64;
    let source_fingerprint = format!("{:x}", Sha256::digest(&bytes));
    let document = project(std::io::Cursor::new(bytes), workspace, title)?;
    Ok(VerifiedExport {
        document,
        source_length,
        source_fingerprint,
    })
}

fn open_source(home: &Path, workspace: &Path, session_id: &str) -> Result<std::fs::File, String> {
    if !opaque(session_id) {
        return Err("invalid_session".into());
    }
    let parent =
        home.join("sessions")
            .join(fabrials_agent_host::session_catalog::encode_cwd_dirname(
                &workspace.to_string_lossy(),
            ));
    let directory = parent.join(session_id);
    let source = directory.join("updates.jsonl");
    let source_meta =
        std::fs::symlink_metadata(&source).map_err(|_| "session_history_unavailable")?;
    if source_meta.file_type().is_symlink()
        || !source_meta.is_file()
        || source_meta.len() > MAX_SOURCE_BYTES
        || std::fs::symlink_metadata(&directory)
            .map_err(|_| "session_history_unavailable")?
            .file_type()
            .is_symlink()
        || std::fs::symlink_metadata(&parent)
            .map_err(|_| "session_history_unavailable")?
            .file_type()
            .is_symlink()
    {
        return Err("session_history_unavailable".into());
    }
    std::fs::File::open(source).map_err(|_| "session_history_unavailable".into())
}

fn project(mut input: impl BufRead, workspace: &Path, title: &str) -> Result<Value, String> {
    let mut entries: Vec<Value> = Vec::new();
    let mut names = BTreeMap::<String, String>::new();
    let mut buffer = Vec::new();
    let mut total = 0usize;
    loop {
        buffer.clear();
        let bytes = std::io::Read::take(&mut input, (MAX_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut buffer)
            .map_err(|_| "session_history_unavailable")?;
        if bytes == 0 {
            break;
        }
        if bytes > MAX_LINE_BYTES {
            return Err("share_source_line_too_large".into());
        }
        total += bytes;
        if total > MAX_SOURCE_BYTES as usize {
            return Err("share_source_too_large".into());
        }
        if buffer.last() != Some(&b'\n') {
            break;
        }
        let value: Value =
            serde_json::from_slice(&buffer).map_err(|_| "session_history_invalid")?;
        let Some(update) = value
            .pointer("/params/update")
            .or_else(|| value.get("update"))
        else {
            continue;
        };
        let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) else {
            continue;
        };
        if update
            .pointer("/_meta/hideFromScrollback")
            .and_then(Value::as_bool)
            == Some(true)
        {
            continue;
        }
        match kind {
            "user_message_chunk" | "agent_message_chunk" => {
                if update.pointer("/content/type").and_then(Value::as_str) != Some("text") {
                    continue;
                }
                let Some(text) = update.pointer("/content/text").and_then(Value::as_str) else {
                    continue;
                };
                checked_text(text)?;
                let role = if kind == "user_message_chunk" {
                    "user"
                } else {
                    "assistant"
                };
                if let Some(last) = entries.last_mut().filter(|entry| {
                    entry.get("kind").and_then(Value::as_str) == Some("message")
                        && entry.get("role").and_then(Value::as_str) == Some(role)
                }) {
                    let combined = format!(
                        "{}{text}",
                        last.get("text").and_then(Value::as_str).unwrap_or_default()
                    );
                    checked_text(&combined)?;
                    last["text"] = json!(combined);
                } else {
                    entries.push(json!({"kind":"message", "role":role, "text":text}));
                }
            }
            "tool_call" | "tool_call_update" => {
                let Some(id) = update.get("toolCallId").and_then(Value::as_str) else {
                    continue;
                };
                let name = update
                    .get("title")
                    .and_then(Value::as_str)
                    .map(label)
                    .filter(|name| !name.trim().is_empty())
                    .or_else(|| names.get(id).cloned())
                    .unwrap_or_else(|| "Tool".into());
                names.insert(id.into(), name.clone());
                if kind == "tool_call" {
                    entries.push(json!({"kind":"tool_call", "name":name, "text":name}));
                }
                if let Some(content) = update.get("content").and_then(Value::as_array) {
                    for item in content {
                        match item.get("type").and_then(Value::as_str) {
                            Some("content")
                                if item.pointer("/content/type").and_then(Value::as_str)
                                    == Some("text") =>
                            {
                                if let Some(text) =
                                    item.pointer("/content/text").and_then(Value::as_str)
                                {
                                    entries.push(json!({"kind":"tool_result", "name":name, "text":checked_text(text)?}));
                                }
                            }
                            Some("diff") => {
                                let Some(path) = item.get("path").and_then(Value::as_str) else {
                                    continue;
                                };
                                let path = Path::new(path);
                                let relative = if path.is_absolute() {
                                    path.strip_prefix(workspace).ok()
                                } else {
                                    Some(path)
                                };
                                let Some(relative) = relative.filter(|path| {
                                    path.components()
                                        .all(|part| matches!(part, std::path::Component::Normal(_)))
                                }) else {
                                    continue;
                                };
                                let path = relative.to_string_lossy();
                                if path.encode_utf16().count() > 1024
                                    || path.chars().any(char::is_control)
                                {
                                    return Err("share_diff_path_invalid".into());
                                }
                                let old = item
                                    .get("oldText")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default();
                                let Some(new) = item.get("newText").and_then(Value::as_str) else {
                                    continue;
                                };
                                let text = full_diff(&path, old, new);
                                entries.push(json!({"kind":"diff", "path":path, "text":checked_text(&text)?}));
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
        if entries.len() > MAX_ENTRIES {
            return Err("share_too_many_entries".into());
        }
        if names.len() > MAX_ENTRIES {
            return Err("share_too_many_entries".into());
        }
    }
    let title = label(title);
    let title = if title.trim().is_empty() {
        "Grok session"
    } else {
        &title
    };
    let document = json!({"title": title, "entries":entries});
    ensure_document(&document)?;
    Ok(document)
}

fn full_diff(path: &str, old: &str, new: &str) -> String {
    let mut output = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -1,{} +1,{} @@\n",
        old.lines().count(),
        new.lines().count()
    );
    for line in old.lines() {
        output.push('-');
        output.push_str(line);
        output.push('\n');
    }
    for line in new.lines() {
        output.push('+');
        output.push_str(line);
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(updates: &[Value]) -> Value {
        let input = updates
            .iter()
            .map(|update| format!("{}\n", json!({"params":{"update":update}})))
            .collect::<String>();
        project(
            std::io::Cursor::new(input),
            Path::new("/workspace"),
            "Session",
        )
        .unwrap()
    }

    #[test]
    fn export_allows_only_visible_text_tools_and_relative_diffs() {
        let output = document(&[
            json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hello"}}),
            json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"PRIVATE_REASONING"}}),
            json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"PRIVATE_INTERNAL"},"_meta":{"hideFromScrollback":true}}),
            json!({"sessionUpdate":"system_message_chunk","content":{"type":"text","text":"PRIVATE_SYSTEM"}}),
            json!({"sessionUpdate":"tool_call","toolCallId":"t","title":"Read","rawInput":{"password":"SECRET_CREDENTIAL"},"content":[{"type":"resource","resource":{"text":"PRIVATE_RESOURCE"}},{"type":"content","content":{"type":"text","text":"visible"}},{"type":"diff","path":"/workspace/file.rs","oldText":"a","newText":"b"}]}),
            json!({"sessionUpdate":"agent_message_chunk","content":{"type":"image","data":"PRIVATE_BINARY"}}),
            json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"done"}}),
        ]);
        let wire = output.to_string();
        for private in [
            "PRIVATE_REASONING",
            "PRIVATE_INTERNAL",
            "PRIVATE_SYSTEM",
            "SECRET_CREDENTIAL",
            "PRIVATE_RESOURCE",
            "PRIVATE_BINARY",
        ] {
            assert!(!wire.contains(private));
        }
        assert_eq!(output["entries"].as_array().unwrap().len(), 5);
        assert_eq!(output["entries"][3]["path"], "file.rs");
        assert_eq!(output["entries"][4]["role"], "assistant");
    }

    #[test]
    fn overlong_text_fails_explicitly_and_partial_tail_is_not_published() {
        let text = "a".repeat(MAX_ENTRY_CHARS + 1);
        let line = format!(
            "{}\n",
            json!({"update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":text}}})
        );
        assert_eq!(
            project(std::io::Cursor::new(line), Path::new("/workspace"), "title").unwrap_err(),
            "share_entry_too_large"
        );
        let document = project(
            std::io::Cursor::new("{broken"),
            Path::new("/workspace"),
            "title",
        )
        .unwrap();
        assert!(document["entries"].as_array().unwrap().is_empty());
    }

    #[test]
    fn tombstones_do_not_keep_upload_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shares.json");
        let mut grants = BTreeMap::new();
        grants.insert(
            "share".into(),
            Grant {
                session_id: "session".into(),
                generation: 1,
                upload_token: String::new(),
                workspace: PathBuf::from("/workspace"),
                title: "title".into(),
                revision: 2,
                fingerprint: String::new(),
                stopped: true,
            },
        );
        persist(&path, &grants).unwrap();
        let saved: BTreeMap<String, Grant> =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(saved["share"].stopped);
        assert!(saved["share"].upload_token.is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn unicode_labels_respect_the_public_utf16_limit() {
        let value = label(&"💎".repeat(200));
        assert_eq!(value.encode_utf16().count(), 190);
        assert_eq!(value.chars().count(), 95);
        assert_eq!(label("a\nb\0c"), "abc");
    }

    #[cfg(unix)]
    #[test]
    fn grant_load_refuses_symlinks_and_world_readable_tokens() {
        use std::os::unix::{fs::PermissionsExt, fs::symlink};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("remote-publications.json");
        let target = directory.path().join("target");
        std::fs::write(&target, b"{}").unwrap();
        symlink(&target, &path).unwrap();
        let state = Arc::new(HostState::new(
            fabrials_agent_host::origin::LocalOrigin::new(
                "0123456789abcdef0123456789abcdef",
                21001,
            )
            .unwrap(),
        ));
        assert!(
            Shares::new(
                state.clone(),
                directory.path().into(),
                crate::http_client().unwrap()
            )
            .is_err()
        );
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            Shares::new(
                state.clone(),
                directory.path().into(),
                crate::http_client().unwrap()
            )
            .is_err()
        );
        persist(&path, &BTreeMap::new()).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(
            Shares::new(
                state,
                directory.path().into(),
                crate::http_client().unwrap()
            )
            .is_ok()
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"{}");
    }

    fn source_fixture(home: &Path, text: &str) -> (PathBuf, Grant) {
        let workspace = PathBuf::from("/workspace");
        let directory = home
            .join("sessions")
            .join(fabrials_agent_host::session_catalog::encode_cwd_dirname(
                &workspace.to_string_lossy(),
            ))
            .join("session");
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("updates.jsonl");
        std::fs::write(&source, format!("{}\n", json!({"update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":text}}}))).unwrap();
        let doc = export(home, &workspace, "session", "Session").unwrap();
        (
            source,
            Grant {
                session_id: "session".into(),
                generation: 1,
                upload_token: "token".into(),
                workspace,
                title: "Session".into(),
                revision: 1,
                fingerprint: format!("{:x}", Sha256::digest(ensure_document(&doc).unwrap())),
                stopped: false,
            },
        )
    }

    fn test_publisher(directory: &Path, detection: ChangeDetection) -> Shares {
        let host = Arc::new(HostState::new(
            fabrials_agent_host::origin::LocalOrigin::new(
                "0123456789abcdef0123456789abcdef",
                21001,
            )
            .unwrap(),
        ));
        let mut publisher =
            Shares::new(host, directory.into(), crate::http_client().unwrap()).unwrap();
        publisher.change_detection = detection;
        publisher
    }

    #[tokio::test]
    async fn synthetic_preview_only_fixture_cannot_register_or_send_publications() {
        let directory = tempfile::tempdir().unwrap();
        let (_, grant) = source_fixture(directory.path(), "synthetic fixture");
        let publisher =
            test_publisher(directory.path(), ChangeDetection::ContentDigest).preview_only();
        publisher.grants.lock().await.insert("share".into(), grant);
        assert_eq!(
            publisher
                .start("session", "new-share", 1, "synthetic-token")
                .await
                .unwrap_err(),
            "publication_unavailable_in_synthetic_fixture"
        );
        publisher.sync().await.unwrap();
        let grants = publisher.grants.lock().await;
        assert_eq!(grants.len(), 1);
        assert_eq!(grants["share"].revision, 1);
        assert!(!directory.path().join("remote-publications.json").exists());
    }

    #[tokio::test]
    async fn content_digest_detects_same_size_and_timestamp_rewrites() {
        let directory = tempfile::tempdir().unwrap();
        let (source, grant) = source_fixture(directory.path(), "first");
        let publisher = test_publisher(directory.path(), ChangeDetection::ContentDigest);
        publisher
            .grants
            .lock()
            .await
            .insert("share".into(), grant.clone());
        let (original, digest) = publisher
            .changed_document(directory.path(), "share", &grant)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            original,
            export(
                directory.path(),
                &grant.workspace,
                &grant.session_id,
                &grant.title
            )
            .unwrap()
        );
        publisher
            .remember_source("share", grant.generation, grant.revision, digest)
            .await;
        assert!(
            publisher
                .changed_document(directory.path(), "share", &grant)
                .await
                .unwrap()
                .is_none()
        );
        let metadata = source.metadata().unwrap();
        let input = std::fs::read_to_string(&source).unwrap();
        std::fs::write(&source, input.replace("first", "other")).unwrap();
        std::fs::File::open(&source)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
            .unwrap();
        assert_eq!(source.metadata().unwrap().len(), metadata.len());
        assert_eq!(
            source.metadata().unwrap().modified().unwrap(),
            metadata.modified().unwrap()
        );
        let (changed, _) = publisher
            .changed_document(directory.path(), "share", &grant)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(original, changed);
        assert_eq!(
            changed,
            export(
                directory.path(),
                &grant.workspace,
                &grant.session_id,
                &grant.title
            )
            .unwrap()
        );
    }

    #[tokio::test]
    async fn content_digest_tracks_partial_append_truncation_and_invalid_rewrites() {
        let directory = tempfile::tempdir().unwrap();
        let (source, grant) = source_fixture(directory.path(), "first");
        let publisher = test_publisher(directory.path(), ChangeDetection::ContentDigest);
        publisher
            .grants
            .lock()
            .await
            .insert("share".into(), grant.clone());
        let original_bytes = std::fs::read(&source).unwrap();
        let (original, digest) = publisher
            .changed_document(directory.path(), "share", &grant)
            .await
            .unwrap()
            .unwrap();
        publisher.remember_source("share", 1, 1, digest).await;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&source)
            .unwrap()
            .write_all(b"{\"update\":")
            .unwrap();
        let (partial, partial_digest) = publisher
            .changed_document(directory.path(), "share", &grant)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(partial, original);
        publisher
            .remember_source("share", 1, 1, partial_digest)
            .await;
        assert!(
            publisher
                .changed_document(directory.path(), "share", &grant)
                .await
                .unwrap()
                .is_none()
        );
        std::fs::OpenOptions::new().append(true).open(&source).unwrap().write_all(br#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"complete"}}}
"#).unwrap();
        let (completed, _) = publisher
            .changed_document(directory.path(), "share", &grant)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            completed,
            export(
                directory.path(),
                &grant.workspace,
                &grant.session_id,
                &grant.title
            )
            .unwrap()
        );
        assert_eq!(completed["entries"].as_array().unwrap().len(), 2);
        std::fs::write(&source, &original_bytes).unwrap();
        assert_eq!(
            publisher
                .changed_document(directory.path(), "share", &grant)
                .await
                .unwrap()
                .unwrap()
                .0,
            original
        );
        std::fs::write(&source, b"{invalid}\n").unwrap();
        for _ in 0..2 {
            assert_eq!(
                publisher
                    .changed_document(directory.path(), "share", &grant)
                    .await
                    .unwrap_err(),
                "session_history_invalid"
            );
        }
    }

    #[tokio::test]
    async fn content_cache_is_grant_and_revision_scoped_and_revocation_clears_it() {
        let directory = tempfile::tempdir().unwrap();
        let (_, grant) = source_fixture(directory.path(), "first");
        let publisher = test_publisher(directory.path(), ChangeDetection::ContentDigest);
        publisher
            .grants
            .lock()
            .await
            .insert("share".into(), grant.clone());
        let (_, digest) = publisher
            .changed_document(directory.path(), "share", &grant)
            .await
            .unwrap()
            .unwrap();
        publisher.remember_source("share", 1, 1, digest).await;
        assert!(
            publisher
                .changed_document(directory.path(), "other", &grant)
                .await
                .unwrap()
                .is_some()
        );
        let mut revision = grant.clone();
        revision.revision += 1;
        assert!(
            publisher
                .changed_document(directory.path(), "share", &revision)
                .await
                .unwrap()
                .is_some()
        );
        let mut generation = grant.clone();
        generation.generation += 1;
        assert!(
            publisher
                .changed_document(directory.path(), "share", &generation)
                .await
                .unwrap()
                .is_some()
        );
        publisher.stop("share").await.unwrap();
        assert!(publisher.source_fingerprints.lock().await.is_empty());
        publisher.remember_source("share", 1, 1, digest).await;
        assert!(publisher.source_fingerprints.lock().await.is_empty());
        let reloaded = test_publisher(directory.path(), ChangeDetection::ContentDigest);
        assert!(reloaded.source_fingerprints.lock().await.is_empty());
        assert!(reloaded.grants.lock().await["share"].stopped);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn content_cache_never_bypasses_current_source_path_and_size_validation() {
        let directory = tempfile::tempdir().unwrap();
        let (source, grant) = source_fixture(directory.path(), "first");
        let publisher = test_publisher(directory.path(), ChangeDetection::ContentDigest);
        publisher
            .grants
            .lock()
            .await
            .insert("share".into(), grant.clone());
        let (_, digest) = publisher
            .changed_document(directory.path(), "share", &grant)
            .await
            .unwrap()
            .unwrap();
        publisher.remember_source("share", 1, 1, digest).await;
        let moved = source.with_extension("moved");
        std::fs::rename(&source, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &source).unwrap();
        assert_eq!(
            publisher
                .changed_document(directory.path(), "share", &grant)
                .await
                .unwrap_err(),
            "session_history_unavailable"
        );
        std::fs::remove_file(&source).unwrap();
        assert_eq!(
            publisher
                .changed_document(directory.path(), "share", &grant)
                .await
                .unwrap_err(),
            "session_history_unavailable"
        );
        std::fs::rename(&moved, &source).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&source)
            .unwrap()
            .set_len(MAX_SOURCE_BYTES + 1)
            .unwrap();
        assert_eq!(
            publisher
                .changed_document(directory.path(), "share", &grant)
                .await
                .unwrap_err(),
            "session_history_unavailable"
        );
    }
}
