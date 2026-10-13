use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::protocol::{CommandEnvelope, Event, Operation, QueueDisposition};
use crate::server::HostState;

pub(crate) fn handles(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::GetSessionExecution { .. }
            | Operation::Prompt { .. }
            | Operation::PromptContent { .. }
            | Operation::RemoveQueued { .. }
            | Operation::SetQueuePaused { .. }
            | Operation::ClearQueuedPrompts { .. }
            | Operation::ReviewUncertainTurn { .. }
            | Operation::CancelSessionTurn { .. }
            | Operation::InterruptAndEnqueue { .. }
    )
}

pub(crate) fn session_id(operation: &Operation) -> Option<&str> {
    match operation {
        Operation::GetSessionExecution { session_id }
        | Operation::GetSessionModels { session_id }
        | Operation::SetSessionConfiguration { session_id, .. }
        | Operation::Prompt { session_id, .. }
        | Operation::PromptContent { session_id, .. }
        | Operation::RemoveQueued { session_id, .. }
        | Operation::SetQueuePaused { session_id, .. }
        | Operation::ReviewUncertainTurn { session_id, .. }
        | Operation::ClearQueuedPrompts { session_id, .. }
        | Operation::CancelSessionTurn { session_id, .. }
        | Operation::InterruptAndEnqueue { session_id, .. } => Some(session_id),
        _ => None,
    }
}

pub(crate) fn response(envelope: &CommandEnvelope, result: Result<Value, &str>) -> Response {
    let body = match result {
        Ok(result) => json!({"protocolVersion":2,"requestId":envelope.request_id,"result":result}),
        Err(error) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                axum::Json(
                    json!({"protocolVersion":2,"requestId":envelope.request_id,"error":error}),
                ),
            )
                .into_response();
        }
    };
    (StatusCode::ACCEPTED, axum::Json(body)).into_response()
}

pub(crate) async fn command(
    state: Arc<HostState>,
    principal: &str,
    envelope: &CommandEnvelope,
) -> Response {
    let Some(id) = session_id(&envelope.operation) else {
        return response(envelope, Err("unsupported"));
    };
    if state.session.lock().await.session(id).is_err() {
        return response(envelope, Err("unknown_session"));
    }
    if matches!(
        envelope.operation,
        Operation::CancelSessionTurn { .. } | Operation::InterruptAndEnqueue { .. }
    ) && !state
        .targeted_cancel_supported
        .load(std::sync::atomic::Ordering::Acquire)
    {
        return response(envelope, Err("targeted_cancel_unsupported"));
    }
    let result = dispatch(&state, principal, envelope).await;
    if !matches!(envelope.operation, Operation::GetSessionExecution { .. }) {
        state
            .emit_event(
                Event::SessionExecutionChanged {
                    session_id: id.into(),
                },
                None,
            )
            .await;
        drain(&state, id);
    }
    response(envelope, result)
}

async fn dispatch(
    state: &Arc<HostState>,
    principal: &str,
    envelope: &CommandEnvelope,
) -> Result<Value, &'static str> {
    let id = session_id(&envelope.operation).ok_or("unsupported")?;
    match &envelope.operation {
        Operation::GetSessionExecution { .. } => return snapshot(state, id).await,
        Operation::Prompt { text, bash, .. } if !bash => {
            return enqueue(state, principal, envelope, id, text, &[], None).await;
        }
        Operation::PromptContent {
            text,
            image_ids,
            send_now: false,
            ..
        } => return enqueue(state, principal, envelope, id, text, image_ids, None).await,
        Operation::InterruptAndEnqueue {
            text,
            image_ids,
            expected_turn_id,
            expected_queue_revision,
            ..
        } => {
            return enqueue(
                state,
                principal,
                envelope,
                id,
                text,
                image_ids,
                Some((expected_turn_id, *expected_queue_revision)),
            )
            .await;
        }
        Operation::Prompt { .. } => return Err("remote_shell_disabled"),
        Operation::PromptContent { .. } => return Err("targeted_interrupt_required"),
        _ => {}
    }
    let key = envelope
        .idempotency_key
        .as_deref()
        .ok_or("idempotency_key_required")?;
    let scoped = crate::receipts::scoped_key(principal, key);
    {
        let mut receipts = state.receipts.lock().await;
        if state
            .execution
            .lock()
            .await
            .status(principal, key)
            .is_some()
        {
            return Err("idempotency_conflict");
        }
        let previous = receipts.get(&scoped).is_some();
        let receipt = receipts.begin(
            scoped.clone(),
            key.into(),
            &serde_json::to_value(&envelope.operation).map_err(|_| "invalid_command")?,
        )?;
        if previous {
            return Ok(
                json!({"outcome":"commandStatus","state":receipt.state,"result":receipt.result,"error":receipt.error,"idempotencyKey":key}),
            );
        }
    }
    let result = change_queue(state, id, &envelope.operation).await;
    let uncertain = matches!(result, Err("agent_failed" | "queue_storage_unavailable"));
    state.receipts.lock().await.finish(
        &scoped,
        result.as_ref().ok().cloned(),
        result.as_ref().err().map(|error| (*error).into()),
        uncertain,
    )?;
    result
}

