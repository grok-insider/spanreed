use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fabrials_agent_host::{journal::ReplayOutcome, remote::RemoteAccess, server::HostState};
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
const REGISTRATIONS_FILE: &str = "direct-notifications.json";
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
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Operation {
    GetDirectNotificationStatus,
    ConfigureDirectNotifications { subscription: Subscription },
    DisableDirectNotifications,
}

#[derive(Default)]
struct State {
    registrations: BTreeMap<String, Registration>,
    cursor: u64,
    transitions: crate::notifications::Transitions,
    attempts: VecDeque<(String, u64)>,
}

pub struct DirectNotifications {
    host: Weak<HostState>,
    directory: PathBuf,
    key: Zeroizing<Vec<u8>>,
    public_key: String,
    contact: String,
    client: reqwest::Client,
    state: Mutex<State>,
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
        let registrations: BTreeMap<String, Registration> =
            match private_read(directory, REGISTRATIONS_FILE, 512 * 1024)? {
                Some(bytes) => {
                    serde_json::from_slice(&bytes).map_err(|_| "invalid_notification_state")?
                }
                None => BTreeMap::new(),
            };
        if registrations.len() > 64
            || registrations
                .iter()
                .any(|(id, row)| !crate::opaque(id) || validate(&row.subscription, 0).is_err())
        {
            return Err("invalid_notification_state".into());
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
            state: Mutex::new(State {
                registrations,
                cursor: host.journal.lock().await.emitted_through(),
                ..State::default()
            }),
        })
    }

    fn persist(&self, registrations: &BTreeMap<String, Registration>) -> Result<(), String> {
        crate::state::write_private_json(&self.directory, REGISTRATIONS_FILE, registrations)
    }

    fn message(&self, subscription: &Subscription) -> Result<reqwest::Request, &'static str> {
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
        .with_valid_duration(Duration::from_secs(60))
        .with_vapid(&key, &self.contact)
        .build(br#"{"version":1,"kind":"remoteActivity"}"#.to_vec())
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
        if body.get("protocolVersion").and_then(Value::as_u64) != Some(2)
            || body
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
                    json!({"result":{"outcome":"directNotificationStatus","enabled":true,"applicationServerKey":self.public_key,"configured":row.is_some(),"expiresAt":row.map(|row|row.expires_at),"subscriptionEndpoint":row.map(|row|row.subscription.endpoint.as_str())}}),
                )
            }
            Operation::DisableDirectNotifications => {
                let mut next = state.registrations.clone();
                next.remove(principal);
                if self.persist(&next).is_err() {
                    return (503, json!({"error":"notification_state_write_failed"}));
                }
                state.registrations = next;
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
                let mut next = state.registrations.clone();
                next.insert(
                    principal.into(),
                    Registration {
                        subscription,
                        expires_at,
                    },
                );
                if self.persist(&next).is_err() {
                    return (503, json!({"error":"notification_state_write_failed"}));
                }
                state.registrations = next;
                (
                    200,
                    json!({"result":{"outcome":"directNotificationsConfigured","expiresAt":expires_at}}),
                )
            }
        }
    }

    pub async fn sync(&self) -> Result<(), String> {
        let Some(host) = self.host.upgrade() else {
            return Ok(());
        };
        if host.is_shutting_down() {
            return Ok(());
        }
        let now = fabrials_agent_host::now_ms();
        let mut state = self.state.lock().await;
        let old = state.registrations.clone();
        let paired = host.pairing.lock().await;
        state
            .registrations
            .retain(|principal, row| row.expires_at > now && paired.has_session(principal, now));
        drop(paired);
        if state.registrations != old
            && let Err(error) = self.persist(&state.registrations)
        {
            state.registrations = old;
            return Err(error);
        }
        let mut changed = false;
        let replay = host.journal.lock().await.replay_after(state.cursor);
        match replay {
            ReplayOutcome::Replay(events) => {
                for envelope in events {
                    state.cursor = envelope.event_sequence;
                    changed |= state.transitions.observe(&envelope.event);
                }
            }
            ReplayOutcome::SnapshotRequired => {
                state.cursor = host.journal.lock().await.emitted_through();
                state.transitions = crate::notifications::Transitions::default();
            }
        }
        if !changed {
            return Ok(());
        }
        state
            .attempts
            .retain(|(_, at)| now.saturating_sub(*at) < 60_000);
        let rows = state.registrations.clone();
        drop(state);
        use futures_util::StreamExt;
        futures_util::stream::iter(rows)
            .for_each_concurrent(4, |(principal, row)| {
                let host = &host;
                async move {
                    let mut state = self.state.lock().await;
                    if state.registrations.get(&principal) != Some(&row)
                        || row.expires_at <= fabrials_agent_host::now_ms()
                        || state.attempts.len() >= 60
                        || state
                            .attempts
                            .iter()
                            .filter(|(id, _)| id == &principal)
                            .count()
                            >= 10
                        || !host
                            .pairing
                            .lock()
                            .await
                            .has_session(&principal, fabrials_agent_host::now_ms())
                    {
                        return;
                    }
                    let Ok(request) = self.message(&row.subscription) else {
                        return;
                    };
                    state.attempts.push_back((principal.clone(), now));
                    drop(state);
                    let response = self.client.execute(request).await;
                    if response
                        .is_ok_and(|response| matches!(response.status().as_u16(), 404 | 410))
                    {
                        let mut state = self.state.lock().await;
                        if state.registrations.get(&principal) == Some(&row) {
                            state.registrations.remove(&principal);
                            let _ = self.persist(&state.registrations);
                        }
                    }
                }
            })
            .await;
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
        let request = service.message(&subscription).unwrap();
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
            json!({"version":1,"kind":"remoteActivity"})
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
}
