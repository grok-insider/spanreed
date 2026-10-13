use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fabrials_agent_host::{remote::RemoteAccess, server::HostState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::Mutex;
use web_push_native::{
    Auth, WebPushBuilder,
    jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256KeyPair},
    p256::PublicKey,
};
use zeroize::Zeroizing;

const RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const LEGACY_FILE: &str = "direct-notifications.json";
const REGISTRATIONS_FILE: &str = "direct-notifications-v2.json";
const JOB_TTL_MS: u64 = 15 * 60_000;
const KEY_FILE: &str = "direct-vapid.json";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Keys {
    p256dh: String,
    auth: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Subscription {
    endpoint: String,
    #[serde(default)]
    expiration_time: Option<u64>,
    keys: Keys,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registration {
    subscription: Subscription,
    expires_at: u64,
    #[serde(default)]
    generation: u64,
    #[serde(default)]
    after_sequence: u64,
    #[serde(default)]
    last_error: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Operation {
    GetDirectNotificationStatus,
    ConfigureDirectNotifications { subscription: Subscription },
    DisableDirectNotifications,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Job {
    id: String,
    generation: u64,
    sequence: u64,
    created_at: u64,
    expires_at: u64,
    attempts: u8,
    next_attempt: u64,
    in_flight: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    schema: u8,
    registrations: BTreeMap<String, Registration>,
    cursor: u64,
    jobs: BTreeMap<String, Job>,
    attempts: VecDeque<(String, u64)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            schema: 2,
            registrations: BTreeMap::new(),
            cursor: 0,
            jobs: BTreeMap::new(),
            attempts: VecDeque::new(),
        }
    }
}

fn generation() -> Result<u64, String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| "push_entropy_unavailable")?;
    Ok(u64::from_le_bytes(bytes).max(1))
}

fn notification_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "push_entropy_unavailable")?;
    Ok(format!("{:032x}", u128::from_le_bytes(bytes)))
}

fn retry_delay(job: &Job) -> u64 {
    let schedule = [0u64, 5_000, 15_000, 45_000, 120_000, 300_000];
    let base = schedule[usize::from(job.attempts).min(schedule.len() - 1)];
    let jitter = job.id.bytes().fold(u64::from(job.attempts), |a, byte| {
        a.wrapping_mul(31).wrapping_add(u64::from(byte))
    }) % 401;
    base.saturating_mul(800 + jitter) / 1000
}

impl State {
    fn validate(&self) -> Result<(), String> {
        if self.schema != 2
            || self.registrations.len() > 64
            || self.jobs.len() > 128
            || self.attempts.len() > 60
            || self.registrations.iter().any(|(id, row)| {
                !crate::opaque(id) || row.generation == 0 || validate(&row.subscription, 0).is_err()
            })
            || self
                .attempts
                .iter()
                .any(|(principal, _)| !crate::opaque(principal))
            || self.jobs.iter().any(|(principal, job)| {
                self.registrations
                    .get(principal)
                    .is_none_or(|row| row.generation != job.generation)
                    || job.id.len() != 32
                    || !job.id.bytes().all(|byte| byte.is_ascii_hexdigit())
                    || job.sequence == 0
                    || job.sequence > self.cursor
                    || job.attempts > 6
                    || job.expires_at <= job.created_at
                    || job.expires_at - job.created_at > JOB_TTL_MS
                    || job.next_attempt < job.created_at
            })
            || serde_json::to_vec(&self.jobs)
                .map_err(|_| "invalid_notification_state")?
                .len()
                > 256 * 1024
        {
            return Err("invalid_notification_state".into());
        }
        Ok(())
    }

    fn ingest(&mut self, activity: Option<(u64, u64)>, now: u64) -> Result<(), String> {
        for job in self.jobs.values_mut().filter(|job| job.in_flight) {
            job.in_flight = false;
            job.next_attempt = now.saturating_add(retry_delay(job));
        }
        self.attempts
            .retain(|(_, at)| now.saturating_sub(*at) < 60_000);
        self.jobs.retain(|principal, job| {
            job.expires_at > now
                && job.attempts < 6
                && self
                    .registrations
                    .get(principal)
                    .is_some_and(|row| row.generation == job.generation)
        });
        let Some((sequence, created_at)) = activity else {
            if self.cursor != 0 {
                return Err("notification_activity_regressed".into());
            }
            return Ok(());
        };
        if sequence < self.cursor {
            return Err("notification_activity_regressed".into());
        }
        if sequence == self.cursor {
            return Ok(());
        }
        self.cursor = sequence;
        if created_at.saturating_add(JOB_TTL_MS) <= now {
            return Ok(());
        }
        for (principal, row) in &self.registrations {
            if row.expires_at <= now || sequence <= row.after_sequence {
                continue;
            }
            if let Some(job) = self.jobs.get_mut(principal) {
                job.sequence = sequence;
                job.id = notification_id()?;
            } else {
                self.jobs.insert(
                    principal.clone(),
                    Job {
                        id: notification_id()?,
                        generation: row.generation,
                        sequence,
                        created_at,
                        expires_at: created_at.saturating_add(JOB_TTL_MS),
                        attempts: 0,
                        next_attempt: now.max(created_at),
                        in_flight: false,
                    },
                );
            }
        }
        self.validate()
    }
}

