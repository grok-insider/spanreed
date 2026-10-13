use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::lease::ControlLease;

const MAX_CONNECTIONS: usize = 64;
const TRANSFER_TTL_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlTransferRequest {
    pub request_id: String,
    pub session_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDevice {
    pub session_id: String,
    pub current_browser: bool,
    pub current_controller: bool,
    pub connected: bool,
    pub connection_count: usize,
    pub last_seen: Option<u64>,
}

#[derive(Debug)]
struct PendingTransfer {
    projection: ControlTransferRequest,
    connection: String,
}

#[derive(Debug, Default)]
pub(crate) struct Connections {
    active: BTreeMap<String, String>,
    pending: BTreeMap<String, PendingTransfer>,
}

impl Connections {
    pub(crate) fn register(&mut self, session: &str, connection: &str) -> Result<(), &'static str> {
        if self.active.len() >= MAX_CONNECTIONS && !self.active.contains_key(connection) {
            return Err("connection_limit");
        }
        self.active.insert(connection.into(), session.into());
        Ok(())
    }

    pub(crate) fn disconnect(&mut self, connection: &str) {
        self.active.remove(connection);
        self.pending
            .retain(|_, value| value.connection != connection);
    }

    pub(crate) fn is_connected(&self, session: &str, connection: &str) -> bool {
        self.active
            .get(connection)
            .is_some_and(|owner| owner == session)
    }

    pub(crate) fn revoke(&mut self, session: Option<&str>) {
        self.active
            .retain(|_, owner| session.is_some_and(|id| owner != id));
        self.pending
            .retain(|_, value| session.is_some_and(|id| value.projection.session_id != id));
    }

    pub(crate) fn requests(&mut self, now: u64) -> Vec<ControlTransferRequest> {
        self.pending
            .retain(|_, value| value.projection.expires_at > now);
        self.pending
            .values()
            .map(|value| value.projection.clone())
            .collect()
    }

    pub(crate) fn request(
        &mut self,
        session: &str,
        connection: &str,
        now: u64,
    ) -> Result<ControlTransferRequest, &'static str> {
        self.requests(now);
        if !self.is_connected(session, connection) {
            return Err("connection_required");
        }
        if let Some(existing) = self
            .pending
            .values()
            .find(|value| value.connection == connection)
        {
            return Ok(existing.projection.clone());
        }
        let request_id = crate::pairing::random_hex()
            .map_err(|_| "entropy_unavailable")?
            .expose()
            .to_owned();
        let projection = ControlTransferRequest {
            request_id: request_id.clone(),
            session_id: session.into(),
            expires_at: now.saturating_add(TRANSFER_TTL_MS),
        };
        self.pending.insert(
            request_id,
            PendingTransfer {
                projection: projection.clone(),
                connection: connection.into(),
            },
        );
        Ok(projection)
    }

    pub(crate) fn cancel(
        &mut self,
        session: &str,
        connection: &str,
        request: &str,
    ) -> Result<(), &'static str> {
        let value = self.pending.get(request).ok_or("unknown_control_request")?;
        if value.projection.session_id != session || value.connection != connection {
            return Err("control_request_owner_required");
        }
        self.pending.remove(request);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn approve(
        &mut self,
        lease: &mut ControlLease,
        principal: &str,
        connection: &str,
        epoch: u64,
        request: &str,
        live_sessions: &[String],
        now: u64,
    ) -> Result<(), &'static str> {
        self.requests(now);
        lease
            .authorize_connection(principal, connection, epoch, now)
            .map_err(|_| "controller_required")?;
        let target = self.pending.get(request).ok_or("unknown_control_request")?;
        if !live_sessions.contains(&target.projection.session_id)
            || !self.is_connected(&target.projection.session_id, &target.connection)
        {
            return Err("control_target_unavailable");
        }
        lease
            .transfer(
                principal,
                connection,
                epoch,
                &target.projection.session_id,
                &target.connection,
                now,
            )
            .map_err(|_| "controller_required")?;
        self.pending.clear();
        Ok(())
    }

    pub(crate) fn devices(
        &self,
        live_sessions: &[String],
        principal: &str,
        controller: Option<&str>,
    ) -> Vec<BrowserDevice> {
        live_sessions
            .iter()
            .map(|id| {
                let count = self.active.values().filter(|owner| *owner == id).count();
                BrowserDevice {
                    session_id: id.clone(),
                    current_browser: id == principal,
                    current_controller: controller == Some(id),
                    connected: count != 0,
                    connection_count: count,
                    last_seen: None,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_is_connection_bound_single_use_and_invalidates_old_epoch() {
        let mut connections = Connections::default();
        connections.register("a", "a-tab").unwrap();
        connections.register("b", "b-tab").unwrap();
        connections.register("b", "b-other").unwrap();
        let request = connections.request("b", "b-tab", 100).unwrap();
        assert_eq!(connections.request("b", "b-tab", 101).unwrap(), request);
        assert!(
            connections
                .cancel("b", "b-other", &request.request_id)
                .is_err()
        );
        let mut lease = ControlLease::new();
        let epoch = lease.acquire("a", "a-tab", 100).unwrap();
        let live = vec!["a".into(), "b".into()];
        assert!(
            connections
                .approve(
                    &mut lease,
                    "a",
                    "wrong-tab",
                    epoch,
                    &request.request_id,
                    &live,
                    101
                )
                .is_err()
        );
        connections
            .approve(
                &mut lease,
                "a",
                "a-tab",
                epoch,
                &request.request_id,
                &live,
                102,
            )
            .unwrap();
        assert!(lease.authorize("a-tab", epoch, 103).is_err());
        assert!(lease.authorize("b-other", epoch + 1, 103).is_err());
        assert!(lease.authorize("b-tab", epoch + 1, 103).is_ok());
        assert!(
            connections
                .approve(
                    &mut lease,
                    "a",
                    "a-tab",
                    epoch,
                    &request.request_id,
                    &live,
                    104
                )
                .is_err()
        );
    }

    #[test]
    fn expiry_disconnect_and_revoke_remove_transfer_authority() {
        let mut connections = Connections::default();
        let mut lease = ControlLease::new();
        let live = vec!["a".into(), "b".into()];
        for reason in ["expired", "disconnected", "revoked"] {
            lease.clear();
            let epoch = lease.acquire("a", "a-tab", 100_000).unwrap();
            connections.register("b", "b-tab").unwrap();
            let request = connections.request("b", "b-tab", 100_000).unwrap();
            let now = if reason == "expired" {
                130_000
            } else {
                100_001
            };
            if reason == "expired" {
                lease.heartbeat("a-tab", 114_000).unwrap();
                lease.heartbeat("a-tab", 128_000).unwrap();
            }
            if reason == "disconnected" {
                connections.disconnect("b-tab");
            }
            if reason == "revoked" {
                connections.revoke(Some("b"));
            }
            assert!(
                connections
                    .approve(
                        &mut lease,
                        "a",
                        "a-tab",
                        epoch,
                        &request.request_id,
                        &live,
                        now
                    )
                    .is_err()
            );
        }
    }

    #[test]
    fn device_projection_never_contains_connection_tokens() {
        let mut connections = Connections::default();
        connections.register("a", "secret-connection").unwrap();
        let result = connections.devices(&["a".into(), "b".into()], "b", Some("a"));
        assert_eq!(result[0].connection_count, 1);
        assert!(!result[1].connected);
        assert!(result[1].current_browser);
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("secret-connection")
        );
    }
}
