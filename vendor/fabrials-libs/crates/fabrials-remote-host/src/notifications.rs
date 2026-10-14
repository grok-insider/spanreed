use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use fabrials_agent_host::{
    journal::ReplayOutcome, protocol::Event, remote::RemoteAccess, server::HostState,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{Credentials, bounded_json, opaque};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registration {
    id: String,
    principal: String,
    generation: u64,
    expires_at: u64,
    active: bool,
    retry_revoke_at: u64,
}

#[derive(Default)]
pub(crate) struct Transitions {
    running: BTreeSet<String>,
    permissions: VecDeque<(String, String)>,
}

impl Transitions {
    pub(crate) fn observe(&mut self, event: &Event) -> bool {
        match event {
            Event::PermissionRequest {
                session_id,
                request_id,
                ..
            } => {
                let key = (session_id.clone(), request_id.clone());
                if self.permissions.contains(&key) {
                    return false;
                }
                if self.permissions.len() == 1024 {
                    self.permissions.pop_front();
                }
                self.permissions.push_back(key);
                true
            }
            Event::SessionStatus { session_id, state } => match state.as_str() {
                "running" => {
                    if self.running.len() < 1024 {
                        self.running.insert(session_id.clone());
                    }
                    false
                }
                "idle" => self.running.remove(session_id),
                _ => {
                    self.running.remove(session_id);
                    false
                }
            },
            _ => false,
        }
    }
}

#[derive(Default)]
struct State {
    registrations: BTreeMap<String, Registration>,
    cursor: u64,
    transitions: Transitions,
    attempts: VecDeque<(String, u64)>,
}

pub struct Notifications {
    host: Arc<HostState>,
    host_id: String,
    api: String,
    client: reqwest::Client,
    credentials: Arc<dyn Credentials>,
    directory: PathBuf,
    filename: String,
    require_enabled_config: bool,
    state: Mutex<State>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Operation {
    GetNotificationStatus,
    #[serde(rename_all = "camelCase")]
    ConfigureNotifications {
        id: String,
        binding_token: String,
        generation: u64,
    },
    DisableNotifications,
}

impl Notifications {
    pub async fn new(
        host: Arc<HostState>,
        directory: PathBuf,
        host_id: String,
        client: reqwest::Client,
        credentials: Arc<dyn Credentials>,
    ) -> Result<Self, String> {
        Self::with_api(
            host,
            directory,
            host_id,
            client,
            credentials,
            crate::API,
            true,
        )
        .await
    }

    pub async fn with_loopback_api(
        host: Arc<HostState>,
        directory: PathBuf,
        host_id: String,
        client: reqwest::Client,
        credentials: Arc<dyn Credentials>,
        api: &str,
    ) -> Result<Self, String> {
        let url = reqwest::Url::parse(api).map_err(|_| "invalid_notification_api")?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("invalid_notification_api".into());
        }
        Self::with_api(host, directory, host_id, client, credentials, api, false).await
    }

    async fn with_api(
        host: Arc<HostState>,
        directory: PathBuf,
        host_id: String,
        client: reqwest::Client,
        credentials: Arc<dyn Credentials>,
        api: &str,
        require_enabled_config: bool,
    ) -> Result<Self, String> {
        if !opaque(&host_id) {
            return Err("invalid_notification_host".into());
        }
        let cursor = host.journal.lock().await.emitted_through();
        let filename = format!("remote-notifications-{host_id}.json");
        let path = directory.join(&filename);
        let registrations: BTreeMap<String, Registration> = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.len() > 65_536 {
                    return Err("invalid_notification_state".into());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o077 != 0 {
                        return Err("notification_state_not_private".into());
                    }
                }
                serde_json::from_slice(
                    &std::fs::read(path).map_err(|_| "notification_state_unreadable")?,
                )
                .map_err(|_| "invalid_notification_state")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(_) => return Err("notification_state_unreadable".into()),
        };
        if registrations.len() > 64
            || registrations.iter().any(|(id, row)| {
                id != &row.id
                    || !opaque(id)
                    || !opaque(&row.principal)
                    || row.generation == 0
                    || row.generation > 9_007_199_254_740_991
            })
        {
            return Err("invalid_notification_state".into());
        }
        Ok(Self {
            host,
            host_id,
            api: api.trim_end_matches('/').into(),
            client,
            credentials,
            directory,
            filename,
            require_enabled_config,
            state: Mutex::new(State {
                cursor,
                registrations,
                ..State::default()
            }),
        })
    }

    fn persist(&self, state: &State) -> Result<(), String> {
        crate::state::write_private_json(&self.directory, &self.filename, &state.registrations)
    }

    async fn request(
        &self,
        route: &str,
        body: Value,
        principal: Option<&str>,
    ) -> Result<reqwest::Response, String> {
        let token = tokio::time::timeout(Duration::from_secs(5), self.credentials.access_token())
            .await
            .map_err(|_| "notification_login_unavailable")?
            .map_err(|_| "notification_login_unavailable")?;
        if principal.is_some() && self.host.is_shutting_down() {
            return Err("notification_host_stopping".into());
        }
        if principal.is_some()
            && self.require_enabled_config
            && !crate::state::load(&self.directory)
                .is_ok_and(|config| config.enabled && config.host_id == self.host_id)
        {
            return Err("notification_host_disabled".into());
        }
        if route == "events" {
            let state = self.state.lock().await;
            let id = body.get("id").and_then(Value::as_str).unwrap_or("");
            if !state.registrations.get(id).is_some_and(|row| {
                row.active
                    && Some(row.principal.as_str()) == principal
                    && Some(row.generation) == body.get("generation").and_then(Value::as_u64)
                    && row.expires_at > fabrials_agent_host::now_ms()
            }) {
                return Err("notification_disabled".into());
            }
        }
        if let Some(principal) = principal
            && !self
                .host
                .pairing
                .lock()
                .await
                .has_session(principal, fabrials_agent_host::now_ms())
        {
            return Err("notification_device_revoked".into());
        }
        self.client
            .post(format!("{}/push/{route}", self.api))
            .bearer_auth(&*token)
            .json(&body)
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .map_err(|_| "notification_outcome_uncertain".into())
    }

    async fn revoke(&self, principal: &str, generation: u64) -> Result<(), String> {
        let response = self
            .request(
                "devices/revoke",
                json!({"deviceId":principal,"hostId":self.host_id,"generation":generation}),
                None,
            )
            .await?;
        if response.status().is_success() || response.status().as_u16() == 410 {
            Ok(())
        } else {
            Err("notification_revoke_pending".into())
        }
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
            .is_none_or(|id| !opaque(id))
        {
            return (400, json!({"error":"invalid_request"}));
        }
        let Ok(operation) = serde_json::from_value::<Operation>(body["operation"].clone()) else {
            return (400, json!({"error":"invalid_notification_request"}));
        };
        if !matches!(operation, Operation::GetNotificationStatus)
            && body
                .get("idempotencyKey")
                .and_then(Value::as_str)
                .is_none_or(|id| !opaque(id))
        {
            return (400, json!({"error":"idempotency_key_required"}));
        }
        let mut state = self.state.lock().await;
        let principal = access.session_id();
        let now = fabrials_agent_host::now_ms();
        match operation {
            Operation::GetNotificationStatus => {
                let registrations: Vec<_> = state.registrations.values().filter(|row| row.principal == principal && row.active && row.expires_at > now).map(|row| json!({"id":row.id,"expiresAt":row.expires_at,"generation":row.generation})).collect();
                (
                    200,
                    json!({"result":{"outcome":"notificationStatus","enabled":true,"registrations":registrations}}),
                )
            }
            Operation::DisableNotifications => {
                state
                    .registrations
                    .retain(|_, row| row.principal != principal);
                if self.persist(&state).is_err() {
                    return (503, json!({"error":"notification_state_write_failed"}));
                }
                (200, json!({"result":{"outcome":"acknowledged"}}))
            }
            Operation::ConfigureNotifications {
                id,
                binding_token,
                generation,
            } => {
                if !opaque(&id)
                    || !opaque(&binding_token)
                    || generation == 0
                    || generation > 9_007_199_254_740_991
                {
                    return (400, json!({"error":"invalid_notification_request"}));
                }
                if let Some(row) = state.registrations.get(&id) {
                    return if row.principal == principal
                        && row.generation == generation
                        && row.active
                        && row.expires_at > now
                    {
                        (
                            200,
                            json!({"result":{"outcome":"notificationsConfigured","id":id,"active":true,"expiresAt":row.expires_at,"generation":row.generation}}),
                        )
                    } else {
                        (409, json!({"error":"notification_registration_conflict"}))
                    };
                }
                if state.registrations.len() >= 64 {
                    return (429, json!({"error":"notification_limit"}));
                }
                let response = match self.request(&format!("subscriptions/{id}/bind"), json!({"bindingToken":binding_token,"deviceId":principal,"hostId":self.host_id,"generation":generation}), Some(principal)).await {
                    Ok(response) => response,
                    Err(error) => return (503, json!({"error":error})),
                };
                if !response.status().is_success() {
                    return (422, json!({"error":"notification_binding_rejected"}));
                }
                let reply = match bounded_json(response, 4096).await {
                    Ok(reply) => reply,
                    Err(_) => return (503, json!({"error":"notification_outcome_uncertain"})),
                };
                let expires_at = reply.get("expiresAt").and_then(Value::as_u64).unwrap_or(0);
                if reply.get("id").and_then(Value::as_str) != Some(id.as_str())
                    || reply.get("generation").and_then(Value::as_u64) != Some(generation)
                    || reply.get("active").and_then(Value::as_bool) != Some(true)
                    || expires_at <= now
                    || expires_at > now.saturating_add(31 * 24 * 60 * 60 * 1000)
                {
                    return (503, json!({"error":"invalid_notification_response"}));
                }
                if access.verify_access().await.is_err() {
                    state.registrations.insert(
                        id.clone(),
                        Registration {
                            id,
                            principal: principal.into(),
                            generation,
                            expires_at,
                            active: false,
                            retry_revoke_at: 0,
                        },
                    );
                    let _ = self.persist(&state);
                    return (403, json!({"error":"revoked"}));
                }
                state.registrations.insert(
                    id.clone(),
                    Registration {
                        id: id.clone(),
                        principal: principal.into(),
                        generation,
                        expires_at,
                        active: true,
                        retry_revoke_at: 0,
                    },
                );
                if self.persist(&state).is_err() {
                    state.registrations.get_mut(&id).unwrap().active = false;
                    return (503, json!({"error":"notification_state_write_failed"}));
                }
                (
                    200,
                    json!({"result":{"outcome":"notificationsConfigured","id":id,"active":true,"expiresAt":expires_at,"generation":generation}}),
                )
            }
        }
    }

    pub async fn sync(&self) -> Result<(), String> {
        let mut state = self.state.lock().await;
        let previous = state.registrations.clone();
        let now = fabrials_agent_host::now_ms();
        let mut revoked = BTreeSet::new();
        for row in state.registrations.values_mut() {
            let pairing_revoked = !self
                .host
                .pairing
                .lock()
                .await
                .has_session(&row.principal, now);
            if pairing_revoked {
                row.active = false;
            }
            if pairing_revoked && row.retry_revoke_at <= now {
                revoked.insert((row.principal.clone(), row.generation));
                row.retry_revoke_at = now.saturating_add(5_000);
            }
        }
        state.registrations.retain(|_, row| row.expires_at > now);
        let replay = self.host.journal.lock().await.replay_after(state.cursor);
        let events = match replay {
            ReplayOutcome::Replay(events) => events,
            ReplayOutcome::SnapshotRequired => {
                state.cursor = self.host.journal.lock().await.emitted_through();
                state.transitions = Transitions::default();
                Vec::new()
            }
        };
        let mut changed = false;
        for envelope in events {
            state.cursor = envelope.event_sequence;
            changed |= state.transitions.observe(&envelope.event);
        }
        if state.registrations != previous {
            self.persist(&state)?;
        }
        let rows: Vec<_> = if changed {
            state
                .registrations
                .values()
                .filter(|row| row.active)
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        drop(state);
        futures_util::stream::iter(revoked)
            .for_each_concurrent(4, |(principal, generation)| async move {
                if self.revoke(&principal, generation).await.is_ok() {
                    let mut state = self.state.lock().await;
                    state.registrations.retain(|_, row| {
                        row.principal != principal || row.generation != generation
                    });
                    let _ = self.persist(&state);
                }
            })
            .await;
        if rows.is_empty() {
            return Ok(());
        }
        let identity =
            fabrials_remote_noise::identity().map_err(|_| "notification_entropy_unavailable")?;
        let event_id = crate::state::hex(&identity.private[..16]);
        futures_util::stream::iter(rows).for_each_concurrent(4, |row| {
            let event_id = &event_id;
            async move {
                let mut state = self.state.lock().await;
                let now = fabrials_agent_host::now_ms();
                state.attempts.retain(|(_, at)| now.saturating_sub(*at) < 60_000);
                if !state.registrations.get(&row.id).is_some_and(|current| current == &row && current.active && current.expires_at > now)
                    || state.attempts.len() >= 60
                    || state.attempts.iter().filter(|(id, _)| id == &row.id).count() >= 30 { return; }
                state.attempts.push_back((row.id.clone(), now));
                drop(state);
                let response = self.request("events", json!({"id":row.id,"deviceId":row.principal,"hostId":self.host_id,"generation":row.generation,"eventId":event_id}), Some(&row.principal)).await;
                if response.is_ok_and(|response| matches!(response.status().as_u16(), 403 | 410)) {
                    let mut state = self.state.lock().await;
                    if state.registrations.get(&row.id) == Some(&row) {
                        state.registrations.remove(&row.id);
                        let _ = self.persist(&state);
                    }
                }
            }
        }).await;
        Ok(())
    }

    pub async fn run(&self) {
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        loop {
            tokio::select! {
                _ = self.host.shutdown_requested() => break,
                _ = tick.tick() => { let _ = self.sync().await; }
            }
        }
    }
}

pub fn configured() -> bool {
    matches!(
        std::env::var("SPANREED_REMOTE_EXPERIMENT_PUSH").as_deref(),
        Ok("1")
    )
}

pub(crate) fn handles(kind: &str) -> bool {
    matches!(
        kind,
        "getNotificationStatus" | "configureNotifications" | "disableNotifications"
    )
}

#[cfg(test)]
#[path = "notification_tests.rs"]
mod tests;

#[async_trait::async_trait]
pub trait NotificationService: Send + Sync {
    async fn dispatch(&self, access: &RemoteAccess, body: Value) -> (u16, Value);
    async fn run(&self);
}

#[async_trait::async_trait]
impl NotificationService for Notifications {
    async fn dispatch(&self, access: &RemoteAccess, body: Value) -> (u16, Value) {
        Notifications::dispatch(self, access, body).await
    }
    async fn run(&self) {
        Notifications::run(self).await;
    }
}
