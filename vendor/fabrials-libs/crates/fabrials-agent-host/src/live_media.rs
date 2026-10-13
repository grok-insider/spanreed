use std::sync::Arc;

use serde_json::Value;

use crate::media::PromptImage;
use crate::protocol::Event;
use crate::server::HostState;

impl HostState {
    pub(crate) async fn absorb_image(self: &Arc<Self>, params: &Value) -> bool {
        if params
            .pointer("/update/content/type")
            .and_then(Value::as_str)
            != Some("image")
        {
            return false;
        }
        let role = match params
            .pointer("/update/sessionUpdate")
            .and_then(Value::as_str)
        {
            Some("user_message_chunk") => "user",
            Some("agent_message_chunk") => "agent",
            _ => return true,
        };
        if params
            .pointer("/update/_meta/hideFromScrollback")
            .and_then(Value::as_bool)
            == Some(true)
        {
            return true;
        }
        let Some(session_id) = params.get("sessionId").and_then(Value::as_str) else {
            return true;
        };
        let Some(opened_at) = self
            .session
            .lock()
            .await
            .sessions
            .get(session_id)
            .map(|live| live.opened_at_ms)
        else {
            return true;
        };
        let permit = self.image_decoders.clone().try_acquire_owned();
        let mut random = [0_u8; 16];
        if getrandom::fill(&mut random).is_err() {
            self.emit_event(
                Event::ImageRejected {
                    session_id: session_id.into(),
                },
                None,
            )
            .await;
            return true;
        }
        let image_id = format!("image-{:032x}", u128::from_le_bytes(random));
        self.emit_event(
            Event::MessageImage {
                session_id: session_id.into(),
                role: role.into(),
                image_id: image_id.clone(),
                phase: if permit.is_ok() {
                    "pending"
                } else {
                    "unavailable"
                }
                .into(),
                image: None,
            },
            None,
        )
        .await;
        let Ok(permit) = permit else {
            return true;
        };
        let block = params["update"]["content"].clone();
        let session_id = session_id.to_owned();
        let state = self.clone();
        tokio::spawn(async move {
            let image = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                PromptImage::from_acp(&block)
            })
            .await
            .ok()
            .and_then(Result::ok);
            let mut session = state.session.lock().await;
            if !session
                .sessions
                .get(&session_id)
                .is_some_and(|live| live.opened_at_ms == opened_at)
            {
                return;
            }
            if let Some(image) = &image {
                session
                    .transcript_media
                    .remember(&session_id, std::slice::from_ref(image));
            }
            let descriptor = image.as_ref().map(PromptImage::descriptor);
            drop(session);
            state
                .emit_event(
                    Event::MessageImage {
                        session_id,
                        role: role.into(),
                        image_id,
                        phase: if image.is_some() {
                            "ready"
                        } else {
                            "unavailable"
                        }
                        .into(),
                        image: descriptor,
                    },
                    None,
                )
                .await;
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::LiveSession;
    use crate::journal::ReplayOutcome;

    #[tokio::test]
    async fn live_images_keep_a_stable_slot_and_hide_internal_content() {
        let state = Arc::new(HostState::new(
            crate::origin::LocalOrigin::new("0123456789abcdef0123456789abcdef", 21219).unwrap(),
        ));
        state
            .session
            .lock()
            .await
            .open_session(LiveSession {
                id: "s".into(),
                workspace_id: "w".into(),
                workspace_name: "Synthetic".into(),
                running: false,
                queued: vec![],
                opened_at_ms: 1,
            })
            .unwrap();
        let image = image::RgbImage::from_pixel(32, 32, image::Rgb([1, 2, 3]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let image = PromptImage::normalize(bytes.get_ref()).unwrap();
        let mut params = serde_json::json!({"sessionId":"s","update":{"sessionUpdate":"user_message_chunk","content":image.acp_block()}});
        params["update"]["_meta"]["hideFromScrollback"] = Value::Bool(true);
        assert!(state.absorb_image(&params).await);
        assert_eq!(state.journal.lock().await.emitted_through(), 0);
        params["update"]["_meta"]["hideFromScrollback"] = Value::Bool(false);
        assert!(state.absorb_image(&params).await);
        state
            .emit_event(
                Event::MessageDelta {
                    session_id: "s".into(),
                    text: "next message".into(),
                },
                None,
            )
            .await;
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if state.journal.lock().await.emitted_through() >= 3 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let ReplayOutcome::Replay(events) = state.journal.lock().await.replay_after(0) else {
            panic!("replay")
        };
        let Event::MessageImage {
            image_id, phase, ..
        } = &events[0].event
        else {
            panic!("pending")
        };
        assert_eq!(phase, "pending");
        assert!(matches!(&events[1].event, Event::MessageDelta { .. }));
        assert!(
            matches!(&events[2].event,Event::MessageImage {image_id:id,phase,image:Some(descriptor),..} if id == image_id && phase == "ready" && descriptor == &image.descriptor())
        );
        assert!(
            state
                .session
                .lock()
                .await
                .transcript_media
                .chunk("s", &image.descriptor().sha256, 0)
                .is_ok()
        );
        let permits = state
            .image_decoders
            .clone()
            .acquire_many_owned(2)
            .await
            .unwrap();
        assert!(state.absorb_image(&params).await);
        let ReplayOutcome::Replay(events) = state.journal.lock().await.replay_after(3) else {
            panic!("replay")
        };
        assert!(
            matches!(&events[0].event,Event::MessageImage {phase,..} if phase == "unavailable")
        );
        drop(permits);
    }
}
