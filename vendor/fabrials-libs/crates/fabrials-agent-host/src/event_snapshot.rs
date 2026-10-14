use std::collections::BTreeMap;

use crate::protocol::{Event, SnapshotMessage, SnapshotTool};

#[derive(Debug, Default)]
pub(crate) struct Snapshots {
    sessions: BTreeMap<String, Event>,
    statuses: BTreeMap<String, Event>,
    pending: BTreeMap<(String, String), Event>,
    echoes: BTreeMap<String, String>,
    image_messages: BTreeMap<(String, String), i64>,
}

impl Snapshots {
    pub fn current(&self, id: &str) -> Option<Event> {
        self.sessions.get(id).cloned()
    }
    pub fn running(&self, id: &str) -> Option<bool> {
        match self.statuses.get(id) {
            Some(Event::SessionStatus { state, .. }) if state == "running" => Some(true),
            Some(Event::SessionStatus { state, .. }) if state == "idle" => Some(false),
            _ => None,
        }
    }

    pub fn clear_session(&mut self, id: &str) {
        self.sessions.remove(id);
        self.statuses.remove(id);
        self.echoes.remove(id);
        self.image_messages.retain(|(session, _), _| session != id);
        self.pending.retain(|(session, _), _| session != id);
    }

    pub fn restore(&self, fallback: Event) -> Event {
        let Event::SessionSnapshot {
            session_id,
            members,
            workflows,
            background_tasks,
            ..
        } = &fallback
        else {
            return fallback;
        };
        let Some(Event::SessionSnapshot {
            messages,
            tools,
            background_tasks: live_tasks,
            omitted_messages,
            omitted_tools,
            history_truncated,
            ..
        }) = self.sessions.get(session_id)
        else {
            return fallback;
        };
        if messages.is_empty() && tools.is_empty() && live_tasks.is_empty() {
            return fallback;
        }
        Event::SessionSnapshot {
            session_id: session_id.clone(),
            messages: messages.clone(),
            tools: tools.clone(),
            members: members.clone(),
            workflows: workflows.clone(),
            background_tasks: if live_tasks.is_empty() {
                background_tasks.clone()
            } else {
                live_tasks.clone()
            },
            omitted_messages: *omitted_messages,
            omitted_tools: *omitted_tools,
            history_truncated: *history_truncated,
        }
    }

    pub fn pending_for(&self, id: &str) -> Vec<Event> {
        self.pending
            .iter()
            .filter(|((session, _), _)| session == id)
            .map(|(_, event)| event.clone())
            .collect()
    }