pub struct DirectNotifications {
    host: Weak<HostState>,
    directory: PathBuf,
    key: Zeroizing<Vec<u8>>,
    public_key: String,
    contact: String,
    client: reqwest::Client,
    state: Mutex<State>,
    operation: Mutex<()>,
    #[cfg(test)]
    delivery_url: Option<reqwest::Url>,
}

pub(crate) fn private_read(
    directory: &Path,
    filename: &str,
    limit: u64,
) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    let path = directory.join(filename);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("notification_state_unavailable".into()),
    };
    if !metadata.is_file() || metadata.len() > limit {
        return Err("invalid_notification_state".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("notification_state_not_private".into());
        }
    }
    #[cfg(windows)]
    if !fabrials_agent_host::win_acl::is_owner_only(&path)
        .map_err(|_| "notification_state_not_private")?
    {
        return Err("notification_state_not_private".into());
    }
    use std::io::Read;
    let mut bytes = Zeroizing::new(Vec::new());
    std::fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|_| "notification_state_unavailable")?;
    if bytes.len() as u64 > limit {
        return Err("invalid_notification_state".into());
    }
    Ok(Some(bytes))
}

fn endpoint(value: &str) -> Result<reqwest::Url, &'static str> {
    let url = reqwest::Url::parse(value).map_err(|_| "invalid_push_endpoint")?;
    if value.len() > 4096
        || url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.as_str() != value
        || !matches!(
            url.host_str(),
            Some(
                "fcm.googleapis.com"
                    | "updates.push.services.mozilla.com"
                    | "wns2-bl2p.notify.windows.com"
                    | "wns2-db5p.notify.windows.com"
            )
        )
    {
        return Err("unsupported_push_provider");
    }
    Ok(url)
}

fn validate(subscription: &Subscription, now: u64) -> Result<(), &'static str> {
    endpoint(&subscription.endpoint)?;
    if subscription
        .expiration_time
        .is_some_and(|expires| expires <= now)
        || subscription.keys.p256dh.len() > 100
        || subscription.keys.auth.len() > 30
    {
        return Err("invalid_push_subscription");
    }
    let public = URL_SAFE_NO_PAD
        .decode(&subscription.keys.p256dh)
        .map_err(|_| "invalid_push_subscription")?;
    let auth = URL_SAFE_NO_PAD
        .decode(&subscription.keys.auth)
        .map_err(|_| "invalid_push_subscription")?;
    if public.len() != 65
        || public[0] != 4
        || auth.len() != 16
        || PublicKey::from_sec1_bytes(&public).is_err()
    {
        return Err("invalid_push_subscription");
    }
    Ok(())
}

fn public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(value) => {
            let [a, b, c, _] = value.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || a == 169 && b == 254
                || a == 172 && (16..=31).contains(&b)
                || a == 192 && b == 168
                || a == 100 && (64..=127).contains(&b)
                || a == 192 && b == 0
                || a == 198 && (b == 18 || b == 19)
                || a == 198 && b == 51 && c == 100
                || a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(value) => {
            let segments = value.segments();
            segments[0] & 0xe000 == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
        }
    }
}

struct PublicDns;
impl reqwest::dns::Resolve for PublicDns {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 443))
                .await?
                .filter(|address| public_address(address.ip()))
                .take(16)
                .collect();
            if addresses.is_empty() {
                return Err(std::io::Error::other("push DNS returned no public address").into());
            }
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

