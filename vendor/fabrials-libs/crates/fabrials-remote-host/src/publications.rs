use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fabrials_agent_host::{receipts::Receipts, remote::RemoteAccess, server::HostState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};
use tokio::sync::Mutex;
use zeroize::Zeroize;

const API: &str = "https://publications.fabrials.com/v1";
const EXCHANGE: &str = "https://fabrials.com/api/spanreed/publications/exchange";
const FILE: &str = "publications-v2.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Authorization {
    grant: String,
    expires_at: u64,
}
impl Drop for Authorization {
    fn drop(&mut self) {
        self.grant.zeroize();
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pending {
    principal: String,
    verifier: String,
    nonce: String,
    expires_at: u64,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.verifier.zeroize();
        self.nonce.zeroize();
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PendingUpload {
    revision: u64,
    request_digest: String,
    fingerprint: String,
    entry_hashes: Vec<String>,
    ordinals: Vec<u64>,
    high_water: u64,
    source_length: u64,
    source_fingerprint: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Publication {
    session_id: String,
    workspace: PathBuf,
    title: String,
    upload_token: String,
    #[serde(default)]
    revoke_token: String,
    #[serde(default)]
    generation: String,
    #[serde(default)]
    pending_upload: Option<PendingUpload>,
    phase: String,
    expires_at: u64,
    revision: u64,
    fingerprint: String,
    entry_hashes: Vec<String>,
    ordinals: Vec<u64>,
    high_water: u64,
    source_length: u64,
    source_fingerprint: String,
    image_hashes: Vec<String>,
    next_attempt: u64,
    delete_retry_ms: u64,
    error: Option<String>,
}
impl Drop for Publication {
    fn drop(&mut self) {
        self.upload_token.zeroize();
        self.revoke_token.zeroize();
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    authorization: Option<Authorization>,
    pending: Option<Pending>,
    publications: BTreeMap<String, Publication>,
}

pub struct Publications {
    host: Weak<HostState>,
    directory: PathBuf,
    public_key: String,
    api: String,
    exchange: String,
    web_origin: String,
    client: reqwest::Client,
    state: Mutex<State>,
    receipts: Mutex<Receipts>,
    operation: Mutex<()>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Operation {
    #[serde(rename_all = "camelCase")]
    PreviewPublication {
        session_id: String,
        #[serde(default)]
        image_hashes: Vec<String>,
    },
    BeginPublicationAuthorization,
    CompletePublicationAuthorization {
        code: String,
        nonce: String,
    },
    #[serde(rename_all = "camelCase")]
    CreatePublication {
        session_id: String,
        image_hashes: Vec<String>,
        preview_digest: String,
        publish_future_text: bool,
    },
    #[serde(rename_all = "camelCase")]
    StopPublication {
        publication_id: String,
    },
    #[serde(rename_all = "camelCase")]
    ResumePublication {
        publication_id: String,
    },
    GetPublicationStatus,
    #[serde(rename_all = "camelCase")]
    GetPublicationCommandStatus {
        idempotency_key: String,
    },
}

pub(crate) fn handles(kind: &str) -> bool {
    matches!(
        kind,
        "beginPublicationAuthorization"
            | "previewPublication"
            | "completePublicationAuthorization"
            | "createPublication"
            | "stopPublication"
            | "resumePublication"
            | "getPublicationStatus"
            | "getPublicationCommandStatus"
    )
}
fn token(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}
fn random() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "publication_entropy_unavailable")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn digest(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
fn now() -> u64 {
    fabrials_agent_host::now_ms()
}
fn result(value: Value) -> (u16, Value) {
    (200, json!({"result":value}))
}
fn error(message: &str) -> (u16, Value) {
    (422, json!({"error":message}))
}
fn receipt(mut receipt: fabrials_agent_host::receipts::Receipt) -> (u16, Value) {
    receipt.fingerprint.clear();
    let mut value = serde_json::to_value(receipt).expect("receipt serializes");
    value["outcome"] = json!("publicationCommand");
    (202, json!({"result":value}))
}

impl Publications {
    pub fn new(
        host: &Arc<HostState>,
        directory: &Path,
        public_key: &str,
        web_origin: &str,
    ) -> Result<Self, String> {
        fabrials_agent_host::origin::validate_hosted_web_origin(web_origin)
            .map_err(|_| "publication_origin_invalid")?;
        fabrials_agent_host::instance::ensure_private_directory(directory)
            .map_err(|_| "publication_state_not_private")?;
        let bytes = crate::direct_notifications::private_read(directory, FILE, 4 * 1024 * 1024)?;
        let mut state: State = match bytes {
            Some(bytes) => {
                serde_json::from_slice(&bytes).map_err(|_| "publication_state_invalid")?
            }
            None => State::default(),
        };
        if public_key.len() != 64
            || !public_key.bytes().all(|b| b.is_ascii_hexdigit())
            || state
                .authorization
                .as_ref()
                .is_some_and(|auth| !token(&auth.grant))
            || state.pending.as_ref().is_some_and(|row| {
                !token(&row.verifier) || !token(&row.nonce) || !crate::opaque(&row.principal)
            })
            || state.publications.len() > 128
            || state.publications.iter().any(|(key, row)| {
                !id(key)
                    || !crate::opaque(&row.session_id)
                    || (!row.upload_token.is_empty() && !token(&row.upload_token))
                    || (!row.revoke_token.is_empty() && !token(&row.revoke_token))
                    || (!row.generation.is_empty() && !id(&row.generation))
                    || row.pending_upload.as_ref().is_some_and(|pending| {
                        pending.entry_hashes.len() > 10_000
                            || pending.ordinals.len() != pending.entry_hashes.len()
                            || pending.revision != row.revision.saturating_add(1)
                            || pending.request_digest.len() != 64
                            || !pending
                                .request_digest
                                .bytes()
                                .all(|byte| byte.is_ascii_hexdigit())
                    })
                    || row.entry_hashes.len() > 10_000
                    || row.ordinals.len() != row.entry_hashes.len()
                    || row.image_hashes.len() > 8
                    || row.image_hashes.iter().any(|hash| {
                        hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit())
                    })
                    || row.ordinals.windows(2).any(|pair| pair[0] >= pair[1])
                    || row
                        .ordinals
                        .last()
                        .is_some_and(|ordinal| *ordinal > row.high_water)
                    || !matches!(
                        row.phase.as_str(),
                        "active"
                            | "uploading"
                            | "paused"
                            | "pendingDelete"
                            | "needsReview"
                            | "expired"
                            | "revoked"
                    )
            })
        {
            return Err("publication_state_invalid".into());
        }
        for row in state
            .publications
            .values_mut()
            .filter(|row| row.phase == "uploading")
        {
            row.phase = "needsReview".into();
            row.error = Some("host_restarted".into());
        }
        let receipt_dir = directory.join("publication-receipts");
        fabrials_agent_host::instance::ensure_private_directory(&receipt_dir)
            .map_err(|_| "publication_receipts_unavailable")?;
        Ok(Self {
            host: Arc::downgrade(host),
            directory: directory.into(),
            public_key: public_key.into(),
            api: API.into(),
            exchange: EXCHANGE.into(),
            web_origin: web_origin.into(),
            client: reqwest::Client::builder()
                .https_only(true)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .map_err(|_| "publication_http_unavailable")?,
            state: Mutex::new(state),
            receipts: Mutex::new(
                Receipts::open(&receipt_dir).map_err(|_| "publication_receipts_unavailable")?,
            ),
            operation: Mutex::new(()),
        })
    }
    fn persist(&self, state: &State) -> Result<(), String> {
        if serde_json::to_vec(state)
            .map_err(|_| "publication_state_invalid")?
            .len()
            > 4 * 1024 * 1024
        {
            return Err("publication_history_full".into());
        }
        crate::state::write_private_json(&self.directory, FILE, state)
    }
    async fn request(&self, request: reqwest::RequestBuilder) -> Result<Value, String> {
        let response = request
            .send()
            .await
            .map_err(|_| "publication_response_unknown")?;
        let status = response.status();
        let body = crate::bounded_json(response, 64 * 1024)
            .await
            .map_err(|_| "publication_response_unknown")?;
        if status.is_success() {
            return Ok(body);
        }
        if status.is_server_error() {
            return Err("publication_response_unknown".into());
        }
        let code = body
            .get("error")
            .and_then(Value::as_str)
            .filter(|code| {
                code.len() <= 100
                    && code
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            })
            .unwrap_or("publication_request_rejected");
        Err(code.into())
    }
    fn authorized(
        &self,
        method: reqwest::Method,
        suffix: &str,
        auth: &Authorization,
    ) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{suffix}", self.api))
            .bearer_auth(&auth.grant)
            .header("X-Publication-Host-Key", &self.public_key)
            .header("Origin", &self.web_origin)
    }
    fn grant(state: &State) -> Result<Authorization, String> {
        state
            .authorization
            .as_ref()
            .filter(|auth| auth.expires_at > now())
            .cloned()
            .ok_or_else(|| "publication_authorization_required".into())
    }
    pub async fn dispatch(&self, access: &RemoteAccess, body: Value) -> (u16, Value) {
        if access.verify_access().await.is_err() {
            return (403, json!({"error":"revoked"}));
        }
        if !matches!(
            body.get("protocolVersion").and_then(Value::as_u64),
            Some(2 | 4)
        ) || body
            .get("requestId")
            .and_then(Value::as_str)
            .is_none_or(|id| !crate::opaque(id))
        {
            return error("invalid_request");
        }
        let Ok(operation) = serde_json::from_value::<Operation>(body["operation"].clone()) else {
            return error("invalid_publication_request");
        };
        match operation {
            Operation::PreviewPublication {
                session_id,
                image_hashes,
            } => {
                return match self.preview(&session_id, &image_hashes).await {
                    Ok(value) => result(value),
                    Err(message) => error(&message),
                };
            }
            Operation::GetPublicationStatus => {
                let state = self.state.lock().await;
                return result(
                    json!({"outcome":"publicationStatus", "authorized":Self::grant(&state).is_ok(),"authorizationExpiresAt":state.authorization.as_ref().map(|auth|auth.expires_at),"publications":state.publications.iter().map(|(id,row)|json!({"id":id,"sessionId":row.session_id,"phase":row.phase,"expiresAt":row.expires_at,"revision":row.revision,"error":row.error})).collect::<Vec<_>>()}),
                );
            }
            Operation::GetPublicationCommandStatus { idempotency_key } => {
                if !crate::opaque(&idempotency_key) {
                    return error("invalid_request");
                }
                let key = fabrials_agent_host::receipts::scoped_key(
                    access.session_id(),
                    &idempotency_key,
                );
                return match self.receipts.lock().await.get(&key) {
                    Some(mut stored) => {
                        stored.idempotency_key = idempotency_key;
                        receipt(stored)
                    }
                    None => error("unknown_command"),
                };
            }
            _ => {}
        }
        let Some(epoch) = body.get("controllerEpoch").and_then(Value::as_u64) else {
            return error("controller_required");
        };
        if access.authorize_control(epoch).await.is_err() {
            return error("controller_required");
        }
        let Some(idempotency) = body
            .get("idempotencyKey")
            .and_then(Value::as_str)
            .filter(|key| crate::opaque(key))
        else {
            return error("idempotency_key_required");
        };
        let key = fabrials_agent_host::receipts::scoped_key(access.session_id(), idempotency);
        let mut receipts = self.receipts.lock().await;
        let exists = receipts.get(&key).is_some();
        let started = match receipts.begin(key.clone(), idempotency.into(), &body["operation"]) {
            Ok(value) => value,
            Err(message) => return error(message),
        };
        if exists {
            return receipt(started);
        }
        drop(receipts);
        let _single = self.operation.lock().await;
        let outcome = if access.authorize_control(epoch).await.is_err() {
            Err("controller_required".into())
        } else {
            self.mutate(access.session_id(), operation).await
        };
        let (value, failure, uncertain) = match outcome {
            Ok(value) => (Some(value), None, false),
            Err(message) => {
                let uncertain = message == "publication_response_unknown"
                    || message.starts_with("remote_state_");
                (None, Some(message), uncertain)
            }
        };
        match self
            .receipts
            .lock()
            .await
            .finish(&key, value, failure, uncertain)
        {
            Ok(stored) => receipt(stored),
            Err(message) => error(message),
        }
    }
    async fn mutate(&self, principal: &str, operation: Operation) -> Result<Value, String> {
        match operation {
            Operation::BeginPublicationAuthorization => {
                let verifier = random()?;
                let nonce = random()?;
                let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
                let expires_at = now() + 5 * 60_000;
                let mut fragment =
                    reqwest::Url::parse("https://fabrials.com/account/publications/authorize")
                        .map_err(|_| "publication_url_invalid")?;
                let params = reqwest::Url::parse_with_params(
                    "https://fabrials.com/",
                    [
                        ("publication_host_key", self.public_key.as_str()),
                        ("publication_origin", self.web_origin.as_str()),
                        ("publication_challenge", challenge.as_str()),
                        ("publication_nonce", nonce.as_str()),
                        ("publication_label", "Spanreed computer"),
                        ("publication_return_path", "/"),
                    ],
                )
                .map_err(|_| "publication_url_invalid")?;
                fragment.set_fragment(params.query());
                let mut state = self.state.lock().await;
                state.pending = Some(Pending {
                    principal: principal.into(),
                    verifier,
                    nonce: nonce.clone(),
                    expires_at,
                });
                self.persist(&state)?;
                Ok(
                    json!({"outcome":"publicationAuthorization","url":fragment.as_str(),"nonce":nonce,"expiresAt":expires_at}),
                )
            }
            Operation::CompletePublicationAuthorization { code, nonce } => {
                if !token(&code) || !token(&nonce) {
                    return Err("publication_authorization_invalid".into());
                }
                let pending = self
                    .state
                    .lock()
                    .await
                    .pending
                    .as_ref()
                    .filter(|row| {
                        row.principal == principal && row.nonce == nonce && row.expires_at > now()
                    })
                    .cloned()
                    .ok_or("publication_authorization_expired")?;
                {
                    let mut state = self.state.lock().await;
                    state.pending = None;
                    self.persist(&state)?;
                }
                let response=self.request(self.client.post(&self.exchange).header("Origin", &self.web_origin).json(&json!({"code":code,"nonce":nonce,"verifier":pending.verifier,"hostPublicKey":self.public_key,"clientOrigin":self.web_origin,"returnPath":"/"}))).await?;
                let grant = response["grant"]
                    .as_str()
                    .filter(|value| token(value))
                    .ok_or("publication_response_unknown")?
                    .to_owned();
                let expires_at = response["expiresAt"]
                    .as_u64()
                    .and_then(|value| value.checked_mul(1000))
                    .filter(|expires| *expires > now() && *expires <= now() + 31 * 86_400_000)
                    .ok_or("publication_response_unknown")?;
                if response["scope"] != "publications:write" {
                    return Err("publication_response_unknown".into());
                }
                let mut state = self.state.lock().await;
                state.authorization = Some(Authorization { grant, expires_at });
                self.persist(&state)?;
                Ok(json!({"outcome":"publicationAuthorized","expiresAt":expires_at}))
            }
            Operation::CreatePublication {
                session_id,
                image_hashes,
                preview_digest,
                publish_future_text,
            } => {
                if !publish_future_text {
                    return Err("publication_consent_required".into());
                }
                self.create(&session_id, &image_hashes, &preview_digest)
                    .await
            }
            Operation::StopPublication { publication_id } => self.stop(&publication_id).await,
            Operation::ResumePublication { publication_id } => {
                {
                    let state = self.state.lock().await;
                    let row = state
                        .publications
                        .get(&publication_id)
                        .ok_or("publication_unknown")?;
                    if matches!(row.phase.as_str(), "revoked" | "pendingDelete" | "expired")
                        || row.expires_at <= now()
                    {
                        return Err("publication_expired".into());
                    }
                }
                self.reconcile(&publication_id).await?;
                let mut state = self.state.lock().await;
                Self::grant(&state)?;
                let row = state
                    .publications
                    .get_mut(&publication_id)
                    .ok_or("publication_unknown")?;
                if matches!(row.phase.as_str(), "revoked" | "pendingDelete")
                    || row.expires_at <= now()
                {
                    return Err("publication_expired".into());
                }
                if row.error.as_deref() == Some("publication_source_changed") {
                    return Err("publication_new_share_required".into());
                }
                row.phase = "active".into();
                row.error = None;
                self.persist(&state)?;
                Ok(json!({"outcome":"publicationResumed"}))
            }
            _ => Err("invalid_publication_request".into()),
        }
    }
    async fn candidate(
        &self,
        session_id: &str,
        image_hashes: &[String],
    ) -> Result<(PathBuf, String, crate::shares::VerifiedExport), String> {
        if !crate::opaque(session_id) {
            return Err("invalid_session".into());
        }
        let host = self.host.upgrade().ok_or("host_stopped")?;
        let session = host.session.lock().await;
        let live = session.sessions.get(session_id).ok_or("session_not_open")?;
        let workspace = session
            .workspaces
            .get(&live.workspace_id)
            .ok_or("workspace_not_enrolled")?;
        let path = workspace.path.clone();
        let title = crate::shares::label(&format!("Grok session · {}", workspace.display_name));
        drop(session);
        let mut exported = crate::shares::export_verified(
            &fabrials_agent_host::session_catalog::grok_home(),
            &path,
            session_id,
            &title,
            0,
            "",
        )?;
        crate::shares::ensure_document(&exported.document)?;
        self.attach_images(session_id, image_hashes, &mut exported.document)
            .await?;
        Ok((path, title, exported))
    }
    async fn attach_images(
        &self,
        session_id: &str,
        hashes: &[String],
        document: &mut Value,
    ) -> Result<(), String> {
        let host = self.host.upgrade().ok_or("host_stopped")?;
        let images = host
            .session
            .lock()
            .await
            .transcript_media
            .select(session_id, hashes)
            .map_err(|_| "publication_images_unavailable")?;
        document["images"]=json!(images.iter().map(|image| {
            let descriptor=image.descriptor();
            json!({"sha256":descriptor.sha256,"mimeType":descriptor.mime_type,"width":descriptor.width,"height":descriptor.height,"data":image.acp_block()["data"]})
        }).collect::<Vec<_>>());
        let mut bounded = document.clone();
        let count = bounded["entries"].as_array().map_or(0, Vec::len);
        bounded["ordinals"] = json!((1..=count).collect::<Vec<_>>());
        if serde_json::to_vec(&json!({"document":bounded,"revision":u64::MAX}))
            .map_err(|_| "publication_document_invalid")?
            .len()
            > 10 * 1024 * 1024
        {
            return Err("publication_document_too_large".into());
        }
        Ok(())
    }
    fn preview_digest(exported: &crate::shares::VerifiedExport) -> Result<String, String> {
        Ok(digest(&serde_json::to_vec(&json!({"document":exported.document,"sourceLength":exported.source_length,"sourceFingerprint":exported.source_fingerprint})).map_err(|_|"publication_document_invalid")?))
    }
    async fn preview(&self, session_id: &str, image_hashes: &[String]) -> Result<Value, String> {
        let (_, _, exported) = self.candidate(session_id, image_hashes).await?;
        let preview_digest = Self::preview_digest(&exported)?;
        let total_bytes = serde_json::to_vec(&exported.document)
            .map_err(|_| "publication_document_invalid")?
            .len();
        let total_entries = exported.document["entries"].as_array().map_or(0, Vec::len);
        let host = self.host.upgrade().ok_or("host_stopped")?;
        let available = host
            .session
            .lock()
            .await
            .transcript_media
            .descriptors(session_id);
        let mut document = exported.document;
        document
            .as_object_mut()
            .ok_or("publication_document_invalid")?
            .remove("images");
        Ok(
            json!({"outcome":"publicationPreview","document":document,"availableImages":available,"selectedImageHashes":image_hashes,"previewDigest":preview_digest,"totalBytes":total_bytes,"totalEntries":total_entries}),
        )
    }
    async fn create(
        &self,
        session_id: &str,
        image_hashes: &[String],
        preview_digest: &str,
    ) -> Result<Value, String> {
        let (path, title, exported) = self.candidate(session_id, image_hashes).await?;
        if Self::preview_digest(&exported)? != preview_digest {
            return Err("publication_preview_changed".into());
        }
        if exported.document["entries"]
            .as_array()
            .is_none_or(|entries| entries.is_empty())
            && image_hashes.is_empty()
        {
            return Err("publication_empty".into());
        }
        let state = self.state.lock().await;
        if state.publications.len() >= 128 {
            return Err("publication_history_full".into());
        }
        if state
            .publications
            .values()
            .filter(|row| !matches!(row.phase.as_str(), "revoked" | "expired"))
            .count()
            >= 5
        {
            return Err("publication_active_limit".into());
        }
        if state.publications.values().any(|row| {
            row.session_id == session_id && !matches!(row.phase.as_str(), "revoked" | "expired")
        }) {
            return Err("publication_already_exists".into());
        }
        let auth = Self::grant(&state)?;
        drop(state);
        let response = self
            .request(self.authorized(reqwest::Method::POST, "/create", &auth))
            .await?;
        let publication_id = response["id"]
            .as_str()
            .filter(|value| id(value))
            .ok_or("publication_response_unknown")?
            .to_owned();
        let upload_token = response["uploadToken"]
            .as_str()
            .filter(|value| token(value))
            .ok_or("publication_response_unknown")?
            .to_owned();
        let revoke_token = response["revokeToken"]
            .as_str()
            .filter(|value| token(value))
            .ok_or("publication_response_unknown")?
            .to_owned();
        let generation = response["generation"]
            .as_str()
            .filter(|value| id(value))
            .ok_or("publication_response_unknown")?
            .to_owned();
        if response["revision"].as_u64() != Some(0)
            || response["publicationUrl"]
                != format!("https://publications.fabrials.com/#{publication_id}")
        {
            return Err("publication_response_unknown".into());
        }
        let expires_at = response["expiresAt"]
            .as_u64()
            .and_then(|value| value.checked_mul(1000))
            .filter(|expires| *expires > now() && *expires <= now() + 7 * 86_400_000)
            .ok_or("publication_response_unknown")?;
        let row = Publication {
            session_id: session_id.into(),
            workspace: path,
            title,
            upload_token,
            revoke_token,
            generation,
            pending_upload: None,
            phase: "active".into(),
            expires_at,
            revision: 0,
            fingerprint: String::new(),
            entry_hashes: Vec::new(),
            ordinals: Vec::new(),
            high_water: 0,
            source_length: 0,
            source_fingerprint: String::new(),
            image_hashes: image_hashes.to_vec(),
            next_attempt: 0,
            delete_retry_ms: 0,
            error: None,
        };
        let mut state = self.state.lock().await;
        state.publications.insert(publication_id.clone(), row);
        self.persist(&state)?;
        drop(state);
        self.upload(&publication_id, exported).await?;
        let state = self.state.lock().await;
        let row = state
            .publications
            .get(&publication_id)
            .ok_or("publication_unknown")?;
        Ok(
            json!({"outcome":"publicationCreated","id":publication_id,"url":format!("https://publications.fabrials.com/#{publication_id}"),"expiresAt":row.expires_at}),
        )
    }
    async fn stop(&self, publication_id: &str) -> Result<Value, String> {
        let mut state = self.state.lock().await;
        let row = state
            .publications
            .get_mut(publication_id)
            .ok_or("publication_unknown")?;
        if row.phase == "revoked" {
            return Ok(
                json!({"outcome":"publicationStopped","accessRevoked":true,"keysDestroyed":true,"storageCleanup":"pending"}),
            );
        }
        row.phase = "pendingDelete".into();
        row.error = None;
        self.persist(&state)?;
        let row = state
            .publications
            .get(publication_id)
            .ok_or("publication_unknown")?;
        let revoke_token = zeroize::Zeroizing::new(row.revoke_token.clone());
        let request = if token(&revoke_token) {
            self.client
                .post(format!("{}/revoke", self.api))
                .header("X-Publication-Id", publication_id)
                .header("X-Publication-Revoke-Token", revoke_token.as_str())
                .header("Origin", &self.web_origin)
        } else {
            let auth = Self::grant(&state)?;
            self.authorized(reqwest::Method::DELETE, "/withdraw", &auth)
                .header("X-Publication-Id", publication_id)
        };
        drop(state);
        let response = self.request(request).await;
        let response = match response {
            Ok(value) => value,
            Err(error) => {
                let mut state = self.state.lock().await;
                if let Some(row) = state.publications.get_mut(publication_id) {
                    row.delete_retry_ms = row
                        .delete_retry_ms
                        .saturating_mul(2)
                        .clamp(60_000, 3_600_000);
                    row.next_attempt = now() + row.delete_retry_ms;
                    row.error = Some(error.clone());
                }
                self.persist(&state)?;
                return Err(error);
            }
        };
        if response["accessRevoked"] != true || response["keysDestroyed"] != true {
            return Err("publication_response_unknown".into());
        }
        let mut state = self.state.lock().await;
        let row = state
            .publications
            .get_mut(publication_id)
            .ok_or("publication_unknown")?;
        row.phase = "revoked".into();
        row.upload_token.zeroize();
        row.revoke_token.zeroize();
        row.pending_upload = None;
        row.entry_hashes.clear();
        row.ordinals.clear();
        row.fingerprint.clear();
        row.image_hashes.clear();
        self.persist(&state)?;
        Ok(
            json!({"outcome":"publicationStopped","accessRevoked":true,"keysDestroyed":true,"metadataCleanup":response["metadataCleanup"],"storageCleanup":response["storageCleanup"]}),
        )
    }
    async fn upload(
        &self,
        publication_id: &str,
        exported: crate::shares::VerifiedExport,
    ) -> Result<(), String> {
        let mut state = self.state.lock().await;
        let auth = Self::grant(&state)?;
        let row = state
            .publications
            .get_mut(publication_id)
            .ok_or("publication_unknown")?;
        if row.phase != "active" || row.next_attempt > now() {
            return Ok(());
        }
        if row.expires_at <= now() {
            row.phase = "expired".into();
            row.upload_token.zeroize();
            self.persist(&state)?;
            return Ok(());
        }
        let mut document = exported.document;
        let fingerprint =
            digest(&serde_json::to_vec(&document).map_err(|_| "publication_document_invalid")?);
        if row.fingerprint == fingerprint {
            return Ok(());
        }
        let entries = document["entries"]
            .as_array()
            .ok_or("publication_document_invalid")?;
        let hashes: Vec<String> = entries
            .iter()
            .map(|entry| digest(&serde_json::to_vec(entry).expect("JSON serializes")))
            .collect();
        let ordinals: Vec<u64> = (1..=hashes.len() as u64).collect();
        let high = row.high_water.max(hashes.len() as u64);
        document["ordinals"] = json!(ordinals);
        let revision = row
            .revision
            .checked_add(1)
            .ok_or("publication_revision_exhausted")?;
        let bytes = serde_json::to_vec(&json!({"revision":revision,"document":document}))
            .map_err(|_| "publication_document_invalid")?;
        if bytes.len() > 10 * 1024 * 1024 {
            return Err("publication_document_too_large".into());
        }
        let pending = PendingUpload {
            revision,
            request_digest: digest(&bytes),
            fingerprint,
            entry_hashes: hashes,
            ordinals,
            high_water: high,
            source_length: exported.source_length,
            source_fingerprint: exported.source_fingerprint,
        };
        row.next_attempt = now() + 60_000;
        let previous_revision = row.revision;
        let generation = row.generation.clone();
        if !id(&generation) {
            return Err("publication_new_share_required".into());
        }
        let upload_token = zeroize::Zeroizing::new(row.upload_token.clone());
        row.phase = "uploading".into();
        row.pending_upload = Some(pending.clone());
        self.persist(&state)?;
        drop(state);
        let response = self
            .request(
                self.authorized(reqwest::Method::PUT, "/upload", &auth)
                    .header("X-Publication-Id", publication_id)
                    .header("X-Publication-Generation", &generation)
                    .header("X-Publication-Revision", previous_revision)
                    .header("X-Publication-Upload-Token", upload_token.as_str())
                    .header("Content-Type", "application/json")
                    .header("Content-Length", bytes.len())
                    .body(bytes),
            )
            .await;
        let mut state = self.state.lock().await;
        let row = state
            .publications
            .get_mut(publication_id)
            .ok_or("publication_unknown")?;
        match response {
            Ok(response) if Self::acknowledged(row, &pending, &response) => {
                Self::commit_upload(row, pending, &response);
                self.persist(&state)
            }
            response => {
                let error = response
                    .err()
                    .unwrap_or_else(|| "publication_response_unknown".into());
                let uncertain = error == "publication_response_unknown"
                    || error.contains("revision")
                    || error.contains("generation");
                row.phase = if uncertain { "needsReview" } else { "paused" }.into();
                if !uncertain {
                    row.pending_upload = None;
                }
                row.error = Some(error.clone());
                self.persist(&state)?;
                Err(error)
            }
        }
    }
    fn acknowledged(row: &Publication, pending: &PendingUpload, response: &Value) -> bool {
        response["generation"].as_str() == Some(row.generation.as_str())
            && response["revision"].as_u64() == Some(pending.revision)
            && response["requestDigest"].as_str() == Some(pending.request_digest.as_str())
            && response["expiresAt"]
                .as_u64()
                .and_then(|value| value.checked_mul(1000))
                .is_some_and(|expires| expires > now() && expires <= now() + 7 * 86_400_000)
    }
    fn commit_upload(row: &mut Publication, pending: PendingUpload, response: &Value) {
        row.phase = "active".into();
        row.revision = pending.revision;
        row.fingerprint = pending.fingerprint;
        row.entry_hashes = pending.entry_hashes;
        row.ordinals = pending.ordinals;
        row.high_water = pending.high_water;
        row.source_length = pending.source_length;
        row.source_fingerprint = pending.source_fingerprint;
        row.expires_at = response["expiresAt"].as_u64().expect("acknowledged expiry") * 1000;
        row.pending_upload = None;
        row.error = None;
    }
    async fn reconcile(&self, publication_id: &str) -> Result<(), String> {
        let state = self.state.lock().await;
        let row = state
            .publications
            .get(publication_id)
            .ok_or("publication_unknown")?;
        let Some(pending) = row.pending_upload.clone() else {
            return Ok(());
        };
        let auth = Self::grant(&state)?;
        drop(state);
        let response = self
            .request(
                self.authorized(reqwest::Method::GET, "/status", &auth)
                    .header("X-Publication-Id", publication_id),
            )
            .await?;
        let mut state = self.state.lock().await;
        let row = state
            .publications
            .get_mut(publication_id)
            .ok_or("publication_unknown")?;
        if response["id"].as_str() != Some(publication_id)
            || !Self::acknowledged(row, &pending, &response)
        {
            return Err("publication_upload_unconfirmed".into());
        }
        Self::commit_upload(row, pending, &response);
        self.persist(&state)
    }
    pub async fn sync(&self) -> Result<(), String> {
        self.sync_at(&fabrials_agent_host::session_catalog::grok_home())
            .await
    }
    async fn sync_at(&self, home: &Path) -> Result<(), String> {
        let Ok(_single) = self.operation.try_lock() else {
            return Ok(());
        };
        {
            let mut state = self.state.lock().await;
            let mut changed = false;
            for row in state.publications.values_mut().filter(|row| {
                row.expires_at <= now()
                    && !matches!(row.phase.as_str(), "revoked" | "expired" | "pendingDelete")
            }) {
                row.phase = "expired".into();
                row.upload_token.zeroize();
                row.entry_hashes.clear();
                row.ordinals.clear();
                row.fingerprint.clear();
                row.error = None;
                changed = true;
            }
            if changed {
                self.persist(&state)?;
            }
        }
        let rows: Vec<(String, Publication)> = self
            .state
            .lock()
            .await
            .publications
            .iter()
            .filter(|(_, row)| {
                row.next_attempt <= now() && (row.phase == "active" || row.phase == "pendingDelete")
            })
            .take(8)
            .map(|(id, row)| (id.clone(), row.clone()))
            .collect();
        for (id, row) in rows {
            if row.next_attempt > now() {
                continue;
            }
            if row.phase == "pendingDelete" {
                {
                    let mut state = self.state.lock().await;
                    if let Some(row) = state.publications.get_mut(&id) {
                        row.next_attempt = now() + 60_000;
                    }
                    self.persist(&state)?;
                }
                let _ = self.stop(&id).await;
                continue;
            }
            match crate::shares::export_verified(
                home,
                &row.workspace,
                &row.session_id,
                &row.title,
                row.source_length,
                &row.source_fingerprint,
            ) {
                Ok(mut exported) => {
                    match self
                        .attach_images(&row.session_id, &row.image_hashes, &mut exported.document)
                        .await
                    {
                        Ok(()) => {
                            let _ = self.upload(&id, exported).await;
                        }
                        Err(error) => {
                            let mut state = self.state.lock().await;
                            if let Some(row) = state.publications.get_mut(&id) {
                                row.phase = "paused".into();
                                row.error = Some(error);
                            }
                            self.persist(&state)?;
                        }
                    }
                }
                Err(error) => {
                    let mut state = self.state.lock().await;
                    if let Some(row) = state.publications.get_mut(&id) {
                        row.phase = "paused".into();
                        row.error = Some(error);
                    }
                    self.persist(&state)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Bytes,
        extract::State as HttpState,
        http::{HeaderMap, Method, StatusCode, Uri},
        routing::any,
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    const ID: &str = "01234567-89ab-cdef-0123-456789abcdef";
    struct Requests {
        seen: Mutex<Vec<(Method, HeaderMap, Value, String)>>,
        reject: AtomicBool,
        lose_reply: AtomicBool,
        committed: Mutex<Value>,
    }
    async fn backend(
        HttpState(state): HttpState<Arc<Requests>>,
        method: Method,
        uri: Uri,
        headers: HeaderMap,
        bytes: Bytes,
    ) -> (StatusCode, axum::Json<Value>) {
        let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        state.seen.lock().await.push((
            method.clone(),
            headers.clone(),
            body.clone(),
            uri.to_string(),
        ));
        if state.reject.load(Ordering::Relaxed) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                axum::Json(json!({"error":"publication_paused_account_ingress_day","paused":true})),
            );
        }
        let result = match (method, uri.path()) {
            (Method::POST, "/exchange") => {
                json!({"grant":"g".repeat(43),"expiresAt":now()/1000+86400,"scope":"publications:write"})
            }
            (Method::PUT, "/v1/upload") => {
                let result = json!({"id":ID,"generation":headers["x-publication-generation"].to_str().unwrap(),"revision":body["revision"],"requestDigest":digest(&bytes),"expiresAt":now()/1000+604800,"highWater":body["document"]["ordinals"].as_array().and_then(|items|items.last())});
                *state.committed.lock().await = result.clone();
                if state.lose_reply.load(Ordering::Relaxed) {
                    return (
                        StatusCode::BAD_GATEWAY,
                        axum::Json(json!({"error":"backend_unavailable"})),
                    );
                }
                result
            }
            (Method::GET, "/v1/status") => state.committed.lock().await.clone(),
            (Method::POST, "/v1/revoke") | (Method::DELETE, "/v1/withdraw") => {
                return (
                    StatusCode::ACCEPTED,
                    axum::Json(
                        json!({"accessRevoked":true,"keysDestroyed":true,"metadataCleanup":"pending","storageCleanup":"pending"}),
                    ),
                );
            }
            _ => Value::Null,
        };
        (StatusCode::OK, axum::Json(result))
    }
    async fn service() -> (
        tempfile::TempDir,
        Arc<HostState>,
        Publications,
        Arc<Requests>,
        tokio::task::JoinHandle<()>,
    ) {
        let temporary = tempfile::tempdir().unwrap();
        let host = Arc::new(HostState::new(
            fabrials_agent_host::origin::LocalOrigin::new(
                "0123456789abcdef0123456789abcdef",
                21001,
            )
            .unwrap(),
        ));
        let mut service = Publications::new(
            &host,
            &temporary.path().join("state"),
            &"a".repeat(64),
            "https://desktop.grok.me",
        )
        .unwrap();
        let requests = Arc::new(Requests {
            seen: Mutex::new(Vec::new()),
            reject: AtomicBool::new(false),
            lose_reply: AtomicBool::new(false),
            committed: Mutex::new(Value::Null),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        service.api = format!("http://{}/v1", listener.local_addr().unwrap());
        service.exchange = format!("http://{}/exchange", listener.local_addr().unwrap());
        service.client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let router = Router::new()
            .fallback(any(backend))
            .with_state(requests.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (temporary, host, service, requests, server)
    }
    async fn seed(service: &Publications) {
        let mut state = service.state.lock().await;
        state.authorization = Some(Authorization {
            grant: "g".repeat(43),
            expires_at: now() + 86_400_000,
        });
        state.publications.insert(
            ID.into(),
            Publication {
                session_id: "session".into(),
                workspace: PathBuf::from("/synthetic"),
                title: "Session".into(),
                upload_token: "u".repeat(43),
                revoke_token: "r".repeat(43),
                generation: ID.into(),
                pending_upload: None,
                phase: "active".into(),
                expires_at: now() + 300_000,
                revision: 0,
                fingerprint: String::new(),
                entry_hashes: Vec::new(),
                ordinals: Vec::new(),
                high_water: 0,
                source_length: 0,
                source_fingerprint: String::new(),
                image_hashes: Vec::new(),
                next_attempt: 0,
                delete_retry_ms: 0,
                error: None,
            },
        );
        service.persist(&state).unwrap();
    }
    fn document(texts: &[&str]) -> crate::shares::VerifiedExport {
        crate::shares::VerifiedExport {
            source_length: 1,
            source_fingerprint: "synthetic".into(),
            document: json!({"title":"Session","entries":texts.iter().map(|text|json!({"kind":"message","role":"user","text":text})).collect::<Vec<_>>()}),
        }
    }
    async fn elapsed(service: &Publications) {
        service
            .state
            .lock()
            .await
            .publications
            .get_mut(ID)
            .unwrap()
            .next_attempt = 0;
    }

    #[tokio::test]
    async fn pkce_is_bound_to_browser_and_only_the_host_receives_grant() {
        let (_dir, host, service, requests, server) = service().await;
        let nonce = host.pairing.lock().await.mint_nonce(now()).unwrap();
        let (access, _) = RemoteAccess::pair(host.clone(), nonce.expose())
            .await
            .unwrap();
        let epoch = access.connect().await.unwrap();
        let envelope = json!({"protocolVersion":2,"requestId":"request","controllerEpoch":epoch,"idempotencyKey":"begin","operation":{"kind":"beginPublicationAuthorization"}});
        let started = service.dispatch(&access, envelope.clone()).await;
        assert_eq!(started.1["result"]["state"], "completed");
        assert_eq!(started, service.dispatch(&access, envelope).await);
        let url =
            reqwest::Url::parse(started.1["result"]["result"]["url"].as_str().unwrap()).unwrap();
        assert_eq!(url.origin().ascii_serialization(), "https://fabrials.com");
        assert!(url.query().is_none());
        let parsed = reqwest::Url::parse(&format!(
            "https://fabrials.com/?{}",
            url.fragment().unwrap()
        ))
        .unwrap();
        let fragment: BTreeMap<_, _> = parsed
            .query_pairs()
            .map(|(a, b)| (a.into_owned(), b.into_owned()))
            .collect();
        let pending = service.state.lock().await.pending.clone().unwrap();
        assert_eq!(
            fragment["publication_challenge"],
            URL_SAFE_NO_PAD.encode(Sha256::digest(pending.verifier.as_bytes()))
        );
        assert!(!started.1.to_string().contains(&pending.verifier));
        assert!(
            service
                .mutate(
                    "other-browser",
                    Operation::CompletePublicationAuthorization {
                        code: "c".repeat(43),
                        nonce: pending.nonce.clone()
                    }
                )
                .await
                .is_err()
        );
        assert!(requests.seen.lock().await.is_empty());
        let complete = json!({"protocolVersion":2,"requestId":"request2","controllerEpoch":epoch,"idempotencyKey":"complete","operation":{"kind":"completePublicationAuthorization","code":"c".repeat(43),"nonce":pending.nonce}});
        let completed = service.dispatch(&access, complete.clone()).await;
        assert_eq!(completed, service.dispatch(&access, complete.clone()).await);
        assert_eq!(requests.seen.lock().await.len(), 1);
        let mut conflict = complete;
        conflict["operation"]["code"] = json!("x".repeat(43));
        assert_eq!(
            service.dispatch(&access, conflict).await.1["error"],
            "idempotency_conflict"
        );
        assert_eq!(completed.1["result"]["state"], "completed");
        assert!(!completed.1.to_string().contains(&"g".repeat(43)));
        assert!(service.state.lock().await.pending.is_none());
        let status=service.dispatch(&access,json!({"protocolVersion":2,"requestId":"status","operation":{"kind":"getPublicationStatus"}})).await;
        assert_eq!(status.1["result"]["authorized"], true);
        assert!(!status.1.to_string().contains(&"g".repeat(43)));
        host.pairing.lock().await.revoke(access.session_id());
        assert_eq!(service.dispatch(&access,json!({"protocolVersion":2,"requestId":"status","operation":{"kind":"getPublicationStatus"}})).await.0,403);
        server.abort();
    }

    #[tokio::test]
    async fn uploads_bound_resources_keep_ordinals_and_pause_until_explicit_resume() {
        let (_dir, _host, service, requests, server) = service().await;
        seed(&service).await;
        service.upload(ID, document(&["one", "two"])).await.unwrap();
        let sent = requests.seen.lock().await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].2["document"]["ordinals"], json!([1, 2]));
        assert!(sent[0].1.contains_key("content-length"));
        assert_eq!(sent[0].1["x-publication-host-key"], "a".repeat(64));
        assert_eq!(sent[0].1["x-publication-upload-token"], "u".repeat(43));
        assert_eq!(sent[0].1["x-publication-id"], ID);
        assert_eq!(sent[0].1["x-publication-generation"], ID);
        assert_eq!(sent[0].1["x-publication-revision"], "0");
        assert_eq!(sent[0].1["origin"], "https://desktop.grok.me");
        assert_eq!(sent[0].3, "/v1/upload");
        drop(sent);
        service
            .upload(ID, document(&["one", "two", "three"]))
            .await
            .unwrap();
        assert_eq!(requests.seen.lock().await.len(), 1);
        elapsed(&service).await;
        service.upload(ID, document(&["one", "two"])).await.unwrap();
        assert_eq!(requests.seen.lock().await.len(), 1);
        service
            .upload(ID, document(&["one", "two", "three"]))
            .await
            .unwrap();
        assert_eq!(
            requests.seen.lock().await[1].2["document"]["ordinals"],
            json!([1, 2, 3])
        );
        elapsed(&service).await;
        service.upload(ID, document(&["one"])).await.unwrap();
        elapsed(&service).await;
        service
            .upload(ID, document(&["one", "two", "three"]))
            .await
            .unwrap();
        assert_eq!(
            requests.seen.lock().await[3].2["document"]["ordinals"],
            json!([1, 2, 3])
        );
        elapsed(&service).await;
        requests.reject.store(true, Ordering::Relaxed);
        assert!(service.upload(ID, document(&["new"])).await.is_err());
        assert_eq!(service.state.lock().await.publications[ID].phase, "paused");
        elapsed(&service).await;
        service.upload(ID, document(&["new"])).await.unwrap();
        assert_eq!(requests.seen.lock().await.len(), 5);
        requests.reject.store(false, Ordering::Relaxed);
        service
            .mutate(
                "browser",
                Operation::ResumePublication {
                    publication_id: ID.into(),
                },
            )
            .await
            .unwrap();
        service.upload(ID, document(&["new"])).await.unwrap();
        assert_eq!(requests.seen.lock().await.len(), 6);
        server.abort();
    }

    #[tokio::test]
    async fn stop_disables_upload_before_network_and_reports_only_acknowledged_erasure() {
        let (_dir, _host, service, requests, server) = service().await;
        seed(&service).await;
        requests.reject.store(true, Ordering::Relaxed);
        assert!(service.stop(ID).await.is_err());
        assert_eq!(
            service.state.lock().await.publications[ID].phase,
            "pendingDelete"
        );
        service.upload(ID, document(&["private"])).await.unwrap();
        assert_eq!(requests.seen.lock().await.len(), 1);
        requests.reject.store(false, Ordering::Relaxed);
        let stopped = service.stop(ID).await.unwrap();
        assert_eq!(stopped["keysDestroyed"], true);
        assert_eq!(service.state.lock().await.publications[ID].phase, "revoked");
        assert!(
            service.state.lock().await.publications[ID]
                .upload_token
                .is_empty()
        );
        assert_eq!(service.stop(ID).await.unwrap()["accessRevoked"], true);
        assert_eq!(requests.seen.lock().await.len(), 2);
        server.abort();
    }

    #[tokio::test]
    async fn restart_does_not_replay_upload_and_stop_works_locally_after_grant_expiry() {
        let (dir, host, service, _requests, server) = service().await;
        seed(&service).await;
        {
            let mut state = service.state.lock().await;
            state.publications.get_mut(ID).unwrap().phase = "uploading".into();
            service.persist(&state).unwrap();
        }
        let mut resumed = Publications::new(
            &host,
            &dir.path().join("state"),
            &"a".repeat(64),
            "https://desktop.grok.me",
        )
        .unwrap();
        resumed.api = service.api.clone();
        resumed.client = service.client.clone();
        assert_eq!(
            resumed.state.lock().await.publications[ID].phase,
            "needsReview"
        );
        resumed.state.lock().await.authorization = None;
        assert_eq!(resumed.stop(ID).await.unwrap()["keysDestroyed"], true);
        assert_eq!(resumed.state.lock().await.publications[ID].phase, "revoked");
        server.abort();
    }

    #[tokio::test]
    async fn lost_upload_response_reconciles_exact_digest_without_reuploading() {
        let (_dir, _host, service, requests, server) = service().await;
        seed(&service).await;
        requests.lose_reply.store(true, Ordering::Relaxed);
        assert_eq!(
            service.upload(ID, document(&["one"])).await.unwrap_err(),
            "publication_response_unknown"
        );
        assert_eq!(service.state.lock().await.publications[ID].revision, 0);
        assert_eq!(
            service.state.lock().await.publications[ID].phase,
            "needsReview"
        );
        service
            .mutate(
                "browser",
                Operation::ResumePublication {
                    publication_id: ID.into(),
                },
            )
            .await
            .unwrap();
        let state = service.state.lock().await;
        assert_eq!(state.publications[ID].revision, 1);
        assert!(state.publications[ID].pending_upload.is_none());
        drop(state);
        let sent = requests.seen.lock().await;
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].0, Method::GET);
        assert_eq!(sent[1].3, "/v1/status");
        assert_eq!(sent[1].1["x-publication-id"], ID);
        server.abort();
    }

    #[tokio::test]
    async fn uncertain_upload_never_adopts_other_content_or_generation() {
        for field in ["requestDigest", "generation", "id", "revision"] {
            let (_dir, _host, service, requests, server) = service().await;
            seed(&service).await;
            requests.lose_reply.store(true, Ordering::Relaxed);
            service.upload(ID, document(&["one"])).await.unwrap_err();
            requests.committed.lock().await[field] = json!("different");
            assert_eq!(
                service
                    .mutate(
                        "browser",
                        Operation::ResumePublication {
                            publication_id: ID.into()
                        }
                    )
                    .await
                    .unwrap_err(),
                "publication_upload_unconfirmed"
            );
            assert_eq!(service.state.lock().await.publications[ID].revision, 0);
            assert!(
                service.state.lock().await.publications[ID]
                    .pending_upload
                    .is_some()
            );
            server.abort();
        }
    }

    #[tokio::test]
    async fn expired_grant_cannot_prevent_revocation_and_token_never_reaches_browser() {
        let (_dir, _host, service, requests, server) = service().await;
        seed(&service).await;
        service.state.lock().await.authorization = None;
        let result = service.stop(ID).await.unwrap();
        assert_eq!(result["keysDestroyed"], true);
        assert_eq!(result["metadataCleanup"], "pending");
        assert!(!result.to_string().contains(&"r".repeat(43)));
        let sent = requests.seen.lock().await;
        assert_eq!(sent[0].3, "/v1/revoke");
        assert!(!sent[0].1.contains_key("authorization"));
        assert!(!sent[0].1.contains_key("cookie"));
        assert_eq!(sent[0].1["x-publication-revoke-token"], "r".repeat(43));
        assert_eq!(sent[0].1["x-publication-id"], ID);
        server.abort();
    }

    #[tokio::test]
    async fn qualification_origin_is_bound_into_pkce_and_exchange() {
        let (_dir, _host, mut service, requests, server) = service().await;
        service.web_origin = "https://grok.fabrials.com".into();
        let begin = service
            .mutate("browser", Operation::BeginPublicationAuthorization)
            .await
            .unwrap();
        let url = reqwest::Url::parse(begin["url"].as_str().unwrap()).unwrap();
        let fragment = reqwest::Url::parse(&format!(
            "https://fabrials.com/?{}",
            url.fragment().unwrap()
        ))
        .unwrap();
        assert!(fragment.query_pairs().any(
            |(key, value)| key == "publication_origin" && value == "https://grok.fabrials.com"
        ));
        service
            .mutate(
                "browser",
                Operation::CompletePublicationAuthorization {
                    code: "c".repeat(43),
                    nonce: begin["nonce"].as_str().unwrap().into(),
                },
            )
            .await
            .unwrap();
        let sent = requests.seen.lock().await;
        assert_eq!(sent[0].1["origin"], "https://grok.fabrials.com");
        assert_eq!(sent[0].2["clientOrigin"], "https://grok.fabrials.com");
        assert_eq!(sent[0].2["returnPath"], "/");
        server.abort();
    }

    fn source(home: &Path, lines: &[Value]) -> PathBuf {
        let directory = home
            .join("sessions")
            .join(fabrials_agent_host::session_catalog::encode_cwd_dirname(
                "/synthetic",
            ))
            .join("session");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("updates.jsonl");
        std::fs::write(
            &path,
            lines
                .iter()
                .map(|line| format!("{line}\n"))
                .collect::<String>(),
        )
        .unwrap();
        path
    }
    fn chunk(role: &str, text: &str) -> Value {
        json!({"update":{"sessionUpdate":format!("{role}_message_chunk"),"content":{"type":"text","text":text}}})
    }

    #[tokio::test]
    async fn source_edits_deletions_and_reorders_pause_without_renumbering_or_network() {
        for mode in 0..3 {
            let (dir, _host, service, requests, server) = service().await;
            seed(&service).await;
            let home = dir.path().join("grok");
            let original = [chunk("user", "one"), chunk("agent", "two")];
            source(&home, &original);
            service.sync_at(&home).await.unwrap();
            assert_eq!(requests.seen.lock().await.len(), 1);
            assert_eq!(
                requests.seen.lock().await[0].2["document"]["ordinals"],
                json!([1, 2])
            );
            elapsed(&service).await;
            let changed = match mode {
                0 => vec![chunk("user", "edited"), original[1].clone()],
                1 => vec![original[0].clone()],
                _ => vec![original[1].clone(), original[0].clone()],
            };
            source(&home, &changed);
            service.sync_at(&home).await.unwrap();
            assert_eq!(requests.seen.lock().await.len(), 1);
            let state = service.state.lock().await;
            assert_eq!(state.publications[ID].phase, "paused");
            assert_eq!(
                state.publications[ID].error.as_deref(),
                Some("publication_source_changed")
            );
            drop(state);
            assert_eq!(
                service
                    .mutate(
                        "browser",
                        Operation::ResumePublication {
                            publication_id: ID.into()
                        }
                    )
                    .await
                    .unwrap_err(),
                "publication_new_share_required"
            );
            server.abort();
        }
    }

    #[tokio::test]
    async fn appended_chunks_keep_identity_and_only_new_entries_get_new_ordinals() {
        let (dir, _host, service, requests, server) = service().await;
        seed(&service).await;
        let home = dir.path().join("grok");
        let mut lines = vec![chunk("user", "one"), chunk("agent", "two")];
        source(&home, &lines);
        service.sync_at(&home).await.unwrap();
        elapsed(&service).await;
        lines.push(chunk("agent", " extended"));
        source(&home, &lines);
        service.sync_at(&home).await.unwrap();
        assert_eq!(
            requests.seen.lock().await[1].2["document"]["ordinals"],
            json!([1, 2])
        );
        elapsed(&service).await;
        lines.push(chunk("user", "new"));
        source(&home, &lines);
        service.sync_at(&home).await.unwrap();
        assert_eq!(
            requests.seen.lock().await[2].2["document"]["ordinals"],
            json!([1, 2, 3])
        );
        server.abort();
    }

    #[tokio::test]
    async fn images_are_normalized_selected_explicitly_and_bound_into_preview_consent() {
        let (_dir, host, service, requests, server) = service().await;
        let normalized=fabrials_agent_host::media::PromptImage::from_acp(&json!({"type":"image","data":"iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAKklEQVR4nGMo75hJU8QwasGoBaMWjFowasGoBaMWjFowasGoBaMWDBULAMfpYFtYNCzjAAAAAElFTkSuQmCC"})).unwrap();
        host.session
            .lock()
            .await
            .transcript_media
            .remember("session", std::slice::from_ref(&normalized));
        let mut plain = document(&["text"]);
        service
            .attach_images("session", &[], &mut plain.document)
            .await
            .unwrap();
        assert_eq!(plain.document["images"], json!([]));
        let before = Publications::preview_digest(&plain).unwrap();
        let hash = normalized.descriptor().sha256;
        service
            .attach_images("session", std::slice::from_ref(&hash), &mut plain.document)
            .await
            .unwrap();
        assert_eq!(plain.document["images"][0]["sha256"], hash);
        assert_eq!(
            plain.document["images"][0]["data"],
            normalized.acp_block()["data"]
        );
        assert_ne!(before, Publications::preview_digest(&plain).unwrap());
        assert!(
            service
                .attach_images(
                    "other-session",
                    std::slice::from_ref(&hash),
                    &mut plain.document
                )
                .await
                .is_err()
        );
        assert!(
            service
                .attach_images("session", &[hash.clone(), hash], &mut plain.document)
                .await
                .is_err()
        );
        assert_eq!(
            service
                .mutate(
                    "browser",
                    Operation::CreatePublication {
                        session_id: "session".into(),
                        image_hashes: Vec::new(),
                        preview_digest: before,
                        publish_future_text: false
                    }
                )
                .await
                .unwrap_err(),
            "publication_consent_required"
        );
        assert!(requests.seen.lock().await.is_empty());
        server.abort();
    }

    #[test]
    fn large_publication_previews_fit_bounded_noise_frames_without_truncation() {
        use fabrials_remote_noise::{Handshake, identity};
        let host = identity().unwrap();
        let browser = identity().unwrap();
        let mut a = Handshake::new(
            &browser.private,
            Some(host.public.as_slice().try_into().unwrap()),
            "host",
            "channel",
        )
        .unwrap();
        let mut b = Handshake::new(&host.private, None, "host", "channel").unwrap();
        b.read(&a.write(&[]).unwrap()).unwrap();
        a.read(&b.write(&[]).unwrap()).unwrap();
        b.read(&a.write(&[]).unwrap()).unwrap();
        let mut reader = a.finish().unwrap();
        let mut writer = b.finish().unwrap();
        let document = document(&[&"large preview ".repeat(10_000)]).document;
        let body=serde_json::to_vec(&json!({"type":"response","status":200,"body":{"result":{"outcome":"publicationPreview","document":document}}})).unwrap();
        assert!(body.len() > 65_535);
        let frames = writer.encrypt(&body).unwrap();
        assert!(frames.len() > 1);
        let mut restored = None;
        for frame in frames {
            let wrapped=json!({"type":"data","channelId":"a".repeat(64),"payload":base64::engine::general_purpose::STANDARD.encode(&frame)}).to_string();
            assert!(wrapped.len() < 90 * 1024);
            if let Some(bytes) = reader.decrypt(&frame).unwrap() {
                restored = Some(bytes);
            }
        }
        assert_eq!(restored.unwrap().as_slice(), body.as_slice());
    }
}