    pub fn absorb(&mut self, event: &Event) {
        match event {
            Event::SessionSnapshot { session_id, .. } => {
                self.sessions.insert(session_id.clone(), event.clone());
            }
            Event::SessionStatus { session_id, state } => {
                if matches!(state.as_str(), "closed" | "gone") {
                    self.sessions.remove(session_id);
                    self.echoes.remove(session_id);
                    self.statuses.remove(session_id);
                    self.pending.retain(|(id, _), _| id != session_id);
                } else {
                    self.statuses.insert(session_id.clone(), event.clone());
                }
            }
            Event::PermissionRequest {
                session_id,
                request_id,
                ..
            }
            | Event::InteractionRequest {
                session_id,
                request_id,
                ..
            } => {
                self.pending
                    .insert((session_id.clone(), request_id.clone()), event.clone());
            }
            Event::InteractionResolved {
                session_id,
                request_id,
            } => {
                self.pending
                    .remove(&(session_id.clone(), request_id.clone()));
            }
            Event::PromptSent {
                session_id,
                text,
                images,
                ..
            } => {
                self.echoes.insert(session_id.clone(), text.clone());
                self.message(session_id, "user", text, images, true);
            }
            Event::UserMessageDelta { session_id, text } => {
                if let Some(remaining) = self.echoes.get_mut(session_id) {
                    if let Some(suffix) = remaining.strip_prefix(text) {
                        *remaining = suffix.into();
                        return;
                    }
                }
                self.echoes.remove(session_id);
                self.message(session_id, "user", text, &[], false);
            }
            Event::MessageDelta { session_id, text } => {
                self.message(session_id, "agent", text, &[], false)
            }
            Event::ToolStart {
                session_id,
                tool_call_id,
                name,
                action,
                read_only,
                provider,
                detail,
            } => {
                if let Event::SessionSnapshot {
                    messages,
                    tools,
                    omitted_tools,
                    ..
                } = self.session(session_id)
                {
                    if !tools.iter().any(|tool| tool.tool_call_id == *tool_call_id) {
                        let seq = next_seq(messages, tools);
                        tools.push(SnapshotTool {
                            tool_call_id: tool_call_id.clone(),
                            name: name.clone(),
                            action: action.clone(),
                            read_only: *read_only,
                            provider: provider.clone(),
                            detail: detail.clone(),
                            finished: false,
                            failed: false,
                            seq,
                        });
                        if tools.len() > 128 {
                            tools.remove(0);
                            *omitted_tools = omitted_tools.saturating_add(1);
                        }
                    }
                }
            }
            Event::ToolProgress {
                session_id,
                tool_call_id,
                title,
                detail,
            } => {
                if let Event::SessionSnapshot { tools, .. } = self.session(session_id)
                    && let Some(tool) = tools
                        .iter_mut()
                        .find(|tool| tool.tool_call_id == *tool_call_id)
                {
                    if let Some(title) = title {
                        tool.name = title.clone();
                    }
                    if let Some(detail) = detail {
                        tool.detail = Some(detail.clone());
                    }
                }
            }
            Event::ToolEnd {
                session_id,
                tool_call_id,
                failed,
                ..
            } => {
                if let Event::SessionSnapshot { tools, .. } = self.session(session_id)
                    && let Some(tool) = tools
                        .iter_mut()
                        .find(|tool| tool.tool_call_id == *tool_call_id)
                {
                    tool.finished = true;
                    tool.failed = *failed;
                }
            }
            Event::BackgroundTaskUpdated { session_id, task } => {
                if let Event::SessionSnapshot {
                    background_tasks, ..
                } = self.session(session_id)
                {
                    if let Some(previous) = background_tasks
                        .iter_mut()
                        .find(|previous| previous.task_id == task.task_id)
                    {
                        *previous = task.clone();
                    } else if background_tasks.len() < crate::session_runtime::MAX_BACKGROUND_TASKS
                    {
                        background_tasks.push(task.clone());
                    }
                }
            }
            Event::MessageImage {
                session_id,
                role,
                image,
                image_id,
                phase,
            } => {
                let key = (session_id.clone(), image_id.clone());
                let previous = self.image_messages.get(&key).copied();
                if previous.is_none() {
                    self.message(session_id, role, "", &[], false);
                }
                let mut seq = None;
                if let Event::SessionSnapshot { messages, .. } = self.session(session_id) {
                    let message = if let Some(previous) = previous {
                        messages.iter_mut().find(|message| message.seq == previous)
                    } else {
                        messages.last_mut()
                    };
                    let Some(message) = message else {
                        return;
                    };
                    seq = Some(message.seq);
                    if let Some(image) = image {
                        if message.images.len() < crate::media::MAX_IMAGES
                            && !message
                                .images
                                .iter()
                                .any(|existing| existing.sha256 == image.sha256)
                        {
                            message.images.push(image.clone());
                        }
                    } else if phase == "unavailable" {
                        message.unavailable_images = message.unavailable_images.saturating_add(1);
                    }
                }
                if let Some(seq) = seq {
                    self.image_messages.insert(key, seq);
                }
            }
            _ => {}
        }
        while self.sessions.len() > crate::bounds::MAX_LIVE_SESSIONS {
            self.sessions.pop_first();
        }
        while self.statuses.len() > crate::bounds::MAX_LIVE_SESSIONS {
            self.statuses.pop_first();
        }
        while self.pending.len() > 128 {
            self.pending.pop_first();
        }
        while self.image_messages.len() > 1024 {
            self.image_messages.pop_first();
        }
    }

    fn session(&mut self, id: &str) -> &mut Event {
        self.sessions
            .entry(id.into())
            .or_insert_with(|| Event::SessionSnapshot {
                session_id: id.into(),
                messages: Vec::new(),
                tools: Vec::new(),
                members: Vec::new(),
                workflows: Vec::new(),
                background_tasks: Vec::new(),
                omitted_messages: 0,
                omitted_tools: 0,
                history_truncated: false,
            })
    }