impl DirectNotifications {
    pub async fn new(
        host: &Arc<HostState>,
        directory: &Path,
        contact: &str,
    ) -> Result<Self, String> {
        fabrials_agent_host::origin::validate_private_origin(contact)
            .map_err(|_| "invalid_push_contact")?;
        let key = match private_read(directory, KEY_FILE, 1024)? {
            Some(bytes) => Zeroizing::new(
                serde_json::from_slice::<Vec<u8>>(&bytes).map_err(|_| "invalid_vapid_key")?,
            ),
            None => {
                let mut key = Zeroizing::new(vec![0; 32]);
                getrandom::fill(&mut key).map_err(|_| "push_entropy_unavailable")?;
                ES256KeyPair::from_bytes(&key).map_err(|_| "invalid_vapid_key")?;
                crate::state::write_private_json(directory, KEY_FILE, &*key)?;
                key
            }
        };
        if key.len() != 32 {
            return Err("invalid_vapid_key".into());
        }
        let key_pair = ES256KeyPair::from_bytes(&key).map_err(|_| "invalid_vapid_key")?;
        let public_key =
            URL_SAFE_NO_PAD.encode(key_pair.public_key().public_key().to_bytes_uncompressed());
        let activity = host
            .journal
            .lock()
            .await
            .notification_activity()
            .map_err(|_| "notification_activity_unavailable")?;
        let now = fabrials_agent_host::now_ms();
        let mut state: State = match private_read(directory, REGISTRATIONS_FILE, 1024 * 1024)? {
            Some(bytes) => {
                serde_json::from_slice(&bytes).map_err(|_| "invalid_notification_state")?
            }
            None => {
                let mut registrations: BTreeMap<String, Registration> =
                    match private_read(directory, LEGACY_FILE, 512 * 1024)? {
                        Some(bytes) => serde_json::from_slice(&bytes)
                            .map_err(|_| "invalid_notification_state")?,
                        None => BTreeMap::new(),
                    };
                for row in registrations.values_mut() {
                    row.generation = generation()?;
                    row.after_sequence = activity.map_or(0, |(sequence, _)| sequence);
                }
                State {
                    registrations,
                    cursor: activity.map_or(0, |(sequence, _)| sequence),
                    ..State::default()
                }
            }
        };
        state.validate()?;
        for job in state.jobs.values_mut().filter(|job| job.in_flight) {
            job.in_flight = false;
            job.next_attempt = now.saturating_add(retry_delay(job));
        }
        crate::state::write_private_json(directory, REGISTRATIONS_FILE, &state)?;
        if directory.join(LEGACY_FILE).exists() {
            std::fs::remove_file(directory.join(LEGACY_FILE))
                .map_err(|_| "notification_legacy_cleanup_failed")?;
        }
        let client = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(5))
            .dns_resolver(Arc::new(PublicDns))
            .pool_max_idle_per_host(2)
            .build()
            .map_err(|_| "push_client_unavailable")?;
        Ok(Self {
            host: Arc::downgrade(host),
            directory: directory.into(),
            key,
            public_key,
            contact: contact.into(),
            client,
            state: Mutex::new(state),
            operation: Mutex::new(()),
            #[cfg(test)]
            delivery_url: None,
        })
    }

    fn persist(&self, state: &State) -> Result<(), String> {
        state.validate()?;
        if serde_json::to_vec(state)
            .map_err(|_| "invalid_notification_state")?
            .len()
            > 1024 * 1024
        {
            return Err("notification_state_limit".into());
        }
        crate::state::write_private_json(&self.directory, REGISTRATIONS_FILE, state)
    }

    fn message(
        &self,
        subscription: &Subscription,
        job: &Job,
    ) -> Result<reqwest::Request, &'static str> {
        let remaining = job.expires_at.saturating_sub(fabrials_agent_host::now_ms()) / 1000;
        if remaining == 0 {
            return Err("notification_expired");
        }
        validate(subscription, fabrials_agent_host::now_ms())?;
        let public = PublicKey::from_sec1_bytes(
            &URL_SAFE_NO_PAD
                .decode(&subscription.keys.p256dh)
                .map_err(|_| "invalid_push_subscription")?,
        )
        .map_err(|_| "invalid_push_subscription")?;
        let auth: [u8; 16] = URL_SAFE_NO_PAD
            .decode(&subscription.keys.auth)
            .map_err(|_| "invalid_push_subscription")?
            .try_into()
            .map_err(|_| "invalid_push_subscription")?;
        let key = ES256KeyPair::from_bytes(&self.key).map_err(|_| "invalid_vapid_key")?;
        let request = WebPushBuilder::new(
            subscription
                .endpoint
                .parse()
                .map_err(|_| "invalid_push_endpoint")?,
            public,
            Auth::from(auth),
        )
        .with_valid_duration(Duration::from_secs(remaining.min(60)))
        .with_vapid(&key, &self.contact)
        .build(serde_json::to_vec(&json!({"version":1,"kind":"remoteActivity","notificationId":job.id,"sequence":job.sequence,"expiresAt":job.expires_at})).map_err(|_| "push_payload_invalid")?)
        .map_err(|_| "push_encryption_failed")?;
        let (parts, body) = request.into_parts();
        self.client
            .post(subscription.endpoint.as_str())
            .headers(parts.headers)
            .header("Urgency", "normal")
            .header("Topic", "spanreed-activity")
            .body(body)
            .build()
            .map_err(|_| "push_request_unavailable")
    }

    async fn dispatch_inner(&self, access: &RemoteAccess, body: Value) -> (u16, Value) {
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
            return (400, json!({"error":"invalid_request"}));
        }
        let Ok(operation) = serde_json::from_value::<Operation>(body["operation"].clone()) else {
            return (400, json!({"error":"invalid_notification_request"}));
        };
        if !matches!(operation, Operation::GetDirectNotificationStatus)
            && body
                .get("idempotencyKey")
                .and_then(Value::as_str)
                .is_none_or(|id| !crate::opaque(id))
        {
            return (400, json!({"error":"idempotency_key_required"}));
        }
        let now = fabrials_agent_host::now_ms();
        let principal = access.session_id();
        let mut state = self.state.lock().await;
        match operation {
            Operation::GetDirectNotificationStatus => {
                let row = state
                    .registrations
                    .get(principal)
                    .filter(|row| row.expires_at > now);
                (
                    200,
                    json!({"result":{"outcome":"directNotificationStatus","enabled":true,"applicationServerKey":self.public_key,"configured":row.is_some(),"expiresAt":row.map(|row|row.expires_at),"subscriptionEndpoint":row.map(|row|row.subscription.endpoint.as_str()),"deliveryPending":state.jobs.contains_key(principal),"lastDeliveryError":row.and_then(|row|row.last_error.as_deref())}}),
                )
            }
            Operation::DisableDirectNotifications => {
                let mut next = state.clone();
                next.registrations.remove(principal);
                next.jobs.remove(principal);
                if self.persist(&next).is_err() {
                    return (503, json!({"error":"notification_state_write_failed"}));
                }
                *state = next;
                (200, json!({"result":{"outcome":"acknowledged"}}))
            }
            Operation::ConfigureDirectNotifications { subscription } => {
                if let Err(error) = validate(&subscription, now) {
                    return (400, json!({"error":error}));
                }
                if let Some(existing) = state
                    .registrations
                    .get(principal)
                    .filter(|row| row.subscription == subscription && row.expires_at > now)
                {
                    return (
                        200,
                        json!({"result":{"outcome":"directNotificationsConfigured","expiresAt":existing.expires_at}}),
                    );
                }
                if !state.registrations.contains_key(principal) && state.registrations.len() >= 64 {
                    return (429, json!({"error":"notification_limit"}));
                }
                let expires_at = subscription
                    .expiration_time
                    .unwrap_or(u64::MAX)
                    .min(now.saturating_add(RETENTION_MS));
                let after_sequence = match self.host.upgrade() {
                    Some(host) => match host.journal.lock().await.notification_activity() {
                        Ok(value) => value.map_or(0, |(sequence, _)| sequence),
                        Err(_) => {
                            return (503, json!({"error":"notification_activity_unavailable"}));
                        }
                    },
                    None => return (503, json!({"error":"host_stopped"})),
                };
                let generation = match generation() {
                    Ok(value) => value,
                    Err(_) => return (503, json!({"error":"push_entropy_unavailable"})),
                };
                let mut next = state.clone();
                next.jobs.remove(principal);
                next.registrations.insert(
                    principal.into(),
                    Registration {
                        subscription,
                        expires_at,
                        generation,
                        after_sequence,
                        last_error: None,
                    },
                );
                if self.persist(&next).is_err() {
                    return (503, json!({"error":"notification_state_write_failed"}));
                }
                *state = next;
                (
                    200,
                    json!({"result":{"outcome":"directNotificationsConfigured","expiresAt":expires_at}}),
                )
            }
        }
    }

    pub async fn sync(&self) -> Result<(), String> {
        let Ok(_single) = self.operation.try_lock() else {
            return Ok(());
        };
        let Some(host) = self.host.upgrade() else {
            return Ok(());
        };
        if host.is_shutting_down() {
            return Ok(());
        }
        let now = fabrials_agent_host::now_ms();
        let activity = host
            .journal
            .lock()
            .await
            .notification_activity()
            .map_err(|_| "notification_activity_unavailable")?;
        let mut state = self.state.lock().await;
        let mut next = state.clone();
        let paired = host.pairing.lock().await;
        next.registrations
            .retain(|principal, row| row.expires_at > now && paired.has_session(principal, now));
        drop(paired);
        next.ingest(activity, now)?;
        if next != *state {
            self.persist(&next)?;
            *state = next;
        }
        let rows: Vec<_> = state
            .jobs
            .iter()
            .filter(|(_, job)| !job.in_flight && job.next_attempt <= now)
            .map(|(principal, job)| (principal.clone(), job.id.clone()))
            .collect();
        drop(state);
        use futures_util::StreamExt;
        let results: Vec<_> = futures_util::stream::iter(rows)
            .map(|(principal, id)| {
                let host = &host;
                async move { self.send_one(host, &principal, &id).await }
            })
            .buffer_unordered(4)
            .collect()
            .await;
        for result in results {
            result?;
        }
        Ok(())
    }

    async fn send_one(&self, host: &HostState, principal: &str, id: &str) -> Result<(), String> {
        let now = fabrials_agent_host::now_ms();
        let mut state = self.state.lock().await;
        let Some(mut job) = state
            .jobs
            .get(principal)
            .filter(|job| job.id == id && !job.in_flight)
            .cloned()
        else {
            return Ok(());
        };
        let Some(row) = state.registrations.get(principal).cloned() else {
            return Ok(());
        };
        if row.generation != job.generation
            || row.expires_at <= now
            || job.expires_at <= now
            || job.attempts >= 6
            || job.next_attempt > now
            || !host.pairing.lock().await.has_session(principal, now)
        {
            return Ok(());
        }
        state
            .attempts
            .retain(|(_, at)| now.saturating_sub(*at) < 60_000);
        if state.attempts.len() >= 60
            || state
                .attempts
                .iter()
                .filter(|(id, _)| id == principal)
                .count()
                >= 10
        {
            return Ok(());
        }
        let request = match self.message(&row.subscription, &job) {
            Ok(request) => request,
            Err(error) => {
                let mut next = state.clone();
                next.jobs.remove(principal);
                if let Some(row) = next.registrations.get_mut(principal) {
                    row.last_error = Some(error.into());
                }
                self.persist(&next)?;
                *state = next;
                return Ok(());
            }
        };
        job.attempts += 1;
        job.in_flight = true;
        let mut next = state.clone();
        next.jobs.insert(principal.into(), job.clone());
        next.attempts.push_back((principal.into(), now));
        self.persist(&next)?;
        *state = next;
        drop(state);
        #[cfg(test)]
        let request = {
            let mut request = request;
            if let Some(url) = &self.delivery_url {
                *request.url_mut() = url.clone();
            }
            request
        };
        let response = self.client.execute(request).await;
        let mut state = self.state.lock().await;
        if state
            .jobs
            .get(principal)
            .is_none_or(|active| active.id != id || active.generation != job.generation)
            || state
                .registrations
                .get(principal)
                .is_none_or(|active| active.generation != job.generation)
        {
            return Ok(());
        }
        let mut next = state.clone();
        let status = response
            .as_ref()
            .ok()
            .map(|response| response.status().as_u16());
        if matches!(status, Some(404 | 410)) {
            next.registrations.remove(principal);
            next.jobs.remove(principal);
        } else if status.is_some_and(|code| (200..300).contains(&code)) {
            next.jobs.remove(principal);
            if let Some(row) = next.registrations.get_mut(principal) {
                row.last_error = None;
            }
        } else {
            let retry = status.is_none_or(|code| matches!(code, 408 | 425 | 429 | 500..=599));
            let now = fabrials_agent_host::now_ms();
            let retry_after = response
                .as_ref()
                .ok()
                .and_then(|response| response.headers().get("Retry-After"))
                .and_then(|header| header.to_str().ok())
                .and_then(|value| {
                    value
                        .parse::<u64>()
                        .ok()
                        .map(|seconds| seconds.saturating_mul(1000))
                        .or_else(|| {
                            httpdate::parse_http_date(value)
                                .ok()?
                                .duration_since(std::time::UNIX_EPOCH)
                                .ok()
                                .map(|at| {
                                    (at.as_millis().min(u64::MAX as u128) as u64)
                                        .saturating_sub(now)
                                })
                        })
                })
                .unwrap_or(0);
            job.in_flight = false;
            job.next_attempt = now.saturating_add(retry_delay(&job).max(retry_after));
            if !retry || job.attempts >= 6 || job.next_attempt >= job.expires_at {
                next.jobs.remove(principal);
            } else {
                next.jobs.insert(principal.into(), job);
            }
            if let Some(row) = next.registrations.get_mut(principal) {
                row.last_error = Some(
                    if retry {
                        "push_delivery_unconfirmed"
                    } else {
                        "push_provider_rejected"
                    }
                    .into(),
                );
            }
        }
        self.persist(&next)?;
        *state = next;
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::notifications::NotificationService for DirectNotifications {
    async fn dispatch(&self, access: &RemoteAccess, body: Value) -> (u16, Value) {
        self.dispatch_inner(access, body).await
    }
    async fn run(&self) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            let Some(host) = self.host.upgrade() else {
                break;
            };
            tokio::select! {
                _ = host.shutdown_requested() => break,
                _ = tick.tick() => { let _ = self.sync().await; }
            }
        }
    }
}

