use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::dispatch::DispatchOutcome;
use crate::protocol::{CommandEnvelope, Event, Operation, PROTOCOL_VERSION};
use crate::server::HostState;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RemoteExperiments {
    pub projects: bool,
    pub control_transfer: bool,
    pub device_panel: bool,
}

impl RemoteExperiments {
    pub(crate) fn from_environment() -> Self {
        fn enabled(name: &str) -> bool {
            std::env::var(name).is_ok_and(|value| value == "1" || value == "true")
        }
        Self {
            projects: cfg!(unix) && enabled("SPANREED_REMOTE_EXPERIMENT_PROJECTS"),
            control_transfer: enabled("SPANREED_REMOTE_EXPERIMENT_CONTROL_TRANSFER"),
            device_panel: enabled("SPANREED_REMOTE_EXPERIMENT_DEVICE_PANEL"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCapabilities {
    pub image: bool,
    pub audio: bool,
    pub embedded_context: bool,
}

impl From<&crate::acp::InitializeResult> for PromptCapabilities {
    fn from(value: &crate::acp::InitializeResult) -> Self {
        Self {
            image: value.supports_qualified_images(),
            audio: value.supports_audio(),
            embedded_context: value.supports_embedded_context(),
        }
    }
}

pub(crate) fn handles(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::GetRemoteCapabilities
            | Operation::ListRemoteProjects { .. }
            | Operation::EnrollRemoteProject { .. }
            | Operation::ListBrowserDevices
            | Operation::RequestControlTransfer
            | Operation::CancelControlTransfer { .. }
            | Operation::ApproveControlTransfer { .. }
    )
}

pub(crate) async fn verify_workspace(
    state: &HostState,
    operation: &Operation,
) -> Result<(), &'static str> {
    if !state.remote_experiments.projects {
        return Ok(());
    }
    let workspace_id = match operation {
        Operation::CreateSession { workspace_id }
        | Operation::LoadSession { workspace_id, .. }
        | Operation::ListSessions { workspace_id }
        | Operation::ListContext { workspace_id, .. }
        | Operation::ListTools {
            workspace_id: Some(workspace_id),
        } => Some(workspace_id.clone()),
        Operation::GetSessionExecution { session_id }
        | Operation::GetSessionModels { session_id }
        | Operation::SetSessionConfiguration { session_id, .. }
        | Operation::SetQueuePaused { session_id, .. }
        | Operation::ClearQueuedPrompts { session_id, .. }
        | Operation::CancelSessionTurn { session_id, .. }
        | Operation::ReviewUncertainTurn { session_id, .. }
        | Operation::InterruptAndEnqueue { session_id, .. }
        | Operation::Prompt { session_id, .. }
        | Operation::PromptContent { session_id, .. }
        | Operation::SendNow { session_id, .. }
        | Operation::GetSessionChanges { session_id, .. }
        | Operation::GetSessionInspector { session_id }
        | Operation::DiagnoseSession { session_id }
        | Operation::RepairSession { session_id, .. } => state
            .session
            .lock()
            .await
            .sessions
            .get(session_id)
            .map(|live| live.workspace_id.clone()),
        _ => None,
    };
    if let Some(workspace_id) = workspace_id {
        let directory = state
            .state_directory
            .as_deref()
            .ok_or("workspace_storage")?;
        let index = crate::workspace::load(directory).map_err(|_| "workspace_storage")?;
        index
            .resolve(&workspace_id)
            .map_err(|_| "workspace_identity_changed")?;
    }
    Ok(())
}

pub(crate) async fn command(
    state: &HostState,
    principal: &str,
    connection: &str,
    envelope: &CommandEnvelope,
) -> Response {
    let key = envelope
        .idempotency_key
        .as_ref()
        .filter(|_| envelope.operation.has_side_effect())
        .map(|key| crate::receipts::scoped_key(principal, key));
    if let Some(key) = &key {
        let result = state
            .journal
            .lock()
            .await
            .begin(key, envelope.operation.name(), None);
        let error = match result {
            crate::journal::BeginOutcome::Dispatch => None,
            crate::journal::BeginOutcome::AlreadyCompleted => Some("already_completed"),
            crate::journal::BeginOutcome::DoNotReplay(_) => Some("not_replayable"),
            crate::journal::BeginOutcome::NotDurable => Some("intent_not_durable"),
        };
        if let Some(error) = error {
            return response(envelope, Err(error));
        }
    }
    let result = dispatch(state, principal, connection, envelope).await;
    if let Some(key) = key {
        if state.journal.lock().await.complete(&key).is_err() {
            return response(envelope, Err("completion_not_durable"));
        }
    }
    response(envelope, result)
}

fn response(envelope: &CommandEnvelope, result: Result<DispatchOutcome, &'static str>) -> Response {
    match result {
        Ok(result) => (StatusCode::ACCEPTED, axum::Json(serde_json::json!({ "protocolVersion": PROTOCOL_VERSION, "requestId": envelope.request_id, "result": result }))).into_response(),
        Err(error) => (StatusCode::UNPROCESSABLE_ENTITY, axum::Json(serde_json::json!({ "protocolVersion": PROTOCOL_VERSION, "requestId": envelope.request_id, "error": error }))).into_response(),
    }
}

async fn dispatch(
    state: &HostState,
    principal: &str,
    connection: &str,
    envelope: &CommandEnvelope,
) -> Result<DispatchOutcome, &'static str> {
    let flags = state.remote_experiments;
    let control_transfer = state.remote_control_transfer_enabled();
    let now = crate::now_ms();
    match &envelope.operation {
        Operation::GetRemoteCapabilities => Ok(DispatchOutcome::RemoteCapabilities {
            projects: flags.projects && cfg!(unix) && state.state_directory.is_some(),
            control_transfer,
            device_panel: flags.device_panel,
            upstream_prompt: *state.upstream_prompt.lock().await,
            content_protocol_version: crate::protocol::CONTENT_PROTOCOL_VERSION,
            multi_writer: state.agent_mode == crate::acp::AgentMode::Shared,
            multi_writer_protocol_version: crate::protocol::MULTI_WRITER_PROTOCOL_VERSION,
            targeted_cancel_supported: state
                .targeted_cancel_supported
                .load(std::sync::atomic::Ordering::Acquire),
        }),
        Operation::ListRemoteProjects {
            directory_id,
            cursor,
        } if flags.projects && cfg!(unix) => {
            let page = state.remote_projects.lock().await.list(
                principal,
                directory_id.as_deref(),
                cursor.as_deref(),
                now,
            )?;
            Ok(DispatchOutcome::RemoteProjects {
                parent_id: page.parent_id,
                entries: page.entries,
                next_cursor: page.next_cursor,
            })
        }
        Operation::EnrollRemoteProject { directory_id } if flags.projects && cfg!(unix) => {
            let directory = state
                .state_directory
                .as_deref()
                .ok_or("remote_roots_unusable")?;
            let projects = state.remote_projects.lock().await;
            let _update = state.workspace_updates.lock().await;
            let mut index = crate::workspace::load(directory).map_err(|_| "workspace_storage")?;
            projects.enrol(principal, directory_id, now, &mut index)?;
            crate::workspace::persist(directory, &index).map_err(|_| "workspace_storage")?;
            state.load_workspaces(&index).await;
            drop(projects);
            state.emit_event(Event::WorkspacesChanged, None).await;
            let journal = state.journal.lock().await;
            let session = state.session.lock().await;
            Ok(DispatchOutcome::Workspaces {
                observation_epoch: journal.observation_epoch(),
                workspaces: session.project_workspaces(),
                projects: session.project_projects(),
                open_sessions: session.project_sessions(),
                integrations: crate::integrations::list(),
                pending_reviews: journal
                    .pending_reviews()
                    .into_iter()
                    .map(crate::dispatch::ReviewProjection::from)
                    .collect(),
            })
        }
        Operation::ListBrowserDevices if flags.device_panel || control_transfer => {
            let broker = state.pairing.lock().await;
            let sessions = broker.session_ids(now);
            let mut connections = state.remote_connections.lock().await;
            let lease = state.lease.lock().await;
            let controller = if lease.state(now) == crate::lease::LeaseState::Vacant {
                None
            } else {
                lease.holder_session()
            };
            let devices = connections.devices(&sessions, principal, controller);
            let requests = connections
                .requests(now)
                .into_iter()
                .filter(|request| sessions.contains(&request.session_id))
                .collect();
            Ok(DispatchOutcome::BrowserDevices {
                devices,
                control_transfer_requests: requests,
            })
        }
        Operation::RequestControlTransfer if control_transfer => {
            let mut connections = state.remote_connections.lock().await;
            let lease = state.lease.lock().await;
            if lease.connection_epoch(connection, now).is_some() {
                return Err("already_controller");
            }
            if lease.state(now) != crate::lease::LeaseState::Held {
                return Err("controller_unavailable");
            }
            let request = connections.request(principal, connection, now)?;
            drop(lease);
            drop(connections);
            state.notify_remote_devices().await;
            Ok(DispatchOutcome::ControlTransferRequested { request })
        }
        Operation::CancelControlTransfer { request_id } if control_transfer => {
            state
                .remote_connections
                .lock()
                .await
                .cancel(principal, connection, request_id)?;
            state.notify_remote_devices().await;
            Ok(DispatchOutcome::ControlTransferCancelled {
                request_id: request_id.clone(),
            })
        }
        Operation::ApproveControlTransfer { request_id } if control_transfer => {
            let broker = state.pairing.lock().await;
            let sessions = broker.session_ids(now);
            if !sessions.iter().any(|id| id == principal) {
                return Err("session_invalid");
            }
            let mut connections = state.remote_connections.lock().await;
            let mut lease = state.lease.lock().await;
            connections.approve(
                &mut lease,
                principal,
                connection,
                envelope.controller_epoch.unwrap_or(0),
                request_id,
                &sessions,
                now,
            )?;
            drop(lease);
            drop(connections);
            drop(broker);
            state.notify_remote_devices().await;
            Ok(DispatchOutcome::ControlTransferApproved {
                request_id: request_id.clone(),
            })
        }
        _ => Err("remote_experiment_disabled"),
    }
}