async fn enqueue(
    state: &Arc<HostState>,
    principal: &str,
    envelope: &CommandEnvelope,
    id: &str,
    text: &str,
    image_ids: &[String],
    interrupt: Option<(&str, u64)>,
) -> Result<Value, &'static str> {
    let key = envelope
        .idempotency_key
        .as_deref()
        .ok_or("idempotency_key_required")?;
    if text.trim().is_empty() && image_ids.is_empty() {
        return Err("empty_prompt");
    }
    let images = if image_ids.is_empty() {
        Vec::new()
    } else {
        if !state
            .upstream_prompt
            .lock()
            .await
            .as_ref()
            .is_some_and(|caps| caps.image)
        {
            return Err("image_not_supported");
        }
        state
            .session
            .lock()
            .await
            .media
            .resolve(Some(principal), id, image_ids, crate::now_ms())
            .map_err(|_| "image_unavailable")?
    };
    let broker = state.pairing.lock().await;
    if !broker.has_session(principal, crate::now_ms()) {
        return Err("session_invalid");
    }
    let fingerprint = crate::execution::fingerprint(&envelope.operation);
    let session = state.session.lock().await;
    session.session(id).map_err(|_| "unknown_session")?;
    let receipts = state.receipts.lock().await;
    if receipts
        .get(&crate::receipts::scoped_key(principal, key))
        .is_some()
    {
        return Err("idempotency_conflict");
    }
    let mut execution = state.execution.lock().await;
    let repeated = execution.status(principal, key).is_some();
    let result = if let Some((turn, revision)) = interrupt {
        execution.interrupt_enqueue(
            id,
            principal,
            key,
            text,
            &images,
            &fingerprint,
            turn,
            revision,
        )?
    } else {
        execution.enqueue(id, principal, key, text, &images, &fingerprint)?
    };
    let send_interrupt =
        interrupt.is_some_and(|(turn, _)| !repeated && execution.cancellation_ready(id, turn));
    drop(execution);
    drop(receipts);
    drop(session);
    drop(broker);
    if let Some((turn, _)) = interrupt.filter(|_| send_interrupt) {
        send_cancel(state, id, turn).await?;
    }
    Ok(result)
}

