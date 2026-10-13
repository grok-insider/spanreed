use crate::pairing::PairingError;
use crate::protocol::CommandEnvelope;
use crate::server::{CONTROLLER_HEADER, CSRF_HEADER, HostState, PairResponse, SESSION_HEADER};
use axum::http::{HeaderMap, HeaderValue};
use std::sync::Arc;

#[derive(Debug, serde::Serialize)]
pub struct RemoteReply {
    pub status: u16,
    pub body: serde_json::Value,
}

pub struct RemoteAccess {
    state: Arc<HostState>,
    token: String,
    csrf: String,
    principal: String,
    connection: String,
    public_key: Option<[u8; 32]>,
    protocol_version: std::sync::atomic::AtomicU32,
}

impl Drop for RemoteAccess {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.token.zeroize();
        self.csrf.zeroize();
        self.connection.zeroize();
    }
}

impl RemoteAccess {
    pub async fn authenticate(
        state: Arc<HostState>,
        token: String,
        csrf: String,
    ) -> Result<Self, PairingError> {
        Self::authenticate_bound(state, token, csrf, None).await
    }

    pub async fn authenticate_device(
        state: Arc<HostState>,
        token: String,
        csrf: String,
        public_key: [u8; 32],
    ) -> Result<Self, PairingError> {
        Self::authenticate_bound(state, token, csrf, Some(public_key)).await
    }

    async fn authenticate_bound(
        state: Arc<HostState>,
        token: String,
        csrf: String,
        public_key: Option<[u8; 32]>,
    ) -> Result<Self, PairingError> {
        let broker = state.pairing.lock().await;
        let principal = match public_key {
            Some(key) => broker.verify_remote(&token, &csrf, key, crate::now_ms())?,
            None => broker.verify_mutation(&token, &csrf, crate::now_ms())?,
        };
        drop(broker);
        let connection = crate::pairing::random_hex()?.expose().to_owned();
        Ok(Self {
            state,
            token,
            csrf,
            principal,
            connection,
            public_key,
            protocol_version: std::sync::atomic::AtomicU32::new(2),
        })
    }

    pub async fn pair(
        state: Arc<HostState>,
        nonce: &str,
    ) -> Result<(Self, PairResponse), PairingError> {
        Self::pair_bound(state, nonce, None).await
    }

    pub async fn pair_device(
        state: Arc<HostState>,
        nonce: &str,
        public_key: [u8; 32],
    ) -> Result<(Self, PairResponse), PairingError> {
        Self::pair_bound(state, nonce, Some(public_key)).await
    }

    async fn pair_bound(
        state: Arc<HostState>,
        nonce: &str,
        public_key: Option<[u8; 32]>,
    ) -> Result<(Self, PairResponse), PairingError> {
        let paired = {
            let mut broker = state.pairing.lock().await;
            let paired = broker.redeem_nonce(nonce, crate::now_ms())?;
            if let Some(key) = public_key {
                broker.bind_remote_session(&paired.session_id, key)?;
            }
            if broker.persist().is_err() {
                broker.revoke(&paired.session_id);
                return Err(PairingError::SessionInvalid);
            }
            paired
        };
        let reply = PairResponse {
            installation_id: Some(state.origin.install_id().to_owned()),
            session_id: paired.session_id,
            session_token: paired.session_token.expose().to_owned(),
            csrf_token: paired.csrf_token.expose().to_owned(),
            protocol_version: crate::protocol::PROTOCOL_VERSION,
        };
        let access = Self::authenticate_bound(
            state,
            reply.session_token.clone(),
            reply.csrf_token.clone(),
            public_key,
        )
        .await?;
        Ok((access, reply))
    }

    async fn verify(&self) -> Result<(), PairingError> {
        let broker = self.state.pairing.lock().await;
        match self.public_key {
            Some(key) => broker.verify_remote(&self.token, &self.csrf, key, crate::now_ms()),
            None => broker.verify_mutation(&self.token, &self.csrf, crate::now_ms()),
        }
        .map(|_| ())
    }

    pub fn session_id(&self) -> &str {
        &self.principal
    }