pub(crate) fn handles(kind: &str) -> bool {
    matches!(
        kind,
        "getDirectNotificationStatus"
            | "configureDirectNotifications"
            | "disableDirectNotifications"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::NotificationService;

    fn subscription() -> (Subscription, web_push_native::p256::SecretKey) {
        let secret = web_push_native::p256::SecretKey::from_slice(&[7; 32]).unwrap();
        (
            Subscription {
                endpoint: "https://fcm.googleapis.com/fcm/send/synthetic".into(),
                expiration_time: None,
                keys: Keys {
                    p256dh: URL_SAFE_NO_PAD.encode(secret.public_key().to_sec1_bytes()),
                    auth: URL_SAFE_NO_PAD.encode([9; 16]),
                },
            },
            secret,
        )
    }

    fn envelope(operation: Value) -> Value {
        json!({"protocolVersion":2,"requestId":"request","idempotencyKey":"operation","operation":operation})
    }

    #[test]
    fn endpoint_validation_refuses_redirect_credentials_local_hosts_and_lookalikes() {
        assert!(endpoint("https://fcm.googleapis.com/fcm/send/id").is_ok());
        for value in [
            "http://fcm.googleapis.com/id",
            "https://fcm.googleapis.com.attacker.example/id",
            "https://127.0.0.1/id",
            "https://user@fcm.googleapis.com/id",
            "https://fcm.googleapis.com:8443/id",
            "https://fcm.googleapis.com/id#secret",
            "https://unknown.notify.windows.com/id",
        ] {
            assert!(endpoint(value).is_err(), "{value}");
        }
        for value in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "192.168.1.1",
            "198.18.0.1",
            "0.0.0.0",
            "::1",
            "::ffff:8.8.8.8",
            "fc00::1",
            "2001:db8::1",
        ] {
            assert!(!public_address(value.parse().unwrap()), "{value}");
        }
        assert!(public_address("8.8.8.8".parse().unwrap()));
        assert!(public_address("2607:f8b0::1".parse().unwrap()));
    }

    #[tokio::test]
    async fn vapid_is_stable_payload_is_encrypted_and_revoked_devices_are_removed() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("state");
        let host = Arc::new(HostState::new(
            fabrials_agent_host::origin::LocalOrigin::new(
                "0123456789abcdef0123456789abcdef",
                21001,
            )
            .unwrap(),
        ));
        let service = DirectNotifications::new(&host, &directory, "https://host.example")
            .await
            .unwrap();
        let again = DirectNotifications::new(&host, &directory, "https://host.example")
            .await
            .unwrap();
        assert_eq!(service.public_key, again.public_key);
        let nonce = host
            .pairing
            .lock()
            .await
            .mint_nonce(fabrials_agent_host::now_ms())
            .unwrap();
        let (access, _) = RemoteAccess::pair(host.clone(), nonce.expose())
            .await
            .unwrap();
        let (subscription, private) = subscription();
        let configured = service
            .dispatch(
                &access,
                envelope(
                    json!({"kind":"configureDirectNotifications","subscription":subscription}),
                ),
            )
            .await;
        assert_eq!(configured.0, 200);
        let expiry = configured.1["result"]["expiresAt"].as_u64().unwrap();
        assert!(expiry <= fabrials_agent_host::now_ms() + RETENTION_MS);
        assert_eq!(
            configured,
            service
                .dispatch(
                    &access,
                    envelope(
                        json!({"kind":"configureDirectNotifications","subscription":subscription})
                    )
                )
                .await
        );
        let job = Job {
            id: "a".repeat(32),
            generation: 1,
            sequence: 1,
            created_at: fabrials_agent_host::now_ms(),
            expires_at: fabrials_agent_host::now_ms() + JOB_TTL_MS,
            attempts: 0,
            next_attempt: fabrials_agent_host::now_ms(),
            in_flight: false,
        };
        let request = service.message(&subscription, &job).unwrap();
        assert_eq!(request.headers()["TTL"], "60");
        assert_eq!(request.headers()["content-encoding"], "aes128gcm");
        assert!(
            request.headers()["authorization"]
                .to_str()
                .unwrap()
                .starts_with("vapid t=")
        );
        let encrypted = request.body().unwrap().as_bytes().unwrap();
        assert!(
            !encrypted
                .windows(b"remoteActivity".len())
                .any(|bytes| bytes == b"remoteActivity")
        );
        let plaintext =
            web_push_native::decrypt(encrypted.to_vec(), &private, &Auth::from([9; 16])).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&plaintext).unwrap(),
            json!({"version":1,"kind":"remoteActivity","notificationId":job.id,"sequence":job.sequence,"expiresAt":job.expires_at})
        );
        host.pairing.lock().await.revoke(access.session_id());
        service.sync().await.unwrap();
        assert!(service.state.lock().await.registrations.is_empty());
        assert_eq!(
            service
                .dispatch(
                    &access,
                    envelope(json!({"kind":"getDirectNotificationStatus"}))
                )
                .await
                .0,
            403
        );
        let restarted = DirectNotifications::new(&host, &directory, "https://host.example")
            .await
            .unwrap();
        assert!(restarted.state.lock().await.registrations.is_empty());
    }

    #[tokio::test]
    async fn configuration_requires_valid_keys_version_and_idempotency() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("state");
        let host = Arc::new(HostState::new(
            fabrials_agent_host::origin::LocalOrigin::new(
                "0123456789abcdef0123456789abcdef",
                21001,
            )
            .unwrap(),
        ));
        let service = DirectNotifications::new(&host, &directory, "https://host.example")
            .await
            .unwrap();
        let nonce = host
            .pairing
            .lock()
            .await
            .mint_nonce(fabrials_agent_host::now_ms())
            .unwrap();
        let (access, _) = RemoteAccess::pair(host.clone(), nonce.expose())
            .await
            .unwrap();
        let (subscription, _) = subscription();
        let mut request =
            envelope(json!({"kind":"configureDirectNotifications","subscription":subscription}));
        request["protocolVersion"] = json!(999);
        assert_eq!(service.dispatch(&access, request.clone()).await.0, 400);
        request["protocolVersion"] = json!(2);
        request.as_object_mut().unwrap().remove("idempotencyKey");
        assert_eq!(service.dispatch(&access, request.clone()).await.0, 400);
        request["idempotencyKey"] = json!("operation");
        request["operation"]["subscription"]["keys"]["p256dh"] = json!("bad");
        assert_eq!(service.dispatch(&access, request).await.0, 400);
        assert!(service.state.lock().await.registrations.is_empty());
    }
    struct Provider {
        responses: Mutex<VecDeque<(u16, Option<u64>)>>,
        bodies: Mutex<Vec<Vec<u8>>>,
    }

    async fn fixture() -> (
        tempfile::TempDir,
        Arc<HostState>,
        DirectNotifications,
        RemoteAccess,
        Arc<Provider>,
        tokio::task::JoinHandle<()>,
    ) {
        use axum::{
            Router,
            body::Bytes,
            extract::State as ProviderState,
            http::{HeaderMap, StatusCode},
            routing::post,
        };
        async fn provider(
            ProviderState(state): ProviderState<Arc<Provider>>,
            body: Bytes,
        ) -> (StatusCode, HeaderMap) {
            state.bodies.lock().await.push(body.to_vec());
            let (status, after) = state
                .responses
                .lock()
                .await
                .pop_front()
                .unwrap_or((201, None));
            let mut headers = HeaderMap::new();
            if let Some(after) = after {
                headers.insert("Retry-After", after.to_string().parse().unwrap());
            }
            (StatusCode::from_u16(status).unwrap(), headers)
        }
        let temp = tempfile::tempdir().unwrap();
        let host = Arc::new(HostState::new(
            fabrials_agent_host::origin::LocalOrigin::new(
                "0123456789abcdef0123456789abcdef",
                21001,
            )
            .unwrap(),
        ));
        let mut service =
            DirectNotifications::new(&host, &temp.path().join("state"), "https://host.example")
                .await
                .unwrap();
        let nonce = host
            .pairing
            .lock()
            .await
            .mint_nonce(fabrials_agent_host::now_ms())
            .unwrap();
        let (access, _) = RemoteAccess::pair(host.clone(), nonce.expose())
            .await
            .unwrap();
        assert_eq!(service.dispatch(&access, envelope(json!({"kind":"configureDirectNotifications","subscription":subscription().0}))).await.0, 200);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        service.delivery_url = Some(
            format!("http://{}/provider", listener.local_addr().unwrap())
                .parse()
                .unwrap(),
        );
        service.client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let provider_state = Arc::new(Provider {
            responses: Mutex::new(VecDeque::new()),
            bodies: Mutex::new(Vec::new()),
        });
        let router = Router::new()
            .route("/provider", post(provider))
            .with_state(provider_state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (temp, host, service, access, provider_state, server)
    }

    async fn activity(host: &HostState) {
        let mut journal = host.journal.lock().await;
        for state in ["running", "idle"] {
            journal.record_event(
                fabrials_agent_host::protocol::Event::SessionStatus {
                    session_id: "synthetic".into(),
                    state: state.into(),
                },
                None,
            );
        }
    }

    #[tokio::test]
    async fn failed_delivery_survives_restart_with_same_id_and_attempt_budget() {
        let (temp, host, service, access, provider, server) = fixture().await;
        provider.responses.lock().await.push_back((503, None));
        activity(&host).await;
        service.sync().await.unwrap();
        let original = service.state.lock().await.jobs[access.session_id()].clone();
        assert_eq!(original.attempts, 1);
        assert_eq!(provider.bodies.lock().await.len(), 1);
        let mut restarted =
            DirectNotifications::new(&host, &temp.path().join("state"), "https://host.example")
                .await
                .unwrap();
        restarted.client = service.client.clone();
        restarted.delivery_url = service.delivery_url.clone();
        assert_eq!(
            restarted.state.lock().await.jobs[access.session_id()],
            original
        );
        assert_eq!(restarted.state.lock().await.attempts.len(), 1);
        restarted
            .state
            .lock()
            .await
            .jobs
            .get_mut(access.session_id())
            .unwrap()
            .next_attempt = fabrials_agent_host::now_ms();
        restarted.sync().await.unwrap();
        assert!(restarted.state.lock().await.jobs.is_empty());
        assert_eq!(restarted.state.lock().await.attempts.len(), 2);
        let (_, private) = subscription();
        let bodies = provider.bodies.lock().await;
        assert_eq!(bodies.len(), 2);
        for body in bodies.iter() {
            let plain =
                web_push_native::decrypt(body.clone(), &private, &Auth::from([9; 16])).unwrap();
            let payload: Value = serde_json::from_slice(&plain).unwrap();
            assert_eq!(payload["notificationId"], original.id);
            assert_eq!(payload["expiresAt"], original.expires_at);
            assert!(!String::from_utf8(plain).unwrap().contains("synthetic"));
        }
        server.abort();
    }

    #[tokio::test]
    async fn disable_and_revocation_remove_pending_jobs_before_another_send() {
        for revoke in [false, true] {
            let (_temp, host, service, access, provider, server) = fixture().await;
            provider.responses.lock().await.push_back((503, None));
            activity(&host).await;
            service.sync().await.unwrap();
            assert!(!service.state.lock().await.jobs.is_empty());
            if revoke {
                host.pairing.lock().await.revoke(access.session_id());
            } else {
                assert_eq!(
                    service
                        .dispatch(
                            &access,
                            envelope(json!({"kind":"disableDirectNotifications"}))
                        )
                        .await
                        .0,
                    200
                );
            }
            activity(&host).await;
            service.sync().await.unwrap();
            assert!(service.state.lock().await.jobs.is_empty());
            assert!(service.state.lock().await.registrations.is_empty());
            assert_eq!(provider.bodies.lock().await.len(), 1);
            server.abort();
        }
    }

    #[tokio::test]
    async fn retry_after_and_attempt_limit_are_bounded_by_fixed_expiry() {
        let (_temp, host, service, access, provider, server) = fixture().await;
        provider.responses.lock().await.push_back((429, Some(600)));
        activity(&host).await;
        let before = fabrials_agent_host::now_ms();
        service.sync().await.unwrap();
        let expiry = {
            let state = service.state.lock().await;
            let job = &state.jobs[access.session_id()];
            assert!(job.next_attempt >= before + 600_000);
            job.expires_at
        };
        for attempt in 2..=6 {
            provider.responses.lock().await.push_back((503, None));
            service
                .state
                .lock()
                .await
                .jobs
                .get_mut(access.session_id())
                .unwrap()
                .next_attempt = fabrials_agent_host::now_ms();
            service.sync().await.unwrap();
            let state = service.state.lock().await;
            if attempt < 6 {
                assert_eq!(state.jobs[access.session_id()].expires_at, expiry);
            }
        }
        assert!(service.state.lock().await.jobs.is_empty());
        assert_eq!(provider.bodies.lock().await.len(), 6);
        activity(&host).await;
        provider.responses.lock().await.push_back((429, Some(3600)));
        service.sync().await.unwrap();
        assert!(service.state.lock().await.jobs.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn gone_subscription_is_deleted_and_new_events_coalesce_without_extending_lifetime() {
        let (_temp, host, service, access, provider, server) = fixture().await;
        provider.responses.lock().await.push_back((503, None));
        activity(&host).await;
        service.sync().await.unwrap();
        let old = service.state.lock().await.jobs[access.session_id()].clone();
        activity(&host).await;
        service.sync().await.unwrap();
        let new = service.state.lock().await.jobs[access.session_id()].clone();
        assert_eq!(new.expires_at, old.expires_at);
        assert_eq!(new.attempts, old.attempts);
        assert!(new.sequence > old.sequence);
        assert_ne!(new.id, old.id);
        provider.responses.lock().await.push_back((410, None));
        service
            .state
            .lock()
            .await
            .jobs
            .get_mut(access.session_id())
            .unwrap()
            .next_attempt = fabrials_agent_host::now_ms();
        service.sync().await.unwrap();
        assert!(service.state.lock().await.registrations.is_empty());
        assert!(service.state.lock().await.jobs.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn ambiguous_inflight_attempt_recovers_after_restart_without_resetting_budget() {
        let (temp, host, service, access, _provider, server) = fixture().await;
        activity(&host).await;
        let mut state = service.state.lock().await;
        state
            .ingest(
                host.journal.lock().await.notification_activity().unwrap(),
                fabrials_agent_host::now_ms(),
            )
            .unwrap();
        let job = state.jobs.get_mut(access.session_id()).unwrap();
        job.in_flight = true;
        job.attempts = 3;
        let id = job.id.clone();
        service.persist(&state).unwrap();
        drop(state);
        let restarted =
            DirectNotifications::new(&host, &temp.path().join("state"), "https://host.example")
                .await
                .unwrap();
        let state = restarted.state.lock().await;
        let job = &state.jobs[access.session_id()];
        assert_eq!(job.id, id);
        assert_eq!(job.attempts, 3);
        assert!(!job.in_flight);
        assert!(job.next_attempt > fabrials_agent_host::now_ms());
        server.abort();
    }
    #[tokio::test]
    async fn global_and_registration_rate_limits_survive_restart() {
        for per_registration in [false, true] {
            let (temp, host, service, access, provider, server) = fixture().await;
            activity(&host).await;
            let now = fabrials_agent_host::now_ms();
            let mut state = service.state.lock().await;
            state
                .ingest(
                    host.journal.lock().await.notification_activity().unwrap(),
                    now,
                )
                .unwrap();
            for index in 0..if per_registration { 10 } else { 60 } {
                state.attempts.push_back((
                    if per_registration {
                        access.session_id().into()
                    } else {
                        format!("other-{index}")
                    },
                    now,
                ));
            }
            service.persist(&state).unwrap();
            drop(state);
            let mut restarted =
                DirectNotifications::new(&host, &temp.path().join("state"), "https://host.example")
                    .await
                    .unwrap();
            restarted.client = service.client.clone();
            restarted.delivery_url = service.delivery_url.clone();
            restarted.sync().await.unwrap();
            assert!(provider.bodies.lock().await.is_empty());
            assert_eq!(
                restarted.state.lock().await.jobs[access.session_id()].attempts,
                0
            );
            server.abort();
        }
    }

    #[tokio::test]
    async fn expired_jobs_are_removed_before_contacting_the_provider() {
        let (_temp, host, service, access, provider, server) = fixture().await;
        activity(&host).await;
        let now = fabrials_agent_host::now_ms();
        let mut state = service.state.lock().await;
        state
            .ingest(
                host.journal.lock().await.notification_activity().unwrap(),
                now,
            )
            .unwrap();
        let job = state.jobs.get_mut(access.session_id()).unwrap();
        job.created_at = now - JOB_TTL_MS - 1;
        job.expires_at = now - 1;
        job.next_attempt = job.created_at;
        service.persist(&state).unwrap();
        drop(state);
        service.sync().await.unwrap();
        assert!(service.state.lock().await.jobs.is_empty());
        assert!(provider.bodies.lock().await.is_empty());
        server.abort();
    }
}
