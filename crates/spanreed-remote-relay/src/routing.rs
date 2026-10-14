use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

pub const MAX_FRAME: usize = 90 * 1024;
pub const MAX_CIPHERTEXT: usize = 65_535;
pub const MAX_CHANNELS: usize = 64;
pub const QUEUE_SIZE: usize = 4;
pub const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub account_id: String,
    pub host_id: String,
    pub role: Role,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Host,
    Browser,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Incoming {
    Authenticate {
        ticket: String,
    },
    Data {
        #[serde(rename = "channelId")]
        channel_id: String,
        payload: String,
    },
    Closed {
        #[serde(rename = "channelId")]
        channel_id: String,
        #[serde(default, rename = "reason")]
        _reason: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Outgoing {
    Ready {
        #[serde(rename = "channelId", skip_serializing_if = "Option::is_none")]
        channel_id: Option<String>,
    },
    Open {
        #[serde(rename = "channelId")]
        channel_id: String,
    },
    Data {
        #[serde(rename = "channelId")]
        channel_id: String,
        payload: String,
    },
    Closed {
        #[serde(rename = "channelId")]
        channel_id: String,
        reason: &'static str,
    },
}

#[derive(Clone)]
pub struct Peer {
    pub id: Uuid,
    pub tx: mpsc::Sender<Outgoing>,
    pub stop: watch::Sender<bool>,
    pub draining: watch::Sender<Option<Instant>>,
}

impl Peer {
    pub fn send(&self, frame: Outgoing) -> bool {
        if self.tx.try_send(frame).is_err() {
            self.stop.send_replace(true);
            false
        } else {
            true
        }
    }
    pub fn close(&self) {
        self.stop.send_replace(true);
    }
    pub fn is_draining(&self) -> bool {
        self.draining.borrow().is_some()
    }
    pub async fn drain_expired(&self) {
        let mut draining = self.draining.subscribe();
        loop {
            let started = *draining.borrow_and_update();
            if let Some(started) = started {
                tokio::time::sleep_until((started + DRAIN_TIMEOUT).into()).await;
                return;
            }
            if draining.changed().await.is_err() {
                return;
            }
        }
    }
}

pub struct Delivery {
    target: Peer,
    frame: Outgoing,
}
impl Delivery {
    pub async fn send(self) -> Result<(), &'static str> {
        let mut stopped = self.target.stop.subscribe();
        if *stopped.borrow() {
            return Err("peer_closed");
        }
        let result = tokio::select! {
            biased;
            _ = stopped.changed() => return Err("peer_closed"),
            _ = self.target.drain_expired() => { self.target.close(); return Err("backpressure"); },
            result = tokio::time::timeout(Duration::from_secs(5), self.target.tx.send(self.frame)) => result,
        };
        if result.is_ok_and(|sent| sent.is_ok()) {
            Ok(())
        } else {
            self.target.close();
            Err("backpressure")
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct HostKey {
    account: String,
    host: String,
}
impl From<&Identity> for HostKey {
    fn from(identity: &Identity) -> Self {
        Self {
            account: identity.account_id.clone(),
            host: identity.host_id.clone(),
        }
    }
}
struct Host {
    peer: Peer,
    generation: u64,
    browsers: HashMap<String, Peer>,
}

#[derive(Default)]
pub struct Router {
    hosts: HashMap<HostKey, Host>,
    shutting_down: bool,
}

impl Router {
    #[cfg(test)]
    pub fn qualification_stats(&self) -> serde_json::Value {
        let mut browsers = 0;
        let mut queued = 0;
        let mut max_queued = 0;
        for host in self.hosts.values() {
            browsers += host.browsers.len();
            for peer in std::iter::once(&host.peer).chain(host.browsers.values()) {
                let pending = QUEUE_SIZE - peer.tx.capacity();
                queued += pending;
                max_queued = max_queued.max(pending);
            }
        }
        serde_json::json!({
            "hosts": self.hosts.len(),
            "browsers": browsers,
            "queuedFrames": queued,
            "maxQueuedFrames": max_queued,
            "queueCapacity": QUEUE_SIZE,
            "stopping": self.shutting_down,
        })
    }

    pub fn shutdown(&mut self) {
        self.shutting_down = true;
        for (_, host) in self.hosts.drain() {
            host.peer.close();
            for peer in host.browsers.values() {
                peer.close();
            }
        }
    }
    pub fn admit(
        &mut self,
        identity: &Identity,
        peer: Peer,
    ) -> Result<Option<Delivery>, &'static str> {
        if self.shutting_down {
            return Err("shutting_down");
        }
        let key = HostKey::from(identity);
        match identity.role {
            Role::Host => {
                if let Some(old) = self.hosts.get(&key)
                    && old.generation > identity.generation
                {
                    return Err("stale_generation");
                }
                if let Some(old) = self.hosts.remove(&key) {
                    old.peer.close();
                    for browser in old.browsers.values() {
                        browser.close();
                    }
                }
                if !peer.send(Outgoing::Ready { channel_id: None }) {
                    return Err("backpressure");
                }
                self.hosts.insert(
                    key,
                    Host {
                        peer,
                        generation: identity.generation,
                        browsers: HashMap::new(),
                    },
                );
                Ok(None)
            }
            Role::Browser => {
                let host = self.hosts.get_mut(&key).ok_or("host_offline")?;
                if host.generation != identity.generation {
                    return Err("stale_generation");
                }
                if host.browsers.len() >= MAX_CHANNELS {
                    return Err("too_many_channels");
                }
                let channel = Uuid::new_v4().to_string();
                if !peer.send(Outgoing::Ready {
                    channel_id: Some(channel.clone()),
                }) {
                    return Err("backpressure");
                }
                host.browsers.insert(channel.clone(), peer);
                Ok(Some(Delivery {
                    target: host.peer.clone(),
                    frame: Outgoing::Open {
                        channel_id: channel,
                    },
                }))
            }
        }
    }

    #[cfg(test)]
    fn admit_immediate(
        &mut self,
        identity: &Identity,
        peer: Peer,
    ) -> Result<Option<String>, &'static str> {
        let Some(notification) = self.admit(identity, peer)? else {
            return Ok(None);
        };
        let Outgoing::Open { channel_id } = &notification.frame else {
            unreachable!();
        };
        let channel = channel_id.clone();
        if !notification.target.send(notification.frame) {
            return Err("backpressure");
        }
        Ok(Some(channel))
    }

    pub fn delivery(
        &self,
        identity: &Identity,
        peer_id: Uuid,
        channel: &str,
        payload: String,
    ) -> Result<Option<Delivery>, &'static str> {
        if payload.len() > MAX_CIPHERTEXT.div_ceil(3) * 4 {
            return Err("invalid_frame");
        }
        let bytes = STANDARD.decode(&payload).map_err(|_| "invalid_frame")?;
        if bytes.is_empty() || bytes.len() > MAX_CIPHERTEXT {
            return Err("invalid_frame");
        }
        let host = self
            .hosts
            .get(&HostKey::from(identity))
            .ok_or("host_offline")?;
        if host.generation != identity.generation {
            return Err("stale_generation");
        }
        let browser = match host.browsers.get(channel) {
            Some(browser) => browser,
            None if identity.role == Role::Host && host.peer.id == peer_id => return Ok(None),
            None => return Err("unknown_channel"),
        };
        if browser.is_draining() {
            return match identity.role {
                Role::Host if host.peer.id == peer_id => Ok(None),
                _ => Err("unknown_channel"),
            };
        }
        let target = match identity.role {
            Role::Host if host.peer.id == peer_id => browser,
            Role::Browser if browser.id == peer_id => &host.peer,
            _ => return Err("forbidden_channel"),
        };
        Ok(Some(Delivery {
            target: target.clone(),
            frame: Outgoing::Data {
                channel_id: channel.to_owned(),
                payload,
            },
        }))
    }

    #[cfg(test)]
    fn data(
        &self,
        identity: &Identity,
        peer_id: Uuid,
        channel: &str,
        payload: String,
    ) -> Result<(), &'static str> {
        match self.delivery(identity, peer_id, channel, payload)? {
            Some(delivery) => {
                if delivery.target.send(delivery.frame) {
                    Ok(())
                } else {
                    Err("backpressure")
                }
            }
            None => Ok(()),
        }
    }

    pub fn close_channel(
        &mut self,
        identity: &Identity,
        peer_id: Uuid,
        channel: &str,
    ) -> Result<Option<Delivery>, &'static str> {
        let host = self
            .hosts
            .get_mut(&HostKey::from(identity))
            .ok_or("host_offline")?;
        if host.generation != identity.generation {
            return Err("stale_generation");
        }
        let browser = match host.browsers.get(channel) {
            Some(browser) => browser,
            None if identity.role == Role::Host && host.peer.id == peer_id => return Ok(None),
            None => return Err("unknown_channel"),
        };
        if !((identity.role == Role::Host && host.peer.id == peer_id)
            || (identity.role == Role::Browser && browser.id == peer_id))
        {
            return Err("forbidden_channel");
        }
        if identity.role == Role::Host {
            if browser.is_draining() {
                return Ok(None);
            }
            browser.draining.send_replace(Some(Instant::now()));
            return Ok(Some(Delivery {
                target: browser.clone(),
                frame: Outgoing::Closed {
                    channel_id: channel.to_owned(),
                    reason: "peer_closed",
                },
            }));
        }
        if let Some(browser) = host.browsers.remove(channel) {
            browser.close();
            return Ok(Some(Delivery {
                target: host.peer.clone(),
                frame: Outgoing::Closed {
                    channel_id: channel.to_owned(),
                    reason: "peer_closed",
                },
            }));
        }
        Ok(None)
    }

    pub fn disconnect(&mut self, identity: &Identity, peer_id: Uuid) -> Option<Delivery> {
        let key = HostKey::from(identity);
        let host = self.hosts.get_mut(&key)?;
        if host.peer.id == peer_id {
            if let Some(host) = self.hosts.remove(&key) {
                for browser in host.browsers.values() {
                    browser.close();
                }
            }
        } else {
            let channel = host
                .browsers
                .iter()
                .find(|(_, p)| p.id == peer_id)
                .map(|(c, _)| c.clone());
            if let Some(channel) = channel {
                host.browsers.remove(&channel);
                return Some(Delivery {
                    target: host.peer.clone(),
                    frame: Outgoing::Closed {
                        channel_id: channel,
                        reason: "disconnected",
                    },
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity(role: Role) -> Identity {
        Identity {
            account_id: "alice".into(),
            host_id: "host".into(),
            role,
            generation: 1,
        }
    }
    fn peer() -> (Peer, mpsc::Receiver<Outgoing>, watch::Receiver<bool>) {
        let (tx, rx) = mpsc::channel(QUEUE_SIZE);
        let (stop, stopped) = watch::channel(false);
        (
            Peer {
                id: Uuid::new_v4(),
                tx,
                stop,
                draining: watch::channel(None).0,
            },
            rx,
            stopped,
        )
    }
    #[test]
    fn routes_only_its_account_host_and_own_channel() {
        let mut router = Router::default();
        let (h, mut hrx, _) = peer();
        router
            .admit_immediate(&identity(Role::Host), h.clone())
            .unwrap();
        hrx.try_recv().unwrap();
        let (b, mut brx, _) = peer();
        let channel = router
            .admit_immediate(&identity(Role::Browser), b.clone())
            .unwrap()
            .unwrap();
        hrx.try_recv().unwrap();
        brx.try_recv().unwrap();
        let payload = STANDARD.encode([1, 2, 3]);
        router
            .data(&identity(Role::Browser), b.id, &channel, payload.clone())
            .unwrap();
        assert!(matches!(hrx.try_recv().unwrap(), Outgoing::Data { .. }));
        router
            .data(&identity(Role::Host), h.id, &channel, payload.clone())
            .unwrap();
        assert!(matches!(brx.try_recv().unwrap(), Outgoing::Data { .. }));
        assert!(
            router
                .data(
                    &identity(Role::Browser),
                    Uuid::new_v4(),
                    &channel,
                    payload.clone()
                )
                .is_err()
        );
        let mut other = identity(Role::Browser);
        other.account_id = "mallory".into();
        assert!(
            router
                .data(&other, b.id, &channel, payload.clone())
                .is_err()
        );
        other = identity(Role::Browser);
        other.host_id = "other".into();
        assert!(router.data(&other, b.id, &channel, payload).is_err());
    }
    #[test]
    fn reconnect_evicts_old_host_and_browsers_without_old_disconnect_harming_new_host() {
        let mut router = Router::default();
        let (old, mut rx, stopped) = peer();
        router
            .admit_immediate(&identity(Role::Host), old.clone())
            .unwrap();
        rx.try_recv().unwrap();
        let (browser, _brx, browser_stopped) = peer();
        router
            .admit_immediate(&identity(Role::Browser), browser)
            .unwrap();
        let (new, _nrx, new_stopped) = peer();
        router.admit_immediate(&identity(Role::Host), new).unwrap();
        assert!(*stopped.borrow());
        assert!(*browser_stopped.borrow());
        assert!(router.disconnect(&identity(Role::Host), old.id).is_none());
        assert!(!*new_stopped.borrow());
        assert_eq!(router.hosts.len(), 1);
    }
    #[test]
    fn newer_generation_rejects_stale_clients() {
        let mut router = Router::default();
        let mut newer = identity(Role::Host);
        newer.generation = 2;
        let (h, _hrx, _hs) = peer();
        router.admit_immediate(&newer, h).unwrap();
        let (stale, _, _) = peer();
        assert_eq!(
            router.admit_immediate(&identity(Role::Host), stale),
            Err("stale_generation")
        );
        let (browser, _, _) = peer();
        assert_eq!(
            router.admit_immediate(&identity(Role::Browser), browser),
            Err("stale_generation")
        );
    }
    #[test]
    fn bounded_queue_closes_slow_peer_and_rejects_bad_ciphertext() {
        let mut router = Router::default();
        let (h, mut hrx, stopped) = peer();
        router.admit_immediate(&identity(Role::Host), h).unwrap();
        hrx.try_recv().unwrap();
        let (b, _brx, _bs) = peer();
        let channel = router
            .admit_immediate(&identity(Role::Browser), b.clone())
            .unwrap()
            .unwrap();
        hrx.try_recv().unwrap();
        for data in [
            "?".to_owned(),
            String::new(),
            STANDARD.encode(vec![0; MAX_CIPHERTEXT + 1]),
        ] {
            assert_eq!(
                router.data(&identity(Role::Browser), b.id, &channel, data),
                Err("invalid_frame")
            );
        }
        for _ in 0..QUEUE_SIZE {
            router
                .data(
                    &identity(Role::Browser),
                    b.id,
                    &channel,
                    STANDARD.encode([0]),
                )
                .unwrap();
        }
        assert_eq!(
            router.data(
                &identity(Role::Browser),
                b.id,
                &channel,
                STANDARD.encode([0])
            ),
            Err("backpressure")
        );
        assert!(*stopped.borrow());
    }
    #[test]
    fn disconnecting_revoked_host_stops_all_browsers_and_routing() {
        let mut router = Router::default();
        let (h, mut hrx, _hs) = peer();
        router
            .admit_immediate(&identity(Role::Host), h.clone())
            .unwrap();
        hrx.try_recv().unwrap();
        let (b, _brx, stopped) = peer();
        let channel = router
            .admit_immediate(&identity(Role::Browser), b.clone())
            .unwrap()
            .unwrap();
        assert!(router.disconnect(&identity(Role::Host), h.id).is_none());
        assert!(*stopped.borrow());
        assert_eq!(
            router.data(
                &identity(Role::Browser),
                b.id,
                &channel,
                STANDARD.encode([1])
            ),
            Err("host_offline")
        );
    }
    #[test]
    fn channel_limit_bounds_one_host_and_shutdown_closes_every_peer() {
        let mut router = Router::default();
        let (h, mut hrx, host_stopped) = peer();
        router
            .admit_immediate(&identity(Role::Host), h.clone())
            .unwrap();
        hrx.try_recv().unwrap();
        let mut receivers = Vec::new();
        let mut stops = Vec::new();
        for _ in 0..MAX_CHANNELS {
            let (b, brx, stopped) = peer();
            let channel = router
                .admit_immediate(&identity(Role::Browser), b)
                .unwrap()
                .unwrap();
            hrx.try_recv().unwrap();
            router
                .close_channel(&identity(Role::Host), h.id, &channel)
                .unwrap();
            assert!(!*stopped.borrow());
            receivers.push(brx);
            stops.push(stopped);
        }
        let (b, _brx, _bs) = peer();
        assert_eq!(
            router.admit_immediate(&identity(Role::Browser), b),
            Err("too_many_channels")
        );
        router.shutdown();
        assert!(*host_stopped.borrow());
        assert!(stops.iter().all(|s| *s.borrow()));
        assert!(router.hosts.is_empty());
        for role in [Role::Host, Role::Browser] {
            let (late, _rx, _stopped) = peer();
            assert_eq!(
                router.admit_immediate(&identity(role), late),
                Err("shutting_down")
            );
        }
    }
    #[test]
    fn late_host_frames_after_browser_disconnect_are_harmless() {
        let mut router = Router::default();
        let (h, mut hrx, _hs) = peer();
        router
            .admit_immediate(&identity(Role::Host), h.clone())
            .unwrap();
        hrx.try_recv().unwrap();
        let (b, _brx, _bs) = peer();
        let channel = router
            .admit_immediate(&identity(Role::Browser), b.clone())
            .unwrap()
            .unwrap();
        assert!(router.disconnect(&identity(Role::Browser), b.id).is_some());
        assert!(
            router
                .close_channel(&identity(Role::Host), h.id, &channel)
                .is_ok()
        );
        assert!(
            router
                .data(&identity(Role::Host), h.id, &channel, STANDARD.encode([1]))
                .is_ok()
        );
        assert!(
            router
                .close_channel(&identity(Role::Host), Uuid::new_v4(), &channel)
                .is_err()
        );
    }
    #[tokio::test]
    async fn concurrent_admissions_wait_for_host_queue_and_preserve_open_before_data() {
        let mut router = Router::default();
        let (host, mut host_rx, host_stopped) = peer();
        router.admit(&identity(Role::Host), host.clone()).unwrap();
        host_rx.try_recv().unwrap();
        let mut browsers = Vec::new();
        let mut notices = Vec::new();
        for _ in 0..64 {
            let (browser, rx, stopped) = peer();
            notices.push(
                router
                    .admit(&identity(Role::Browser), browser.clone())
                    .unwrap()
                    .unwrap(),
            );
            browsers.push((browser, rx, stopped));
        }
        assert!(!*host_stopped.borrow());
        let receiver = tokio::spawn(async move {
            let mut channels = std::collections::HashSet::new();
            for _ in 0..64 {
                let Some(Outgoing::Open { channel_id }) = host_rx.recv().await else {
                    panic!("expected open notice");
                };
                assert!(channels.insert(channel_id));
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            host_rx
        });
        let outcomes =
            futures_util::future::join_all(notices.into_iter().map(Delivery::send)).await;
        assert!(outcomes.into_iter().all(|outcome| outcome.is_ok()));
        let mut host_rx = receiver.await.unwrap();
        assert!(!*host_stopped.borrow());
        for (browser, mut rx, _) in browsers {
            let Some(Outgoing::Ready {
                channel_id: Some(channel),
            }) = rx.recv().await
            else {
                panic!("expected browser ready");
            };
            router
                .delivery(
                    &identity(Role::Browser),
                    browser.id,
                    &channel,
                    STANDARD.encode([1]),
                )
                .unwrap()
                .unwrap()
                .send()
                .await
                .unwrap();
            assert!(
                matches!(host_rx.recv().await, Some(Outgoing::Data { channel_id, .. }) if channel_id == channel)
            );
        }
    }

    #[tokio::test]
    async fn concurrent_browser_disconnects_wait_for_host_queue_without_closing_it() {
        let mut router = Router::default();
        let (host, mut host_rx, host_stopped) = peer();
        router
            .admit_immediate(&identity(Role::Host), host.clone())
            .unwrap();
        host_rx.try_recv().unwrap();
        let mut browsers = Vec::new();
        for _ in 0..64 {
            let (browser, rx, stopped) = peer();
            router
                .admit_immediate(&identity(Role::Browser), browser.clone())
                .unwrap();
            host_rx.try_recv().unwrap();
            browsers.push((browser, rx, stopped));
        }
        let deliveries: Vec<_> = browsers
            .iter()
            .take(54)
            .map(|(browser, _, _)| {
                router
                    .disconnect(&identity(Role::Browser), browser.id)
                    .unwrap()
            })
            .collect();
        assert!(!*host_stopped.borrow());
        let receiver = tokio::spawn(async move {
            let mut channels = std::collections::HashSet::new();
            for _ in 0..54 {
                let Some(Outgoing::Closed {
                    channel_id,
                    reason: "disconnected",
                }) = host_rx.recv().await
                else {
                    panic!("expected disconnect notice");
                };
                assert!(channels.insert(channel_id));
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            host_rx
        });
        let outcomes =
            futures_util::future::join_all(deliveries.into_iter().map(Delivery::send)).await;
        assert!(outcomes.into_iter().all(|outcome| outcome.is_ok()));
        let _host_rx = receiver.await.unwrap();
        assert!(!*host_stopped.borrow());
        assert_eq!(
            router.hosts[&HostKey::from(&identity(Role::Host))]
                .browsers
                .len(),
            10
        );
    }

    #[tokio::test]
    async fn fragmented_burst_waits_for_slow_receiver_without_growing_queue() {
        let mut router = Router::default();
        let (h, mut hrx, _hs) = peer();
        router
            .admit_immediate(&identity(Role::Host), h.clone())
            .unwrap();
        hrx.try_recv().unwrap();
        let (b, mut brx, stopped) = peer();
        let channel = router
            .admit_immediate(&identity(Role::Browser), b)
            .unwrap()
            .unwrap();
        brx.try_recv().unwrap();
        hrx.try_recv().unwrap();
        let receiver = tokio::spawn(async move {
            for sequence in 0..36u8 {
                tokio::time::sleep(Duration::from_millis(1)).await;
                assert!(brx.len() <= QUEUE_SIZE);
                let Outgoing::Data { payload, .. } = brx.recv().await.unwrap() else {
                    panic!("expected data");
                };
                let decoded = STANDARD.decode(payload).unwrap();
                assert_eq!(decoded.len(), MAX_CIPHERTEXT);
                assert_eq!(decoded[0], sequence);
            }
        });
        for sequence in 0..36u8 {
            let delivery = router
                .delivery(
                    &identity(Role::Host),
                    h.id,
                    &channel,
                    STANDARD.encode(vec![sequence; MAX_CIPHERTEXT]),
                )
                .unwrap()
                .unwrap();
            delivery.send().await.unwrap();
        }
        receiver.await.unwrap();
        assert!(!*stopped.borrow());
    }
    #[test]
    fn browser_cannot_close_another_browser_channel() {
        let mut router = Router::default();
        let (h, mut hrx, _) = peer();
        router.admit_immediate(&identity(Role::Host), h).unwrap();
        hrx.try_recv().unwrap();
        let (a, _arx, _as) = peer();
        router
            .admit_immediate(&identity(Role::Browser), a.clone())
            .unwrap();
        hrx.try_recv().unwrap();
        let (b, _brx, stopped) = peer();
        let channel = router
            .admit_immediate(&identity(Role::Browser), b)
            .unwrap()
            .unwrap();
        assert!(matches!(
            router.close_channel(&identity(Role::Browser), a.id, &channel),
            Err("forbidden_channel")
        ));
        assert!(!*stopped.borrow());
    }

    #[tokio::test]
    async fn host_close_drains_queued_frames_before_the_terminal_frame() {
        let mut router = Router::default();
        let (host, mut host_rx, _) = peer();
        router
            .admit_immediate(&identity(Role::Host), host.clone())
            .unwrap();
        host_rx.try_recv().unwrap();
        let (browser, mut browser_rx, stopped) = peer();
        let channel = router
            .admit_immediate(&identity(Role::Browser), browser.clone())
            .unwrap()
            .unwrap();
        browser_rx.try_recv().unwrap();
        host_rx.try_recv().unwrap();
        for index in 0..QUEUE_SIZE {
            router
                .data(
                    &identity(Role::Host),
                    host.id,
                    &channel,
                    STANDARD.encode([u8::try_from(index).unwrap()]),
                )
                .unwrap();
        }
        let closing = router
            .close_channel(&identity(Role::Host), host.id, &channel)
            .unwrap()
            .unwrap();
        let sending = tokio::spawn(closing.send());
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!sending.is_finished());
        assert!(browser.is_draining());
        let started = *browser.draining.borrow();
        assert_eq!(router.hosts.values().next().unwrap().browsers.len(), 1);
        assert!(
            router
                .close_channel(&identity(Role::Host), host.id, &channel)
                .unwrap()
                .is_none()
        );
        assert_eq!(*browser.draining.borrow(), started);
        assert!(
            router
                .delivery(
                    &identity(Role::Browser),
                    browser.id,
                    &channel,
                    STANDARD.encode([1])
                )
                .is_err()
        );
        assert!(browser.is_draining());
        assert!(!*stopped.borrow());
        for index in 0..QUEUE_SIZE {
            let Outgoing::Data { payload, .. } = browser_rx.recv().await.unwrap() else {
                panic!("ciphertext must precede closure");
            };
            assert_eq!(
                STANDARD.decode(payload).unwrap(),
                [u8::try_from(index).unwrap()]
            );
        }
        sending.await.unwrap().unwrap();
        assert!(matches!(
            browser_rx.recv().await,
            Some(Outgoing::Closed { .. })
        ));
        assert!(!*stopped.borrow());
        assert!(router.disconnect(&identity(Role::Host), host.id).is_none());
        assert!(*stopped.borrow());
    }
}