async fn change_queue(
    state: &Arc<HostState>,
    id: &str,
    operation: &Operation,
) -> Result<Value, &'static str> {
    match operation {
        Operation::RemoveQueued { entry_id, .. } => {
            state
                .execution
                .lock()
                .await
                .remove(id, Some(entry_id), None)?
        }
        Operation::ClearQueuedPrompts {
            expected_queue_revision,
            ..
        } => state
            .execution
            .lock()
            .await
            .remove(id, None, Some(*expected_queue_revision))?,
        Operation::SetQueuePaused {
            paused,
            expected_queue_revision,
            ..
        } => state
            .execution
            .lock()
            .await
            .set_paused(id, *paused, *expected_queue_revision)?,
        Operation::ReviewUncertainTurn {
            expected_turn_id, ..
        } => {
            let path = {
                let session = state.session.lock().await;
                let live = session.session(id).map_err(|_| "unknown_session")?;
                if live.running {
                    return Err("session_busy");
                }
                session
                    .workspaces
                    .get(&live.workspace_id)
                    .ok_or("unknown_workspace")?
                    .path
                    .clone()
            };
            let agent = state
                .agent
                .lock()
                .await
                .clone()
                .ok_or("agent_unavailable")?;
            let previous = state
                .journal
                .lock()
                .await
                .begin_restore(id)
                .unwrap_or_else(|| Event::SessionSnapshot {
                    session_id: id.into(),
                    messages: vec![],
                    tools: vec![],
                    members: vec![],
                    workflows: vec![],
                    background_tasks: vec![],
                });
            let loaded = agent.load_session(id, &path).await;
            let flushed = if loaded.is_ok() {
                agent.flush_events().await
            } else {
                Ok(())
            };
            state.journal.lock().await.finish_restore(previous);
            state.event_notify.notify_waiters();
            let raw = loaded.map_err(|_| "agent_failed")?;
            flushed.map_err(|_| "agent_failed")?;
            if raw.get("_meta").is_none() {
                return Err("idle_not_confirmed");
            }
            if raw
                .pointer("/_meta/x.ai~1runningPromptId")
                .is_some_and(|id| !id.is_null())
            {
                return Err("session_busy");
            }
            let session = state.session.lock().await;
            if session.session(id).map_err(|_| "unknown_session")?.running {
                return Err("session_busy");
            }
            state
                .execution
                .lock()
                .await
                .review_uncertain_turn(id, expected_turn_id)?;
        }
        Operation::CancelSessionTurn {
            expected_turn_id,
            queue_disposition,
            ..
        } => {
            let first = state.execution.lock().await.cancel(
                id,
                expected_turn_id,
                *queue_disposition == QueueDisposition::Pause,
            )?;
            if first {
                send_cancel(state, id, expected_turn_id).await?;
            }
        }
        _ => return Err("unsupported"),
    }
    snapshot(state, id).await
}

async fn snapshot(state: &HostState, id: &str) -> Result<Value, &'static str> {
    let session = state.session.lock().await;
    let running = session.session(id).map_err(|_| "unknown_session")?.running;
    let mut value = state.execution.lock().await.snapshot(id);
    value["externalTurnRunning"] = (running && value["activeTurn"].is_null()).into();
    value["canCancelTurn"] = (state
        .targeted_cancel_supported
        .load(std::sync::atomic::Ordering::Acquire)
        && matches!(
            value["activeTurn"]["state"].as_str(),
            Some("starting" | "running")
        ))
    .into();
    Ok(value)
}

async fn send_cancel(state: &HostState, id: &str, turn: &str) -> Result<(), &'static str> {
    let agent = state.agent.lock().await.clone();
    let result = match agent {
        Some(agent) => agent
            .cancel_target(id, turn)
            .await
            .map_err(|_| "agent_failed"),
        None => Err("agent_failed"),
    };
    state
        .execution
        .lock()
        .await
        .finish_cancel(id, turn, result.is_err())?;
    result
}

#[derive(Debug, Default)]
pub(crate) struct Drainers {
    active: std::collections::BTreeMap<String, bool>,
    overflow: bool,
}

impl Drainers {
    fn reserve(&mut self, id: &str) -> bool {
        if let Some(dirty) = self.active.get_mut(id) {
            *dirty = true;
            return false;
        }
        if self.active.len() >= crate::bounds::MAX_LIVE_SESSIONS {
            self.overflow = true;
            return false;
        }
        self.active.insert(id.into(), false);
        true
    }

    fn repeat(&mut self, id: &str) -> bool {
        if let Some(dirty) = self.active.get_mut(id)
            && std::mem::take(dirty)
        {
            return true;
        }
        self.active.remove(id);
        false
    }
}

pub(crate) fn drain(state: &Arc<HostState>, session_id: &str) {
    if !state
        .drainers
        .lock()
        .expect("drainer registry")
        .reserve(session_id)
    {
        return;
    }
    let state = state.clone();
    let id = session_id.to_owned();
    tokio::spawn(async move {
        loop {
            drain_session(state.clone(), id.clone()).await;
            let overflow = {
                let mut registry = state.drainers.lock().expect("drainer registry");
                if registry.repeat(&id) {
                    continue;
                }
                std::mem::take(&mut registry.overflow)
            };
            if overflow {
                let ids = state
                    .session
                    .lock()
                    .await
                    .sessions
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                for id in ids {
                    drain(&state, &id);
                }
            }
            return;
        }
    });
}

