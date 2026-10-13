use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine;

use crate::media::{CHUNK_BYTES, ImageDescriptor, MediaError, PromptImage};
use crate::protocol::{CommandEnvelope, Operation, PROTOCOL_VERSION};
use crate::server::HostState;

pub(crate) fn handles(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::StartImageUpload { .. }
            | Operation::AppendImageUpload { .. }
            | Operation::FinishImageUpload { .. }
            | Operation::DiscardImage { .. }
            | Operation::ReadImageChunk { .. }
    )
}

fn error_code(error: MediaError) -> &'static str {
    match error {
        MediaError::Size => "image_size_limit",
        MediaError::Format => "image_format_unsupported",
        MediaError::Decode => "image_decode_failed",
        MediaError::Dimensions => "image_dimensions_unsupported",
        MediaError::Unknown => "image_unavailable",
        MediaError::Capacity => "image_capacity",
        MediaError::Offset => "image_chunk_offset",
        MediaError::Integrity => "image_integrity",
    }
}

pub(crate) async fn validate_prompt(
    state: &HostState,
    principal: &str,
    operation: &Operation,
) -> Result<(), &'static str> {
    if let Operation::PromptContent {
        session_id,
        image_ids,
        ..
    } = operation
    {
        if !state
            .upstream_prompt
            .lock()
            .await
            .as_ref()
            .is_some_and(|capabilities| capabilities.image)
        {
            return Err("image_not_supported");
        }
        let mut session = state.session.lock().await;
        session.session(session_id).map_err(|_| "unknown_session")?;
        session
            .media
            .resolve(Some(principal), session_id, image_ids, crate::now_ms())
            .map_err(error_code)?;
    }
    Ok(())
}

pub(crate) async fn command(
    state: &HostState,
    principal: &str,
    envelope: &CommandEnvelope,
) -> Response {
    let result = dispatch(state, principal, &envelope.operation).await;
    match result {
        Ok(result) => (StatusCode::OK, axum::Json(serde_json::json!({"protocolVersion": PROTOCOL_VERSION,"requestId":envelope.request_id,"result":result}))).into_response(),
        Err(error) => (StatusCode::UNPROCESSABLE_ENTITY, axum::Json(serde_json::json!({"protocolVersion": PROTOCOL_VERSION,"requestId":envelope.request_id,"error":error}))).into_response(),
    }
}