    pub async fn verify_access(&self) -> Result<(), &'static str> {
        self.verify().await.map_err(|_| "session_invalid")
    }

    pub fn is_multiwriter(&self) -> bool {
        self.state.agent_mode == crate::acp::AgentMode::Shared
    }

    pub async fn authorize_protocol(&self, version: u32) -> Result<(), &'static str> {
        self.verify_access().await?;
        if self.is_multiwriter() && version != crate::protocol::MULTI_WRITER_PROTOCOL_VERSION {
            return Err("client_upgrade_required");
        }
        Ok(())
    }

    pub async fn authorize_control(&self, epoch: u64) -> Result<(), &'static str> {
        self.verify_access().await?;
        if self.is_multiwriter() {
            return self
                .authorize_protocol(
                    self.protocol_version
                        .load(std::sync::atomic::Ordering::Acquire),
                )
                .await;
        }
        self.state
            .lease
            .lock()
            .await
            .authorize(&self.connection, epoch, crate::now_ms())
            .map_err(|_| "controller_required")
    }

    pub async fn connect(&self) -> Result<u64, &'static str> {
        self.connect_version(2).await
    }

    pub async fn connect_version(&self, version: u32) -> Result<u64, &'static str> {
        self.protocol_version
            .store(version, std::sync::atomic::Ordering::Release);
        self.verify().await.map_err(|_| "session_invalid")?;
        self.state
            .remote_connections
            .lock()
            .await
            .register(&self.principal, &self.connection)?;
        if self.is_multiwriter() {
            self.state.notify_remote_devices().await;
            return if version == 4 {
                Ok(0)
            } else {
                Err("controller_held")
            };
        }
        let epoch = self
            .state
            .lease
            .lock()
            .await
            .acquire(&self.principal, &self.connection, crate::now_ms())
            .map_err(|_| "controller_held");
        self.state.notify_remote_devices().await;
        let epoch = epoch?;
        self.state.replay_open_sessions().await;
        self.state.replay_pending_permissions().await;
        Ok(epoch)
    }

    pub fn control_transfer_enabled(&self) -> bool {
        self.state.remote_control_transfer_enabled()
    }

    pub async fn control_epoch(&self) -> Result<Option<u64>, &'static str> {
        self.verify_access().await?;
        Ok(self
            .state
            .lease
            .lock()
            .await
            .connection_epoch(&self.connection, crate::now_ms()))
    }

    pub async fn heartbeat(&self, epoch: u64) -> Result<(), &'static str> {
        self.verify().await.map_err(|_| "session_invalid")?;
        if self.is_multiwriter() {
            return Ok(());
        }
        let mut lease = self.state.lease.lock().await;
        lease
            .authorize(&self.connection, epoch, crate::now_ms())
            .map_err(|_| "controller_required")?;
        lease
            .heartbeat(&self.connection, crate::now_ms())
            .map_err(|_| "controller_required")
    }

    #[allow(missing_docs)]
    pub fn event_notification(&self) -> tokio::sync::futures::Notified<'_> {
        self.state.event_notify.notified()
    }

    pub async fn events(
        &self,
        after_sequence: u64,
    ) -> Result<crate::journal::ReplayOutcome, &'static str> {
        self.verify().await.map_err(|_| "session_invalid")?;
        Ok(self.state.journal.lock().await.replay_after(after_sequence))
    }

    pub async fn snapshot_frames(
        &self,
    ) -> Result<(u64, Vec<crate::protocol::EventEnvelope>), &'static str> {
        self.verify_access().await?;
        Ok(self.state.journal.lock().await.snapshot_frames())
    }

    pub async fn snapshot(&self) -> Result<u64, &'static str> {
        self.verify().await.map_err(|_| "session_invalid")?;
        let before = self.state.journal.lock().await.emitted_through();
        self.state.replay_open_sessions().await;
        self.state.replay_pending_permissions().await;
        Ok(before)
    }

    pub async fn command(&self, envelope: CommandEnvelope) -> RemoteReply {
        if self.verify().await.is_err() {
            return RemoteReply {
                status: 403,
                body: serde_json::json!({"error": "session_invalid"}),
            };
        }
        if envelope.operation.requires_control()
            && !self.is_multiwriter()
            && self
                .authorize_control(envelope.controller_epoch.unwrap_or(0))
                .await
                .is_err()
        {
            return RemoteReply {
                status: 409,
                body: serde_json::json!({"error":"controller_required"}),
            };
        }
        let mut headers = HeaderMap::new();
        let Ok(token) = HeaderValue::from_str(&self.token) else {
            return RemoteReply {
                status: 403,
                body: serde_json::Value::Null,
            };
        };
        let Ok(csrf) = HeaderValue::from_str(&self.csrf) else {
            return RemoteReply {
                status: 403,
                body: serde_json::Value::Null,
            };
        };
        headers.insert(SESSION_HEADER, token);
        headers.insert(CSRF_HEADER, csrf);
        headers.insert(
            CONTROLLER_HEADER,
            HeaderValue::from_str(&self.connection).expect("hex connection token"),
        );
        let body = serde_json::to_vec(&envelope)
            .expect("command serializes")
            .into();
        let response =
            crate::server::command(axum::extract::State(self.state.clone()), headers, body).await;
        let status = response.status().as_u16();
        let body = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(serde_json::Value::Null);
        RemoteReply { status, body }
    }

    pub async fn disconnect(&self) {
        crate::server::release_controller(&self.state, &self.connection).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::AgentMode;
    use crate::protocol::{Operation, PROTOCOL_VERSION};

    fn state() -> Arc<HostState> {
        Arc::new(
            HostState::new(
                crate::origin::LocalOrigin::new("0123456789abcdef0123456789abcdef", 21001).unwrap(),
            )
            .with_agent_mode(AgentMode::Isolated),
        )
    }

    async fn pair(state: &Arc<HostState>) -> (RemoteAccess, PairResponse) {
        let nonce = state
            .pairing
            .lock()
            .await
            .mint_nonce(crate::now_ms())
            .unwrap();
        RemoteAccess::pair(state.clone(), nonce.expose())
            .await
            .unwrap()
    }

    fn command(operation: Operation, epoch: u64, key: &str) -> CommandEnvelope {
        CommandEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: key.into(),
            idempotency_key: Some(key.into()),
            controller_epoch: Some(epoch),
            expected_revision: None,
            deadline_ms: None,
            operation,
        }
    }

    fn shared_command(operation: Operation, epoch: u64, key: &str) -> CommandEnvelope {
        let mut envelope = command(operation, epoch, key);
        envelope.protocol_version = 4;
        envelope
    }

    #[tokio::test]
    async fn remote_experiments_are_disabled_until_host_explicitly_enables_them() {
        let state = Arc::new(
            HostState::new(
                crate::origin::LocalOrigin::new("0123456789abcdef0123456789abcdef", 21001).unwrap(),
            )
            .with_remote_experiments(crate::remote_management::RemoteExperiments::default()),
        );
        let (access, _) = pair(&state).await;
        let capabilities = access
            .command(command(Operation::GetRemoteCapabilities, 0, "capabilities"))
            .await;
        assert_eq!(capabilities.status, 202);
        for name in ["projects", "devicePanel", "controlTransfer"] {
            assert_eq!(capabilities.body["result"][name], false);
        }
        assert!(capabilities.body["result"]["upstreamPrompt"].is_null());
        for operation in [
            Operation::ListRemoteProjects {
                directory_id: None,
                cursor: None,
            },
            Operation::ListBrowserDevices,
            Operation::RequestControlTransfer,
        ] {
            let reply = access.command(command(operation, 0, "disabled")).await;
            assert_eq!(reply.status, 422);
            assert_eq!(reply.body["error"], "remote_experiment_disabled");
        }
    }

    #[tokio::test]
    async fn shared_devices_are_all_writers_without_transfer_requests() {
        let state = Arc::new(
            HostState::new(
                crate::origin::LocalOrigin::new("0123456789abcdef0123456789abcdef", 21001).unwrap(),
            )
            .with_agent_mode(AgentMode::Shared)
            .with_remote_experiments(crate::remote_management::RemoteExperiments {
                control_transfer: true,
                device_panel: true,
                projects: false,
            }),
        );
        let (phone, _) = pair(&state).await;
        let (desktop, _) = pair(&state).await;
        phone.connect_version(4).await.unwrap();
        desktop.connect_version(4).await.unwrap();
        assert!(!phone.control_transfer_enabled());
        for operation in [
            Operation::RequestControlTransfer,
            Operation::ApproveControlTransfer {
                request_id: "retired".into(),
            },
            Operation::CancelControlTransfer {
                request_id: "retired".into(),
            },
        ] {
            let reply = phone.command(shared_command(operation, 0, "retired")).await;
            assert_eq!(reply.status, 422);
        }
        let listed = desktop
            .command(shared_command(Operation::ListBrowserDevices, 0, "devices"))
            .await;
        assert_eq!(
            listed.body["result"]["devices"].as_array().unwrap().len(),
            2
        );
        assert_eq!(phone.authorize_control(0).await, Ok(()));
        assert_eq!(desktop.authorize_control(0).await, Ok(()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn remote_enrollment_requires_local_root_current_handle_and_controller() {
        let persisted = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("project")).unwrap();
        let state = Arc::new(
            HostState::new(
                crate::origin::LocalOrigin::new("0123456789abcdef0123456789abcdef", 21001).unwrap(),
            )
            .with_agent_mode(AgentMode::Isolated)
            .with_remote_experiments(crate::remote_management::RemoteExperiments {
                projects: true,
                ..Default::default()
            })
            .with_persistence(
                persisted.path(),
                Arc::new(crate::picker::UnavailableDirectoryPicker),
            )
            .unwrap(),
        );
        let (owner, _) = pair(&state).await;
        let (observer, _) = pair(&state).await;
        let epoch = owner.connect().await.unwrap();
        let empty = owner
            .command(command(
                Operation::ListRemoteProjects {
                    directory_id: None,
                    cursor: None,
                },
                epoch,
                "empty-roots",
            ))
            .await;
        assert!(
            empty.body["result"]["entries"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let granted = crate::control::execute(
            crate::control::ControlRequest::AuthorizeRemoteRoot {
                path: root.path().to_path_buf(),
            },
            &state,
        )
        .await;
        let crate::control::ControlResponse::RemoteRoots { roots } = granted else {
            panic!("local grant failed")
        };
        let page = owner
            .command(command(
                Operation::ListRemoteProjects {
                    directory_id: None,
                    cursor: None,
                },
                epoch,
                "roots",
            ))
            .await;
        let directory_id = page.body["result"]["entries"][0]["directoryId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            !page
                .body
                .to_string()
                .contains(root.path().to_str().unwrap())
        );
        let refused = observer
            .command(command(
                Operation::EnrollRemoteProject {
                    directory_id: directory_id.clone(),
                },
                epoch,
                "borrowed-root",
            ))
            .await;
        assert_eq!(refused.status, 409);
        let enrolled = owner
            .command(command(
                Operation::EnrollRemoteProject {
                    directory_id: directory_id.clone(),
                },
                epoch,
                "enroll-root",
            ))
            .await;
        assert_eq!(enrolled.status, 202, "{:?}", enrolled.body);
        assert_eq!(enrolled.body["result"]["outcome"], "workspaces");
        assert_eq!(crate::workspace::load(persisted.path()).unwrap().len(), 1);
        let workspace_id = enrolled.body["result"]["workspaces"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        std::fs::rename(root.path(), persisted.path().join("moved-root")).unwrap();
        std::fs::create_dir(root.path()).unwrap();
        let swapped = owner
            .command(command(
                Operation::CreateSession { workspace_id },
                epoch,
                "swapped-root",
            ))
            .await;
        assert_eq!(swapped.status, 422);
        assert_eq!(swapped.body["error"], "workspace_identity_changed");
        crate::control::execute(
            crate::control::ControlRequest::RevokeRemoteRoot {
                root_id: roots[0].root_id.clone(),
            },
            &state,
        )
        .await;
        let refused = owner
            .command(command(
                Operation::EnrollRemoteProject { directory_id },
                epoch,
                "revoked-root",
            ))
            .await;
        assert_eq!(refused.status, 422);
    }

    #[tokio::test]
    async fn shared_host_refuses_retired_transfer_operations() {
        let state = Arc::new(
            HostState::new(
                crate::origin::LocalOrigin::new("0123456789abcdef0123456789abcdef", 21001).unwrap(),
            )
            .with_agent_mode(AgentMode::Shared)
            .with_remote_experiments(crate::remote_management::RemoteExperiments {
                control_transfer: true,
                device_panel: true,
                projects: false,
            }),
        );
        let (phone, _) = pair(&state).await;
        let (desktop, _) = pair(&state).await;
        phone.connect_version(4).await.unwrap();
        desktop.connect_version(4).await.unwrap();
        assert!(!phone.control_transfer_enabled());
        for operation in [
            Operation::RequestControlTransfer,
            Operation::ApproveControlTransfer {
                request_id: "retired".into(),
            },
            Operation::CancelControlTransfer {
                request_id: "retired".into(),
            },
        ] {
            let reply = phone.command(shared_command(operation, 0, "retired")).await;
            assert_eq!(reply.status, 422);
        }
        let listed = desktop
            .command(shared_command(Operation::ListBrowserDevices, 0, "devices"))
            .await;
        assert_eq!(
            listed.body["result"]["devices"].as_array().unwrap().len(),
            2
        );
        assert_eq!(phone.authorize_control(0).await, Ok(()));
        assert_eq!(desktop.authorize_control(0).await, Ok(()));
    }

    #[tokio::test]
    async fn device_resume_requires_original_noise_key() {
        let state = state();
        let (_, local) = pair(&state).await;
        assert!(
            RemoteAccess::authenticate_device(
                state.clone(),
                local.session_token,
                local.csrf_token,
                [1; 32]
            )
            .await
            .is_err()
        );
        let nonce = state
            .pairing
            .lock()
            .await
            .mint_nonce(crate::now_ms())
            .unwrap();
        let (_, remote) = RemoteAccess::pair_device(state.clone(), nonce.expose(), [1; 32])
            .await
            .unwrap();
        assert!(
            RemoteAccess::authenticate_device(
                state.clone(),
                remote.session_token.clone(),
                remote.csrf_token.clone(),
                [2; 32]
            )
            .await
            .is_err()
        );
        let resumed = RemoteAccess::authenticate_device(
            state.clone(),
            remote.session_token,
            remote.csrf_token,
            [1; 32],
        )
        .await
        .unwrap();
        assert!(resumed.verify_access().await.is_ok());
        state.pairing.lock().await.revoke(&remote.session_id);
        assert!(resumed.verify_access().await.is_err());
    }

    #[tokio::test]
    async fn paired_readers_cannot_mutate_with_a_borrowed_or_stale_epoch() {
        let state = state();
        let (owner, _) = pair(&state).await;
        let (observer, _) = pair(&state).await;
        let epoch = owner.connect().await.unwrap();
        let operation = Operation::OpenWorkspacePicker;
        assert_eq!(
            observer
                .command(command(operation.clone(), epoch, "other"))
                .await
                .status,
            409
        );
        assert_eq!(
            owner
                .command(command(operation, epoch + 1, "stale"))
                .await
                .status,
            409
        );
        assert!(!state.session.lock().await.picker_open);
    }

    #[tokio::test]
    async fn another_connection_with_the_same_pairing_cannot_borrow_the_epoch() {
        let state = state();
        let (owner, paired) = pair(&state).await;
        let observer =
            RemoteAccess::authenticate(state.clone(), paired.session_token, paired.csrf_token)
                .await
                .unwrap();
        let epoch = owner.connect().await.unwrap();
        assert_eq!(observer.connect().await, Err("controller_held"));
        let result = observer
            .command(command(Operation::OpenWorkspacePicker, epoch, "borrowed"))
            .await;
        assert_eq!(result.status, 409);
        assert!(!state.session.lock().await.picker_open);
    }

    #[tokio::test]
    async fn revocation_stops_events_commands_and_heartbeat_immediately() {
        let state = state();
        let (access, paired) = pair(&state).await;
        let epoch = access.connect().await.unwrap();
        let result = access
            .command(command(
                Operation::RevokeBrowserPairing {
                    session_id: Some(paired.session_id),
                },
                epoch,
                "revoke",
            ))
            .await;
        assert_eq!(result.status, 202);
        assert!(access.events(0).await.is_err());
        assert!(access.heartbeat(epoch).await.is_err());
        assert_eq!(
            access
                .command(command(Operation::Bootstrap, epoch, "read"))
                .await
                .status,
            403
        );
    }

    #[tokio::test]
    async fn observer_can_forget_only_itself_without_controlling_the_host() {
        let state = state();
        let (owner, owner_grant) = pair(&state).await;
        let (observer, _) = pair(&state).await;
        let epoch = owner.connect().await.unwrap();
        assert_eq!(observer.connect().await, Err("controller_held"));
        for target in [None, Some(owner_grant.session_id)] {
            assert_eq!(
                observer
                    .command(command(
                        Operation::RevokeBrowserPairing { session_id: target },
                        epoch,
                        "forged"
                    ))
                    .await
                    .status,
                409
            );
        }
        let mut envelope = command(Operation::ForgetBrowserPairing, 0, "forget");
        envelope.controller_epoch = None;
        assert_eq!(observer.command(envelope).await.status, 202);
        assert!(observer.verify_access().await.is_err());
        assert!(owner.verify_access().await.is_ok());
        assert!(owner.heartbeat(epoch).await.is_ok());
    }

    #[tokio::test]
    async fn disconnect_preserves_shared_work_without_an_exclusive_epoch() {
        let state = Arc::new(
            HostState::new(
                crate::origin::LocalOrigin::new("0123456789abcdef0123456789abcdef", 21001).unwrap(),
            )
            .with_agent_mode(AgentMode::Shared),
        );
        let (access, paired) = pair(&state).await;
        let epoch = access.connect_version(4).await.unwrap();
        state
            .journal
            .lock()
            .await
            .begin("inflight", "Prompt", Some("s"));
        access.disconnect().await;
        assert_eq!(
            state.journal.lock().await.effect_state("inflight"),
            Some(crate::journal::EffectState::Intended)
        );
        let resumed =
            RemoteAccess::authenticate(state.clone(), paired.session_token, paired.csrf_token)
                .await
                .unwrap();
        let next = resumed.connect_version(4).await.unwrap();
        assert_eq!(next, epoch);
        assert!(access.heartbeat(epoch).await.is_ok());
        assert!(resumed.heartbeat(next).await.is_ok());
    }

    #[tokio::test]
    async fn acknowledgements_change_cursor_and_reject_future_events() {
        let state = state();
        let (access, _) = pair(&state).await;
        let epoch = access.connect().await.unwrap();
        let event = state
            .emit_event(crate::protocol::Event::WorkspacesChanged, None)
            .await;
        assert_eq!(
            access
                .command(command(
                    Operation::AcknowledgeEvents {
                        through_sequence: event.event_sequence
                    },
                    epoch,
                    "ack"
                ))
                .await
                .status,
            202
        );
        assert_eq!(
            state.journal.lock().await.acknowledged_through(),
            event.event_sequence
        );
        assert_eq!(
            access
                .command(command(
                    Operation::AcknowledgeEvents {
                        through_sequence: event.event_sequence + 1
                    },
                    epoch,
                    "future"
                ))
                .await
                .status,
            422
        );
    }

    #[tokio::test]
    async fn shared_shell_shortcut_is_refused_before_an_effect_is_recorded() {
        let state = Arc::new(
            HostState::new(
                crate::origin::LocalOrigin::new("0123456789abcdef0123456789abcdef", 21001).unwrap(),
            )
            .with_agent_mode(AgentMode::Shared),
        );
        let (access, _) = pair(&state).await;
        let epoch = access.connect_version(4).await.unwrap();
        let response = access
            .command(shared_command(
                Operation::Prompt {
                    session_id: "s".into(),
                    text: "! echo secret".into(),
                    bash: false,
                },
                epoch,
                "shell",
            ))
            .await;
        assert_eq!(response.status, 422);
        assert_eq!(response.body["error"], "remote_shell_disabled");
        assert_eq!(state.journal.lock().await.effect_state("shell"), None);
    }
}