async fn drain_session(state: Arc<HostState>, id: String) {
    loop {
        if state
            .agent_reconciling
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        if state.session.lock().await.session(&id).is_err() {
            return;
        }
        if state
            .session
            .lock()
            .await
            .session(&id)
            .is_ok_and(|live| live.running)
        {
            return;
        }
        let Some(agent) = state.agent.lock().await.clone() else {
            return;
        };
        let Ok(_permit) = state.inflight_commands.clone().acquire_owned().await else {
            return;
        };
        let next = {
            let broker = state.pairing.lock().await;
            let mut session = state.session.lock().await;
            if session.session(&id).is_err() || session.session(&id).is_ok_and(|live| live.running)
            {
                return;
            }
            let mut execution = state.execution.lock().await;
            for principal in execution.queued_principals(&id) {
                if !broker.has_session(&principal, crate::now_ms())
                    && execution.revoke(Some(&principal)).is_err()
                {
                    return;
                }
            }
            let next = match execution.begin_turn(&id) {
                Ok(Some(turn)) => turn,
                _ => return,
            };
            session.set_running(&id, true);
            session.local_turns.insert(id.clone());
            next
        };
        let images = state.execution.lock().await.images_for(&next.entry);
        if images.is_err() {
            {
                let mut session = state.session.lock().await;
                session.set_running(&id, false);
                session.local_turns.remove(&id);
            }
            let _ = state
                .execution
                .lock()
                .await
                .finish_turn(&id, &next.turn_id, true, true);
            state
                .emit_event(
                    Event::SessionExecutionChanged {
                        session_id: id.clone(),
                    },
                    None,
                )
                .await;
            return;
        }
        let images = images.expect("checked images");
        {
            let mut session = state.session.lock().await;
            session.set_running(&id, true);
            session.local_turns.insert(id.clone());
            let _ = session.begin_review_turn(&id);
            session.transcript_media.remember(&id, &images);
        }
        state
            .emit_event(
                Event::SessionExecutionChanged {
                    session_id: id.clone(),
                },
                None,
            )
            .await;
        state
            .emit_event(
                Event::SessionStatus {
                    session_id: id.clone(),
                    state: "running".into(),
                },
                None,
            )
            .await;
        state
            .emit_event(
                Event::PromptSent {
                    session_id: id.clone(),
                    text: next.entry.text.clone(),
                    images: next.entry.images.clone(),
                    idempotency_key: None,
                },
                None,
            )
            .await;
        let (started_tx, mut started_rx) = tokio::sync::oneshot::channel();
        let pending = agent.prompt_target_with_started(
            &id,
            &next.turn_id,
            &next.entry.text,
            &images,
            Some(started_tx),
        );
        tokio::pin!(pending);
        let result = tokio::select! {
            biased;
            started=&mut started_rx => {
                if started.is_ok() {
                    let cancel=state.execution.lock().await.mark_dispatched(&id,&next.turn_id);
                    state.emit_event(Event::SessionExecutionChanged {session_id:id.clone()},None).await;
                    if matches!(cancel,Ok(true)) { let _=send_cancel(&state,&id,&next.turn_id).await; }
                }
                pending.await
            }
            result=&mut pending => result,
        };
        let uncertain = result
            .as_ref()
            .err()
            .is_some_and(|error| !error.is_unsupported_method());
        {
            let mut session = state.session.lock().await;
            if let Ok(result) = &result {
                session.finish_review_turn(&id, Some(result));
            } else {
                session.interrupt_review_turn(&id);
            }
            session.local_turns.remove(&id);
            session.set_running(&id, false);
        }
        let persisted = state.execution.lock().await.finish_turn(
            &id,
            &next.turn_id,
            uncertain,
            result.is_err(),
        );
        state
            .emit_event(
                Event::SessionExecutionChanged {
                    session_id: id.clone(),
                },
                None,
            )
            .await;
        state
            .emit_event(
                Event::SessionStatus {
                    session_id: id.clone(),
                    state: if uncertain || persisted.is_err() {
                        "needsReview"
                    } else {
                        "idle"
                    }
                    .into(),
                },
                None,
            )
            .await;
        state
            .emit_event(
                Event::SessionReviewUpdated {
                    session_id: id.clone(),
                    changes: true,
                    context: true,
                },
                None,
            )
            .await;
        if uncertain || persisted.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{AgentCommand, AgentHandle, AgentMode};
    use crate::dispatch::{LiveSession, Workspace};
    use crate::remote::RemoteAccess;

    #[test]
    fn drainer_wakeups_are_coalesced_and_an_exit_cannot_lose_a_new_request() {
        let mut registry = Drainers::default();
        assert!(registry.reserve("s"));
        for _ in 0..1000 {
            assert!(!registry.reserve("s"));
        }
        assert!(registry.repeat("s"));
        assert_eq!(registry.active.len(), 1);
        assert!(!registry.repeat("s"));
        assert!(registry.reserve("s"));
        for i in 1..crate::bounds::MAX_LIVE_SESSIONS {
            assert!(registry.reserve(&i.to_string()));
        }
        assert!(!registry.reserve("overflow"));
        assert!(registry.overflow);
        assert_eq!(registry.active.len(), crate::bounds::MAX_LIVE_SESSIONS);
    }

    #[tokio::test]
    async fn saturated_command_permits_do_not_accumulate_duplicate_drainers() {
        let (state, agent, _directory) = setup().await;
        let phone = pair(&state).await;
        let permits = state
            .inflight_commands
            .clone()
            .acquire_many_owned(16)
            .await
            .unwrap();
        for id in ["one", "two"] {
            state
                .execution
                .lock()
                .await
                .enqueue(id, phone.session_id(), id, id, &[], id)
                .unwrap();
            for _ in 0..1000 {
                drain(&state, id);
            }
        }
        tokio::task::yield_now().await;
        assert_eq!(state.drainers.lock().unwrap().active.len(), 2);
        drop(permits);
        wait_for(&agent, |state| {
            state["running"].as_array().unwrap().len() == 2
        })
        .await;
        let release = agent.request("test/release", json!({"sessionId":"one"}));
        let successor = phone.command(envelope(prompt("one", "successor"), "successor"));
        let (released, queued) = tokio::join!(release, successor);
        released.unwrap();
        assert_eq!(queued.status, 202, "{}", queued.body);
        let observed = wait_for(&agent, |state| {
            state["accepted"].as_array().unwrap().len() == 3
        })
        .await;
        assert_eq!(observed["accepted"][2]["text"], "successor");
        assert!(state.drainers.lock().unwrap().active.len() <= 2);
        agent.shutdown().await.unwrap();
    }

    fn envelope(operation: Operation, key: &str) -> CommandEnvelope {
        CommandEnvelope {
            protocol_version: 4,
            request_id: key.into(),
            idempotency_key: Some(key.into()),
            controller_epoch: None,
            expected_revision: None,
            deadline_ms: None,
            operation,
        }
    }
    fn prompt(session: &str, text: &str) -> Operation {
        Operation::Prompt {
            session_id: session.into(),
            text: text.into(),
            bash: false,
        }
    }
    async fn pair(state: &Arc<HostState>) -> RemoteAccess {
        let nonce = state
            .pairing
            .lock()
            .await
            .mint_nonce(crate::now_ms())
            .unwrap();
        let (access, _) = RemoteAccess::pair(state.clone(), nonce.expose())
            .await
            .unwrap();
        access.connect_version(4).await.unwrap();
        access
    }
    async fn setup() -> (Arc<HostState>, Arc<AgentHandle>, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let state = Arc::new(
            HostState::new(
                crate::origin::LocalOrigin::new("0123456789abcdef0123456789abcdef", 21001).unwrap(),
            )
            .with_agent_mode(AgentMode::Shared)
            .with_persistence(
                directory.path(),
                Arc::new(crate::picker::UnavailableDirectoryPicker),
            )
            .unwrap(),
        );
        let mut path = std::env::current_exe().unwrap();
        path.pop();
        path.pop();
        path.push("examples");
        path.push(format!(
            "fake_multiwriter_agent{}",
            std::env::consts::EXE_SUFFIX
        ));
        let (agent, events) =
            AgentHandle::spawn(&AgentCommand::new(path.to_string_lossy())).unwrap();
        agent.initialize().await.unwrap();
        state
            .targeted_cancel_supported
            .store(true, std::sync::atomic::Ordering::Release);
        state.attach_agent(agent.clone(), events);
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while state.agent.lock().await.is_none()
                || state
                    .agent_reconciling
                    .load(std::sync::atomic::Ordering::Acquire)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let mut sessions = state.session.lock().await;
        sessions.workspaces.insert(
            "w".into(),
            Workspace {
                id: "w".into(),
                display_name: "test".into(),
                path: directory.path().to_path_buf(),
            },
        );
        for id in ["one", "two"] {
            sessions
                .open_session(LiveSession {
                    id: id.into(),
                    workspace_id: "w".into(),
                    workspace_name: "test".into(),
                    running: false,
                    queued: Vec::new(),
                    opened_at_ms: 1,
                })
                .unwrap();
        }
        drop(sessions);
        (state, agent, directory)
    }
    async fn wait_for(agent: &AgentHandle, predicate: impl Fn(&Value) -> bool) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let snapshot = agent.request("test/state", json!({})).await.unwrap();
                if predicate(&snapshot) {
                    return snapshot;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn two_paired_writers_share_one_fifo_and_other_sessions_overlap() {
        let (state, agent, _directory) = setup().await;
        let phone = pair(&state).await;
        let desktop = pair(&state).await;
        let first = phone
            .command(envelope(prompt("one", "first"), "same"))
            .await;
        assert_eq!(first.status, 202, "{}", first.body);
        wait_for(&agent, |value| {
            value["accepted"].as_array().unwrap().len() == 1
        })
        .await;
        let second = desktop
            .command(envelope(prompt("one", "second"), "same"))
            .await;
        assert_eq!(second.status, 202, "{}", second.body);
        assert_ne!(
            first.body["result"]["entryId"],
            second.body["result"]["entryId"]
        );
        assert_eq!(
            phone
                .command(envelope(prompt("two", "independent"), "independent"))
                .await
                .status,
            202
        );
        let parallel = wait_for(&agent, |value| {
            value["running"].as_array().unwrap().len() == 2
        })
        .await;
        assert_eq!(parallel["accepted"].as_array().unwrap().len(), 2);
        assert_eq!(
            phone
                .command(envelope(prompt("one", "first"), "same"))
                .await
                .body["result"]["entryId"],
            first.body["result"]["entryId"]
        );
        assert_eq!(
            phone
                .command(envelope(prompt("one", "different"), "same"))
                .await
                .body["error"],
            "idempotency_conflict"
        );
        agent
            .request("test/release", json!({"sessionId":"one"}))
            .await
            .unwrap();
        let released = wait_for(&agent, |value| {
            value["accepted"].as_array().unwrap().len() == 3
        })
        .await;
        assert_eq!(released["accepted"][2]["text"], "second");
        agent.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn revoked_pending_origin_is_held_even_if_revocation_was_not_mirrored_to_queue() {
        let (state, agent, _directory) = setup().await;
        let phone = pair(&state).await;
        let desktop = pair(&state).await;
        desktop
            .command(envelope(prompt("one", "first"), "first"))
            .await;
        wait_for(&agent, |value| {
            value["accepted"].as_array().unwrap().len() == 1
        })
        .await;
        phone
            .command(envelope(prompt("one", "second"), "second"))
            .await;
        state.pairing.lock().await.revoke(phone.session_id());
        agent
            .request("test/release", json!({"sessionId":"one"}))
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if state.execution.lock().await.snapshot("one")["queue"][0]["state"]
                    == "originRevoked"
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            agent.request("test/state", json!({})).await.unwrap()["accepted"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        agent.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn legacy_client_is_read_only_and_two_new_clients_need_no_lease() {
        let (state, agent, _directory) = setup().await;
        let phone = pair(&state).await;
        let desktop = pair(&state).await;
        let mut old = envelope(prompt("one", "old"), "old");
        old.protocol_version = 2;
        old.controller_epoch = Some(99);
        assert_eq!(
            phone.command(old).await.body["error"],
            "client_upgrade_required"
        );
        assert_eq!(phone.authorize_control(0).await, Ok(()));
        assert_eq!(desktop.authorize_control(0).await, Ok(()));
        let mut read = envelope(Operation::Bootstrap, "read");
        read.protocol_version = 2;
        assert_eq!(phone.command(read).await.status, 202);
        agent.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn generic_effect_receipts_are_scoped_and_fingerprinted() {
        let (state, agent, _directory) = setup().await;
        let phone = pair(&state).await;
        let desktop = pair(&state).await;
        let create = Operation::CreateSession {
            workspace_id: "w".into(),
        };
        let first = phone.command(envelope(create.clone(), "create")).await;
        assert_eq!(first.status, 202, "{}", first.body);
        let retry = phone.command(envelope(create.clone(), "create")).await;
        assert_eq!(retry.body["result"]["state"], "completed");
        let conflict = phone
            .command(envelope(
                Operation::CloseSession {
                    session_id: "one".into(),
                },
                "create",
            ))
            .await;
        assert_eq!(conflict.body["error"], "idempotency_conflict");
        let other = desktop.command(envelope(create, "create")).await;
        assert_eq!(other.status, 202, "{}", other.body);
        assert_ne!(
            first.body["result"]["sessionId"],
            other.body["result"]["sessionId"]
        );
        assert_eq!(
            agent.request("test/state", json!({})).await.unwrap()["created"],
            2
        );
        agent.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_permission_answers_apply_exactly_once() {
        let (state, agent, _directory) = setup().await;
        let phone = pair(&state).await;
        let desktop = pair(&state).await;
        phone
            .command(envelope(prompt("one", "permission"), "prompt"))
            .await;
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while state.session.lock().await.pending_permissions.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let request = state
            .session
            .lock()
            .await
            .pending_permissions
            .keys()
            .next()
            .unwrap()
            .clone();
        let answer = |option: &str| Operation::DecidePermission {
            session_id: "one".into(),
            request_id: request.clone(),
            option_id: option.into(),
        };
        let (a, b) = tokio::join!(
            phone.command(envelope(answer("allow-once"), "answer")),
            desktop.command(envelope(answer("reject-once"), "answer"))
        );
        assert_eq!(
            usize::from(a.status == 202) + usize::from(b.status == 202),
            1,
            "{} {}",
            a.body,
            b.body
        );
        let result = wait_for(&agent, |value| {
            value["accepted"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry.get("permission").is_some())
        })
        .await;
        assert_eq!(
            result["accepted"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry.get("permission").is_some())
                .count(),
            1
        );
        agent.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn targeted_stop_pauses_preserves_queue_and_stale_stop_cannot_hit_successor() {
        let (state, agent, _directory) = setup().await;
        let phone = pair(&state).await;
        phone
            .command(envelope(prompt("one", "first"), "first"))
            .await;
        wait_for(&agent, |value| {
            value["accepted"].as_array().unwrap().len() == 1
        })
        .await;
        phone
            .command(envelope(prompt("one", "second"), "second"))
            .await;
        let first = state.execution.lock().await.snapshot("one")["activeTurn"]["turnId"]
            .as_str()
            .unwrap()
            .to_owned();
        let stop = phone
            .command(envelope(
                Operation::CancelSessionTurn {
                    session_id: "one".into(),
                    expected_turn_id: first.clone(),
                    queue_disposition: QueueDisposition::Pause,
                },
                "stop",
            ))
            .await;
        assert_eq!(stop.status, 202, "{}", stop.body);
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if state.execution.lock().await.snapshot("one")["activeTurn"].is_null() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let snapshot = state.execution.lock().await.snapshot("one");
        assert_eq!(snapshot["paused"], true);
        assert_eq!(snapshot["queue"][0]["text"], "second");
        let resume = phone
            .command(envelope(
                Operation::SetQueuePaused {
                    session_id: "one".into(),
                    paused: false,
                    expected_queue_revision: snapshot["revision"].as_u64().unwrap(),
                },
                "resume",
            ))
            .await;
        assert_eq!(resume.status, 202, "{}", resume.body);
        wait_for(&agent, |value| {
            value["accepted"].as_array().unwrap().len() == 2
        })
        .await;
        let stale = phone
            .command(envelope(
                Operation::CancelSessionTurn {
                    session_id: "one".into(),
                    expected_turn_id: first,
                    queue_disposition: QueueDisposition::Pause,
                },
                "stale-stop",
            ))
            .await;
        assert_eq!(stale.body["error"], "turn_changed");
        assert_eq!(
            agent.request("test/state", json!({})).await.unwrap()["running"],
            json!(["one"])
        );
        agent.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn review_requires_idle_and_leaves_pending_messages_paused_without_replay() {
        let (state, agent, _directory) = setup().await;
        let phone = pair(&state).await;
        state.session.lock().await.set_running("one", true);
        phone
            .command(envelope(prompt("one", "ambiguous"), "ambiguous"))
            .await;
        phone
            .command(envelope(prompt("one", "pending"), "pending"))
            .await;
        let turn = state
            .execution
            .lock()
            .await
            .begin_turn("one")
            .unwrap()
            .unwrap();
        state
            .execution
            .lock()
            .await
            .finish_turn("one", &turn.turn_id, true, true)
            .unwrap();
        let review = Operation::ReviewUncertainTurn {
            session_id: "one".into(),
            expected_turn_id: turn.turn_id,
        };
        assert_eq!(
            phone
                .command(envelope(review.clone(), "busy-review"))
                .await
                .body["error"],
            "session_busy"
        );
        state.session.lock().await.set_running("one", false);
        let reviewed = phone.command(envelope(review, "review")).await;
        assert_eq!(reviewed.status, 202, "{}", reviewed.body);
        assert_eq!(reviewed.body["result"]["paused"], true);
        assert!(reviewed.body["result"]["activeTurn"].is_null());
        assert_eq!(reviewed.body["result"]["queue"][0]["text"], "pending");
        assert_eq!(
            state
                .execution
                .lock()
                .await
                .status(phone.session_id(), "ambiguous")
                .unwrap()["state"],
            "reviewed"
        );
        assert_eq!(
            agent.request("test/state", json!({})).await.unwrap()["accepted"],
            json!([])
        );
        agent.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_close_and_enqueue_cannot_both_succeed() {
        let (state, agent, _directory) = setup().await;
        let phone = pair(&state).await;
        let desktop = pair(&state).await;
        let (close, queued) = tokio::join!(
            phone.command(envelope(
                Operation::CloseSession {
                    session_id: "one".into()
                },
                "close"
            )),
            desktop.command(envelope(prompt("one", "message"), "send"))
        );
        assert_eq!(
            usize::from(close.status == 202) + usize::from(queued.status == 202),
            1,
            "{} {}",
            close.body,
            queued.body
        );
        if close.status == 202 {
            assert!(!state.session.lock().await.sessions.contains_key("one"));
            assert_eq!(
                state.execution.lock().await.snapshot("one")["queue"],
                json!([])
            );
        } else {
            assert_eq!(close.body["error"], "session_busy");
        }
        agent.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn initial_restore_keeps_live_deltas_and_does_not_emit_historical_chunks_twice() {
        let (state, agent, _directory) = setup().await;
        let phone = pair(&state).await;
        let result = phone
            .command(envelope(
                Operation::LoadSession {
                    workspace_id: "w".into(),
                    session_id: "history".into(),
                },
                "load-history",
            ))
            .await;
        assert_eq!(result.status, 202, "{}", result.body);
        agent.flush_events().await.unwrap();
        let (_, frames) = state.journal.lock().await.snapshot_frames();
        let snapshot = frames
            .iter()
            .find_map(|frame| match &frame.event {
                Event::SessionSnapshot {
                    session_id,
                    messages,
                    ..
                } if session_id == "history" => Some(messages),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            snapshot
                .iter()
                .map(|message| message.text.as_str())
                .collect::<String>(),
            "history live"
        );
        let crate::journal::ReplayOutcome::Replay(events) = phone.events(0).await.unwrap() else {
            panic!("events");
        };
        assert!(!events.iter().any(|event|matches!(&event.event,Event::MessageDelta {session_id,text} if session_id=="history" && text=="history")));
        assert!(
            state
                .session
                .lock()
                .await
                .session("history")
                .unwrap()
                .running
        );
        agent.shutdown().await.unwrap();
    }
}