async fn dispatch(
    state: &HostState,
    principal: &str,
    operation: &Operation,
) -> Result<serde_json::Value, &'static str> {
    if let Operation::ReadImageChunk {
        session_id,
        sha256,
        offset,
    } = operation
    {
        let session = state.session.lock().await;
        session.session(session_id).map_err(|_| "unknown_session")?;
        return session
            .transcript_media
            .chunk(session_id, sha256, *offset)
            .map_err(error_code);
    }
    if let Operation::DiscardImage { image_id } = operation {
        state
            .session
            .lock()
            .await
            .media
            .discard(principal, image_id)
            .map_err(error_code)?;
        return Ok(serde_json::json!({"outcome":"imageDiscarded"}));
    }
    if !state
        .upstream_prompt
        .lock()
        .await
        .as_ref()
        .is_some_and(|capabilities| capabilities.image)
    {
        return Err("image_not_supported");
    }
    let mut session = state.session.lock().await;
    let now = crate::now_ms();
    match operation {
        Operation::StartImageUpload {
            session_id,
            byte_length,
            sha256,
        } => {
            session.session(session_id).map_err(|_| "unknown_session")?;
            let id = session
                .media
                .begin(principal, session_id, *byte_length, sha256, now)
                .map_err(error_code)?;
            Ok(
                serde_json::json!({"outcome":"imageUploadStarted","uploadId":id,"chunkBytes":CHUNK_BYTES,"expiresAt":now.saturating_add(crate::media::TRANSFER_TTL_MS)}),
            )
        }
        Operation::AppendImageUpload {
            session_id,
            upload_id,
            offset,
            data,
        } => {
            session.session(session_id).map_err(|_| "unknown_session")?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|_| "image_integrity")?;
            let offset = session
                .media
                .append(principal, session_id, upload_id, *offset, &bytes, now)
                .map_err(error_code)?;
            Ok(serde_json::json!({"outcome":"imageChunkAccepted","offset":offset}))
        }
        Operation::FinishImageUpload {
            session_id,
            upload_id,
        } => {
            session.session(session_id).map_err(|_| "unknown_session")?;
            let permit = state
                .image_decoders
                .clone()
                .try_acquire_owned()
                .map_err(|_| "image_capacity")?;
            let bytes = session
                .media
                .finish(principal, session_id, upload_id, now)
                .map_err(error_code)?;
            drop(session);
            let image = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                PromptImage::normalize(&bytes)
            })
            .await
            .map_err(|_| "image_decode_failed")?
            .map_err(error_code)?;
            let descriptor: ImageDescriptor = image.descriptor();
            let broker = state.pairing.lock().await;
            if !broker
                .session_ids(crate::now_ms())
                .iter()
                .any(|id| id == principal)
            {
                return Err("session_invalid");
            }
            let mut session = state.session.lock().await;
            session.session(session_id).map_err(|_| "unknown_session")?;
            let id = session
                .media
                .insert(principal, session_id, image, crate::now_ms())
                .map_err(error_code)?;
            Ok(serde_json::json!({"outcome":"imageReady","imageId":id,"image":descriptor}))
        }
        _ => Err("unsupported"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{AgentCommand, AgentHandle, AgentMode};
    use crate::dispatch::LiveSession;
    use crate::journal::ReplayOutcome;
    use crate::protocol::Event;
    use crate::remote::RemoteAccess;
    use sha2::{Digest, Sha256};
    use std::sync::Arc;

    fn envelope(operation: Operation, epoch: u64, key: &str) -> CommandEnvelope {
        CommandEnvelope {
            protocol_version: crate::protocol::MULTI_WRITER_PROTOCOL_VERSION,
            request_id: key.into(),
            idempotency_key: Some(key.into()),
            controller_epoch: Some(epoch),
            expected_revision: None,
            deadline_ms: None,
            operation,
        }
    }

    async fn pair(state: &Arc<HostState>) -> RemoteAccess {
        let nonce = state
            .pairing
            .lock()
            .await
            .mint_nonce(crate::now_ms())
            .unwrap();
        RemoteAccess::pair(state.clone(), nonce.expose())
            .await
            .unwrap()
            .0
    }

    fn png() -> Vec<u8> {
        let image = image::RgbImage::from_pixel(32, 32, image::Rgb([55, 210, 11]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[tokio::test]
    async fn chunked_images_reach_acp_and_receipts_prevent_duplicate_turns() {
        let directory = tempfile::tempdir().unwrap();
        let state = Arc::new(
            HostState::new(
                crate::origin::LocalOrigin::new("0123456789abcdef0123456789abcdef", 21219).unwrap(),
            )
            .with_agent_mode(AgentMode::Shared)
            .with_persistence(
                directory.path(),
                Arc::new(crate::picker::UnavailableDirectoryPicker),
            )
            .unwrap(),
        );
        *state.upstream_prompt.lock().await = Some(crate::remote_management::PromptCapabilities {
            image: true,
            audio: false,
            embedded_context: true,
        });
        state
            .session
            .lock()
            .await
            .open_session(LiveSession {
                id: "s-1".into(),
                workspace_id: "w".into(),
                workspace_name: "Synthetic".into(),
                running: false,
                queued: vec![],
                opened_at_ms: 1,
            })
            .unwrap();
        let mut path = std::env::current_exe().unwrap();
        path.pop();
        path.pop();
        path.push("examples");
        path.push(format!("fake_agent{}", std::env::consts::EXE_SUFFIX));
        let (agent, events) =
            AgentHandle::spawn(&AgentCommand::new(path.to_string_lossy())).unwrap();
        agent.initialize().await.unwrap();
        state.attach_agent(agent.clone(), events);
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while state.agent.lock().await.is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let access = pair(&state).await;
        let epoch = access.connect_version(4).await.unwrap();
        let bytes = png();
        let reply = access
            .command(envelope(
                Operation::StartImageUpload {
                    session_id: "s-1".into(),
                    byte_length: bytes.len(),
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                },
                epoch,
                "start",
            ))
            .await;
        assert_eq!(reply.status, 200, "{}", reply.body);
        let upload_id = reply.body["result"]["uploadId"]
            .as_str()
            .unwrap()
            .to_owned();
        let chunk = Operation::AppendImageUpload {
            session_id: "s-1".into(),
            upload_id: upload_id.clone(),
            offset: 0,
            data: base64::engine::general_purpose::STANDARD.encode(&bytes),
        };
        let observer = pair(&state).await;
        assert!(observer.connect_version(4).await.is_ok());
        assert_eq!(
            observer
                .command(envelope(chunk.clone(), epoch, "foreign-chunk"))
                .await
                .status,
            422
        );
        assert_eq!(
            access.command(envelope(chunk, epoch, "chunk")).await.status,
            200
        );
        let complete = access
            .command(envelope(
                Operation::FinishImageUpload {
                    session_id: "s-1".into(),
                    upload_id,
                },
                epoch,
                "finish",
            ))
            .await;
        assert_eq!(complete.status, 200, "{}", complete.body);
        let id = complete.body["result"]["imageId"]
            .as_str()
            .unwrap()
            .to_owned();
        let hash = complete.body["result"]["image"]["sha256"].as_str().unwrap();
        let operation = Operation::PromptContent {
            session_id: "s-1".into(),
            text: "inspect-images".into(),
            image_ids: vec![id],
            send_now: false,
        };
        let reply = access
            .command(envelope(operation.clone(), epoch, "image-turn"))
            .await;
        assert_eq!(reply.status, 202, "{}", reply.body);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let status = access
                    .command(envelope(
                        Operation::GetCommandStatus {
                            idempotency_key: "image-turn".into(),
                        },
                        epoch,
                        "status",
                    ))
                    .await;
                if status.body["result"]["state"] == "completed" {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            access
                .command(envelope(operation.clone(), epoch, "image-turn"))
                .await
                .status,
            202
        );
        let ReplayOutcome::Replay(events) = access.events(0).await.unwrap() else {
            panic!("events")
        };
        let expected = format!("images:1:{hash}");
        assert_eq!(events.iter().filter(|entry| matches!(&entry.event, Event::MessageDelta {text,..} if text == &expected)).count(),1);
        assert!(
            !events
                .iter()
                .any(|entry| matches!(&entry.event, Event::MessageImage { .. }))
        );
        assert!(events.iter().any(|entry| matches!(&entry.event, Event::PromptSent {images,..} if images.len()==1 && images[0].sha256==hash)));
        let read = observer
            .command(envelope(
                Operation::ReadImageChunk {
                    session_id: "s-1".into(),
                    sha256: hash.into(),
                    offset: 0,
                },
                epoch,
                "read-image",
            ))
            .await;
        assert_eq!(read.status, 200, "{}", read.body);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(read.body["result"]["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(decoded)), hash);
        let foreign = observer
            .command(envelope(
                Operation::ReadImageChunk {
                    session_id: "another-session".into(),
                    sha256: hash.into(),
                    offset: 0,
                },
                epoch,
                "read-foreign",
            ))
            .await;
        assert_eq!(foreign.body["error"], "unknown_session");
        {
            let mut session = state.session.lock().await;
            session.set_running("s-1", true);
            session.transcript_media.forget_session("s-1");
        }
        assert_eq!(
            access
                .command(envelope(operation, epoch, "queued-image-turn"))
                .await
                .status,
            202
        );
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if state.execution.lock().await.snapshot("s-1")["queue"]
                    .as_array()
                    .is_some_and(|entries| !entries.is_empty())
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        {
            let mut session = state.session.lock().await;
            session.media.revoke(None);
            session.set_running("s-1", false);
        }
        state.drain_queue("s-1");
        tokio::time::timeout(std::time::Duration::from_secs(3),async {
            loop {
                if let ReplayOutcome::Replay(events) = access.events(0).await.unwrap()
                    && events.iter().filter(|entry| matches!(&entry.event,Event::MessageDelta {text,..} if text == &expected)).count() == 2 {break;}
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
        assert!(
            state
                .session
                .lock()
                .await
                .transcript_media
                .chunk("s-1", hash, 0)
                .is_ok()
        );
        observer.disconnect().await;
        access.disconnect().await;
        agent.shutdown().await.unwrap();
    }
}