    fn message(
        &mut self,
        id: &str,
        role: &str,
        text: &str,
        images: &[crate::media::ImageDescriptor],
        new: bool,
    ) {
        if let Event::SessionSnapshot {
            messages,
            tools,
            omitted_messages,
            history_truncated,
            ..
        } = self.session(id)
        {
            let seq = next_seq(messages, tools);
            if !new
                && messages
                    .last()
                    .is_some_and(|message| message.role == role && message.seq == seq - 1)
            {
                messages.last_mut().expect("message").text.push_str(text);
            } else {
                messages.push(SnapshotMessage {
                    role: role.into(),
                    text: text.into(),
                    seq,
                    images: images.into(),
                    unavailable_images: 0,
                });
            }
            while messages.len() > 512
                || messages
                    .iter()
                    .map(|message| message.text.len())
                    .sum::<usize>()
                    > crate::bounds::MAX_REHYDRATE_CHARS
            {
                if messages.len() == 1 {
                    messages[0].text = crate::bounds::truncate_utf8(
                        &messages[0].text,
                        crate::bounds::MAX_REHYDRATE_CHARS,
                    )
                    .0;
                    *history_truncated = true;
                    break;
                }
                messages.remove(0);
                *omitted_messages = omitted_messages.saturating_add(1);
            }
        }
    }

    pub fn events(&self) -> Vec<Event> {
        std::iter::once(Event::ReplayReset)
            .chain(self.sessions.values().cloned())
            .chain(self.statuses.values().cloned())
            .chain(self.pending.values().cloned())
            .collect()
    }
}

fn next_seq(messages: &[SnapshotMessage], tools: &[SnapshotTool]) -> i64 {
    messages
        .iter()
        .map(|message| message.seq)
        .chain(tools.iter().map(|tool| tool.seq))
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

pub(crate) fn session_id(event: &Event) -> Option<&str> {
    match event {
        Event::SessionSnapshot { session_id, .. }
        | Event::SessionStatus { session_id, .. }
        | Event::SessionExecutionChanged { session_id }
        | Event::SessionModelsChanged { session_id }
        | Event::MessageDelta { session_id, .. }
        | Event::UserMessageDelta { session_id, .. }
        | Event::MessageImage { session_id, .. }
        | Event::ThoughtDelta { session_id, .. }
        | Event::ToolStart { session_id, .. }
        | Event::ToolProgress { session_id, .. }
        | Event::ToolEnd { session_id, .. }
        | Event::BackgroundTaskUpdated { session_id, .. }
        | Event::PromptSent { session_id, .. }
        | Event::PermissionRequest { session_id, .. }
        | Event::InteractionRequest { session_id, .. }
        | Event::InteractionResolved { session_id, .. }
        | Event::PlanUpdated { session_id, .. }
        | Event::CommandsUpdated { session_id, .. }
        | Event::QueueChanged { session_id }
        | Event::SessionReviewUpdated { session_id, .. }
        | Event::TurnInterrupted { session_id, .. } => Some(session_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reconnect_snapshot_has_the_same_order_without_replaying_user_echoes() {
        let mut snapshots = Snapshots::default();
        snapshots.absorb(&Event::PromptSent {
            session_id: "s".into(),
            text: "question".into(),
            images: vec![],
            idempotency_key: None,
        });
        for text in ["ques", "tion"] {
            snapshots.absorb(&Event::UserMessageDelta {
                session_id: "s".into(),
                text: text.into(),
            });
        }
        for text in ["ans", "wer"] {
            snapshots.absorb(&Event::MessageDelta {
                session_id: "s".into(),
                text: text.into(),
            });
        }
        let events = snapshots.events();
        let Event::SessionSnapshot { messages, .. } = &events[1] else {
            panic!("snapshot");
        };
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text, "question");
        assert_eq!(messages[1].text, "answer");
        assert_eq!(snapshots.events(), events);
    }

    #[test]
    fn a_resolution_removes_only_its_own_session_request() {
        let mut snapshots = Snapshots::default();
        for session_id in ["a", "b"] {
            snapshots.absorb(&Event::PermissionRequest {
                session_id: session_id.into(),
                request_id: "same".into(),
                options: vec![],
                title: None,
                description: None,
            });
        }
        snapshots.absorb(&Event::InteractionResolved {
            session_id: "a".into(),
            request_id: "same".into(),
        });
        assert_eq!(snapshots.pending.len(), 1);
        assert!(snapshots.pending.contains_key(&("b".into(), "same".into())));
    }

    #[test]
    fn a_bounded_window_counts_the_messages_and_tools_it_leaves_out() {
        let mut snapshots = Snapshots::default();
        for index in 0..600 {
            snapshots.absorb(&Event::PromptSent {
                session_id: "s".into(),
                text: format!("q{index}"),
                images: vec![],
                idempotency_key: None,
            });
            snapshots.absorb(&Event::MessageDelta {
                session_id: "s".into(),
                text: format!("a{index}"),
            });
        }
        for index in 0..130 {
            snapshots.absorb(&Event::ToolStart {
                session_id: "s".into(),
                tool_call_id: format!("t{index}"),
                name: "read".into(),
                action: "read".into(),
                read_only: true,
                provider: None,
                detail: None,
            });
        }
        let Some(event) = snapshots.current("s") else {
            panic!("snapshot");
        };
        let Event::SessionSnapshot {
            messages,
            tools,
            omitted_messages,
            omitted_tools,
            history_truncated,
            ..
        } = &event
        else {
            panic!("snapshot");
        };
        assert_eq!(messages.len(), 512);
        assert_eq!(*omitted_messages, 688);
        assert_eq!(messages[0].text, "q344");
        assert_eq!(tools.len(), 128);
        assert_eq!(*omitted_tools, 2);
        assert!(!history_truncated);
        let wire = serde_json::to_value(&event).expect("serializes");
        assert_eq!(wire["omittedMessages"], 688);
        assert_eq!(wire["omittedTools"], 2);
        assert!(wire.get("historyTruncated").is_none());
        let restored = snapshots.restore(Event::SessionSnapshot {
            session_id: "s".into(),
            messages: vec![],
            tools: vec![],
            members: vec![],
            workflows: vec![],
            background_tasks: vec![],
            omitted_messages: 0,
            omitted_tools: 0,
            history_truncated: false,
        });
        assert!(matches!(
            restored,
            Event::SessionSnapshot {
                omitted_messages: 688,
                omitted_tools: 2,
                ..
            }
        ));
    }

    #[test]
    fn a_complete_snapshot_keeps_the_previous_wire_shape() {
        let mut snapshots = Snapshots::default();
        snapshots.absorb(&Event::MessageDelta {
            session_id: "s".into(),
            text: "answer".into(),
        });
        let wire =
            serde_json::to_value(snapshots.current("s").expect("snapshot")).expect("serializes");
        for field in ["omittedMessages", "omittedTools", "historyTruncated"] {
            assert!(
                wire.get(field).is_none(),
                "{field} must stay off the wire when nothing was left out"
            );
        }
    }

    #[test]
    fn a_single_message_cut_to_the_text_bound_marks_the_history_truncated() {
        let mut snapshots = Snapshots::default();
        snapshots.absorb(&Event::MessageDelta {
            session_id: "s".into(),
            text: "x".repeat(crate::bounds::MAX_REHYDRATE_CHARS + 10),
        });
        let Some(Event::SessionSnapshot {
            messages,
            omitted_messages,
            history_truncated,
            ..
        }) = snapshots.current("s")
        else {
            panic!("snapshot");
        };
        assert_eq!(messages.len(), 1);
        assert_eq!(omitted_messages, 0);
        assert!(history_truncated);
    }

    #[test]
    fn an_image_only_message_survives_a_late_decode_after_the_reply() {
        let mut snapshots = Snapshots::default();
        let image = crate::media::ImageDescriptor {
            byte_length: 20,
            mime_type: "image/png".into(),
            width: 1,
            height: 1,
            sha256: "abc".into(),
        };
        snapshots.absorb(&Event::MessageImage {
            session_id: "s".into(),
            role: "user".into(),
            image_id: "i".into(),
            phase: "pending".into(),
            image: None,
        });
        snapshots.absorb(&Event::MessageDelta {
            session_id: "s".into(),
            text: "reply".into(),
        });
        snapshots.absorb(&Event::MessageImage {
            session_id: "s".into(),
            role: "user".into(),
            image_id: "i".into(),
            phase: "ready".into(),
            image: Some(image.clone()),
        });
        let Some(Event::SessionSnapshot { messages, .. }) = snapshots.current("s") else {
            panic!("snapshot")
        };
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].images, vec![image]);
        assert_eq!(messages[0].unavailable_images, 0);
        assert_eq!(messages[1].text, "reply");
    }
}
