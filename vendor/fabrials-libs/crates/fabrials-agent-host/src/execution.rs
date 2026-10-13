use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::media::{ImageDescriptor, PromptImage};

pub(crate) const MAX_QUEUE_BYTES: usize = 100 * 1024 * 1024;
const MAX_RECEIPTS: usize = 65_536;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    pub entry_id: String,
    pub command_id: String,
    pub origin_device_id: String,
    pub text: String,
    pub images: Vec<ImageDescriptor>,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActiveTurn {
    pub turn_id: String,
    pub state: String,
    #[serde(default)]
    dispatched: bool,
    pub entry: Entry,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Execution {
    pub revision: u64,
    pub paused: bool,
    pub uncertain: bool,
    #[serde(default)]
    configuration_uncertain: bool,
    pub active_turn: Option<ActiveTurn>,
    pub queue: Vec<Entry>,
    #[serde(default)]
    configuring: bool,
    #[serde(default)]
    cancellation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Receipt {
    fingerprint: String,
    entry_id: String,
    session_id: String,
    state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Ledger {
    version: u32,
    next_id: u64,
    sessions: BTreeMap<String, Execution>,
    receipts: BTreeMap<String, Receipt>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            version: 1,
            next_id: 0,
            sessions: BTreeMap::new(),
            receipts: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ExecutionStore {
    ledger: Ledger,
    directory: Option<PathBuf>,
    images: BTreeMap<String, PromptImage>,
}

impl ExecutionStore {
    pub fn open(directory: &Path) -> Result<Self, &'static str> {
        let path = directory.join("session-execution.json");
        let ledger = match std::fs::metadata(&path) {
            Ok(meta) => {
                if meta.len() > MAX_QUEUE_BYTES as u64 + 32 * 1024 * 1024 {
                    return Err("queue_storage_corrupt");
                }
                let bytes = std::fs::read(&path).map_err(|_| "queue_storage_unavailable")?;
                let ledger: Ledger =
                    serde_json::from_slice(&bytes).map_err(|_| "queue_storage_corrupt")?;
                if ledger.version != 1
                    || ledger.receipts.len() > MAX_RECEIPTS
                    || ledger.sessions.len() > crate::bounds::MAX_LIVE_SESSIONS
                {
                    return Err("queue_storage_corrupt");
                }
                ledger
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ledger::default(),
            Err(_) => return Err("queue_storage_unavailable"),
        };
        let mut store = Self {
            ledger,
            directory: Some(directory.into()),
            images: BTreeMap::new(),
        };
        for session in store.ledger.sessions.values_mut() {
            if session.active_turn.is_some()
                || session.configuring
                || session.cancellation.is_some()
            {
                session.uncertain = session.active_turn.is_some() || session.cancellation.is_some();
                session.configuration_uncertain |= session.configuring;
                session.paused = true;
                session.configuring = false;
                session.cancellation = None;
                if let Some(turn) = &mut session.active_turn {
                    turn.state = "uncertain".into();
                    if let Some(receipt) = store.ledger.receipts.get_mut(&turn.entry.command_id) {
                        receipt.state = "needsReview".into();
                    }
                }
            }
            if !session.queue.is_empty() {
                session.paused = true;
            }
        }
        store.validate_budget()?;
        store.persist()?;
        store.collect_images();
        Ok(store)
    }

    pub fn snapshot(&self, session_id: &str) -> serde_json::Value {
        let execution = self
            .ledger
            .sessions
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        let mut value = serde_json::to_value(execution).expect("execution serializes");
        if let Some(turn) = &self
            .ledger
            .sessions
            .get(session_id)
            .and_then(|state| state.active_turn.as_ref())
        {
            value["activeTurn"]["originDeviceId"] = turn.entry.origin_device_id.clone().into();
        }
        value["uncertain"] = self
            .ledger
            .sessions
            .get(session_id)
            .is_some_and(|state| state.uncertain || state.configuration_uncertain)
            .into();
        value["outcome"] = "sessionExecution".into();
        value["sessionId"] = session_id.into();
        value
    }

    pub fn status(&self, principal: &str, key: &str) -> Option<serde_json::Value> {
        let command_id = crate::receipts::scoped_key(principal, key);
        self.ledger.receipts.get(&command_id).map(|receipt| {
            serde_json::json!({
                "outcome":"commandStatus", "idempotencyKey":key, "commandId":command_id,
                "sessionId":receipt.session_id, "entryId":receipt.entry_id, "state":receipt.state
            })
        })
    }

    pub fn enqueue(
        &mut self,
        session_id: &str,
        principal: &str,
        key: &str,
        text: &str,
        images: &[PromptImage],
        fingerprint: &str,
    ) -> Result<serde_json::Value, &'static str> {
        self.enqueue_inner(session_id, principal, key, text, images, fingerprint, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn interrupt_enqueue(
        &mut self,
        session_id: &str,
        principal: &str,
        key: &str,
        text: &str,
        images: &[PromptImage],
        fingerprint: &str,
        turn: &str,
        revision: u64,
    ) -> Result<serde_json::Value, &'static str> {
        self.enqueue_inner(
            session_id,
            principal,
            key,
            text,
            images,
            fingerprint,
            Some((turn, revision)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn enqueue_inner(
        &mut self,
        session_id: &str,
        principal: &str,
        key: &str,
        text: &str,
        images: &[PromptImage],
        fingerprint: &str,
        interrupt: Option<(&str, u64)>,
    ) -> Result<serde_json::Value, &'static str> {
        let command_id = crate::receipts::scoped_key(principal, key);
        if let Some(receipt) = self.ledger.receipts.get(&command_id) {
            if receipt.fingerprint != fingerprint || receipt.session_id != session_id {
                return Err("idempotency_conflict");
            }
            return Ok(
                serde_json::json!({"outcome":"commandAccepted","commandId":command_id,"entryId":receipt.entry_id,"sessionId":session_id,"state":receipt.state}),
            );
        }
        if self.ledger.receipts.len() >= MAX_RECEIPTS {
            return Err("command_history_full");
        }
        if !self.ledger.sessions.contains_key(session_id)
            && self.ledger.sessions.len() >= crate::bounds::MAX_LIVE_SESSIONS
        {
            return Err("too_many_sessions");
        }
        if self
            .ledger
            .sessions
            .get(session_id)
            .is_some_and(|state| state.queue.len() >= crate::bounds::MAX_QUEUED_PROMPTS)
        {
            return Err("queue_full");
        }
        if text.len() > crate::bounds::MAX_COMMAND_BODY_BYTES || text.trim_start().starts_with('!')
        {
            return Err("remote_shell_disabled");
        }
        if let Some((turn, revision)) = interrupt {
            let state = self.ledger.sessions.get(session_id).ok_or("turn_changed")?;
            if state.revision != revision {
                return Err("queue_revision_conflict");
            }
            if !state.active_turn.as_ref().is_some_and(|active| {
                active.turn_id == turn && matches!(active.state.as_str(), "starting" | "running")
            }) {
                return Err("turn_changed");
            }
        }
        self.validate_additional_bytes(text, images)?;
        for image in images {
            self.store_image(image)?;
        }
        self.transact(|ledger| {
            ledger.next_id = ledger.next_id.checked_add(1).ok_or("command_history_full")?;
            let entry_id = format!("queued-{}", ledger.next_id);
            let state = ledger.sessions.entry(session_id.into()).or_default();
            state.queue.push(Entry { entry_id:entry_id.clone(), command_id:command_id.clone(), origin_device_id:principal.into(), text:text.into(), images:images.iter().map(PromptImage::descriptor).collect(), state:"pending".into() });
            if interrupt.is_some() {
                let entry = state.queue.pop().expect("new entry");
                state.queue.insert(0,entry);
                state.paused = false;
                state.active_turn.as_mut().expect("checked turn").state = "cancelling".into();
                state.cancellation = interrupt.map(|(id,_)|id.into());
            }
            state.revision = state.revision.checked_add(1).ok_or("queue_revision_exhausted")?;
            ledger.receipts.insert(command_id.clone(), Receipt { fingerprint:fingerprint.into(), entry_id:entry_id.clone(), session_id:session_id.into(), state:"queued".into() });
            Ok(serde_json::json!({"outcome":"commandAccepted","commandId":command_id,"entryId":entry_id,"sessionId":session_id,"state":"queued","revision":state.revision}))
        })
    }

    pub fn begin_turn(&mut self, session_id: &str) -> Result<Option<ActiveTurn>, &'static str> {
        self.transact(|ledger| {
            let Some(state) = ledger.sessions.get_mut(session_id) else {
                return Ok(None);
            };
            if state.paused
                || state.uncertain
                || state.configuration_uncertain
                || state.configuring
                || state.cancellation.is_some()
                || state.active_turn.is_some()
                || state.queue.is_empty()
            {
                return Ok(None);
            }
            if state.queue[0].state != "pending" {
                state.paused = true;
                return Ok(None);
            }
            let entry = state.queue.remove(0);
            let turn = ActiveTurn {
                turn_id: format!("turn-{}", entry.entry_id),
                state: "starting".into(),
                dispatched: false,
                entry,
            };
            ledger
                .receipts
                .get_mut(&turn.entry.command_id)
                .ok_or("queue_storage_corrupt")?
                .state = "running".into();
            state.active_turn = Some(turn.clone());
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or("queue_revision_exhausted")?;
            Ok(Some(turn))
        })
    }

    pub fn mark_dispatched(
        &mut self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<bool, &'static str> {
        self.transact(|ledger| {
            let state = ledger
                .sessions
                .get_mut(session_id)
                .ok_or("unknown_session")?;
            let turn = state
                .active_turn
                .as_mut()
                .filter(|turn| turn.turn_id == turn_id)
                .ok_or("turn_changed")?;
            turn.dispatched = true;
            if turn.state == "starting" {
                turn.state = "running".into();
            }
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or("queue_revision_exhausted")?;
            Ok(state.cancellation.as_deref() == Some(turn_id))
        })
    }

    pub fn cancellation_ready(&self, session_id: &str, turn_id: &str) -> bool {
        self.ledger.sessions.get(session_id).is_some_and(|state| {
            state
                .active_turn
                .as_ref()
                .is_some_and(|turn| turn.turn_id == turn_id && turn.dispatched)
                && state.cancellation.as_deref() == Some(turn_id)
        })
    }

    pub fn finish_turn(
        &mut self,
        session_id: &str,
        turn_id: &str,
        uncertain: bool,
        failed: bool,
    ) -> Result<(), &'static str> {
        self.transact(|ledger| {
            let state = ledger
                .sessions
                .get_mut(session_id)
                .ok_or("unknown_session")?;
            let turn = state
                .active_turn
                .as_mut()
                .filter(|turn| turn.turn_id == turn_id)
                .ok_or("turn_changed")?;
            let receipt = ledger
                .receipts
                .get_mut(&turn.entry.command_id)
                .ok_or("queue_storage_corrupt")?;
            receipt.state = if uncertain {
                "needsReview"
            } else if failed {
                "failed"
            } else {
                "completed"
            }
            .into();
            if uncertain {
                state.uncertain = true;
                state.paused = true;
                turn.state = "uncertain".into();
            } else {
                state.active_turn = None;
                state.uncertain = false;
            }
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or("queue_revision_exhausted")?;
            Ok(())
        })
    }

    pub fn cancel(
        &mut self,
        session_id: &str,
        turn_id: &str,
        pause: bool,
    ) -> Result<bool, &'static str> {
        self.transact(|ledger| {
            let state = ledger.sessions.get_mut(session_id).ok_or("turn_changed")?;
            let turn = state
                .active_turn
                .as_mut()
                .filter(|turn| turn.turn_id == turn_id)
                .ok_or("turn_changed")?;
            if turn.state == "uncertain" {
                return Err("interrupted_needs_review");
            }
            if pause {
                state.paused = true;
            }
            let first = turn.state != "cancelling";
            turn.state = "cancelling".into();
            if first {
                state.cancellation = Some(turn_id.into());
            }
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or("queue_revision_exhausted")?;
            Ok(first && turn.dispatched)
        })
    }

    pub fn finish_cancel(
        &mut self,
        session_id: &str,
        turn_id: &str,
        uncertain: bool,
    ) -> Result<(), &'static str> {
        self.transact(|ledger| {
            let state = ledger
                .sessions
                .get_mut(session_id)
                .ok_or("unknown_session")?;
            if state.cancellation.as_deref() != Some(turn_id) {
                return Err("turn_changed");
            }
            state.cancellation = None;
            if uncertain {
                state.uncertain = state.active_turn.is_some();
                state.paused = true;
            }
            Ok(())
        })
    }

    pub fn set_paused(
        &mut self,
        session_id: &str,
        paused: bool,
        revision: u64,
    ) -> Result<(), &'static str> {
        self.transact(|ledger| {
            let state = ledger.sessions.entry(session_id.into()).or_default();
            if state.revision != revision {
                return Err("queue_revision_conflict");
            }
            if !paused && (state.uncertain || state.configuration_uncertain) {
                return Err("interrupted_needs_review");
            }
            if !paused && state.queue.iter().any(|entry| entry.state != "pending") {
                return Err("queued_origin_revoked");
            }
            state.paused = paused;
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or("queue_revision_exhausted")?;
            Ok(())
        })
    }

    pub fn remove(
        &mut self,
        session_id: &str,
        entry_id: Option<&str>,
        revision: Option<u64>,
    ) -> Result<(), &'static str> {
        self.transact(|ledger| {
            let state = ledger
                .sessions
                .get_mut(session_id)
                .ok_or("unknown_queue_entry")?;
            if revision.is_some_and(|revision| revision != state.revision) {
                return Err("queue_revision_conflict");
            }
            if entry_id.is_some_and(|id| !state.queue.iter().any(|entry| entry.entry_id == id)) {
                return Err("unknown_queue_entry");
            }
            for entry in state
                .queue
                .iter()
                .filter(|entry| entry_id.is_none_or(|id| entry.entry_id == id))
            {
                if let Some(receipt) = ledger.receipts.get_mut(&entry.command_id) {
                    receipt.state = "removed".into();
                }
            }
            state
                .queue
                .retain(|entry| entry_id.is_some_and(|id| entry.entry_id != id));
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or("queue_revision_exhausted")?;
            Ok(())
        })
    }

    pub fn revoke(&mut self, principal: Option<&str>) -> Result<Vec<String>, &'static str> {
        self.transact(|ledger| {
            let mut affected = Vec::new();
            for (id, state) in &mut ledger.sessions {
                let mut changed = false;
                for entry in &mut state.queue {
                    if principal.is_none_or(|principal| entry.origin_device_id == principal) {
                        entry.state = "originRevoked".into();
                        changed = true;
                    }
                }
                if changed {
                    state.paused = true;
                    state.revision += 1;
                    affected.push(id.clone());
                }
            }
            Ok(affected)
        })
    }

    pub fn queued_principals(&self, session_id: &str) -> Vec<String> {
        self.ledger
            .sessions
            .get(session_id)
            .map(|state| {
                state
                    .queue
                    .iter()
                    .map(|entry| entry.origin_device_id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn review_uncertain_turn(
        &mut self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<(), &'static str> {
        self.transact(|ledger| {
            let state = ledger
                .sessions
                .get_mut(session_id)
                .ok_or("unknown_session")?;
            let turn = state
                .active_turn
                .as_ref()
                .filter(|turn| turn.turn_id == turn_id && turn.state == "uncertain")
                .ok_or("turn_changed")?;
            ledger
                .receipts
                .get_mut(&turn.entry.command_id)
                .ok_or("queue_storage_corrupt")?
                .state = "reviewed".into();
            state.active_turn = None;
            state.uncertain = false;
            state.paused = true;
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or("queue_revision_exhausted")?;
            Ok(())
        })
    }

    pub fn configuration_allowed(&self, session_id: &str) -> Result<(), &'static str> {
        if self.ledger.sessions.get(session_id).is_some_and(|state| {
            state.active_turn.is_some()
                || state.uncertain
                || state.configuration_uncertain
                || state.configuring
                || (!state.paused && !state.queue.is_empty())
        }) {
            return Err("session_busy");
        }
        Ok(())
    }

    pub fn begin_configuration(&mut self, session_id: &str) -> Result<(), &'static str> {
        self.configuration_allowed(session_id)?;
        self.transact(|ledger| {
            ledger
                .sessions
                .entry(session_id.into())
                .or_default()
                .configuring = true;
            Ok(())
        })
    }

    pub fn finish_configuration(
        &mut self,
        session_id: &str,
        uncertain: bool,
    ) -> Result<(), &'static str> {
        self.transact(|ledger| {
            let state = ledger.sessions.entry(session_id.into()).or_default();
            state.configuring = false;
            if uncertain {
                state.configuration_uncertain = true;
                state.paused = true;
            }
            Ok(())
        })
    }

    pub fn reconcile_configuration(&mut self, session_id: &str) -> Result<(), &'static str> {
        self.transact(|ledger| {
            if let Some(state) = ledger.sessions.get_mut(session_id) {
                state.configuration_uncertain = false;
            }
            Ok(())
        })
    }

    pub fn close(&mut self, session_id: &str) -> Result<(), &'static str> {
        if self.ledger.sessions.get(session_id).is_some_and(|state| {
            state.active_turn.is_some()
                || !state.queue.is_empty()
                || state.configuring
                || state.uncertain
                || state.configuration_uncertain
                || state.cancellation.is_some()
        }) {
            return Err("session_busy");
        }
        self.transact(|ledger| {
            ledger.sessions.remove(session_id);
            Ok(())
        })
    }

    pub fn images_for(&self, entry: &Entry) -> Result<Vec<PromptImage>, &'static str> {
        entry
            .images
            .iter()
            .map(|descriptor| {
                if let Some(image) = self.images.get(&descriptor.sha256) {
                    return Ok(image.clone());
                }
                let path = self
                    .image_path(&descriptor.sha256)
                    .ok_or("image_unavailable")?;
                let bytes = std::fs::read(path).map_err(|_| "image_unavailable")?;
                PromptImage::from_normalized(&bytes, descriptor).map_err(|_| "image_integrity")
            })
            .collect()
    }

    fn image_path(&self, sha256: &str) -> Option<PathBuf> {
        if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        self.directory
            .as_ref()
            .map(|directory| directory.join("queued-images").join(sha256))
    }

    fn store_image(&mut self, image: &PromptImage) -> Result<(), &'static str> {
        let descriptor = image.descriptor();
        if let Some(path) = self.image_path(&descriptor.sha256) {
            let parent = path.parent().ok_or("queue_storage_unavailable")?;
            std::fs::create_dir_all(parent).map_err(|_| "queue_storage_unavailable")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| "queue_storage_unavailable")?;
            }
            crate::state::write_private(&path, image.normalized_bytes())
                .map_err(|_| "queue_storage_unavailable")?;
        } else {
            self.images.insert(descriptor.sha256, image.clone());
        }
        Ok(())
    }

    fn referenced_images(&self) -> BTreeSet<String> {
        self.ledger
            .sessions
            .values()
            .flat_map(|state| {
                state
                    .queue
                    .iter()
                    .chain(state.active_turn.iter().map(|turn| &turn.entry))
            })
            .flat_map(|entry| entry.images.iter().map(|image| image.sha256.clone()))
            .collect()
    }

    fn collect_images(&mut self) {
        let keep = self.referenced_images();
        self.images.retain(|hash, _| keep.contains(hash));
        let Some(directory) = self
            .directory
            .as_ref()
            .map(|path| path.join("queued-images"))
        else {
            return;
        };
        if let Ok(entries) = std::fs::read_dir(directory) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.len() == 64
                    && name.bytes().all(|byte| byte.is_ascii_hexdigit())
                    && !keep.contains(&name)
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    fn validate_additional_bytes(
        &self,
        text: &str,
        additional: &[PromptImage],
    ) -> Result<(), &'static str> {
        let mut bytes = text.len();
        let mut images = BTreeMap::new();
        for state in self.ledger.sessions.values() {
            for entry in state
                .queue
                .iter()
                .chain(state.active_turn.iter().map(|turn| &turn.entry))
            {
                bytes = bytes
                    .checked_add(entry.text.len())
                    .ok_or("queue_byte_limit")?;
                for image in &entry.images {
                    images.insert(image.sha256.clone(), image.byte_length);
                }
            }
        }
        for image in additional {
            let descriptor = image.descriptor();
            images.insert(descriptor.sha256, descriptor.byte_length);
        }
        for length in images.values() {
            bytes = bytes.checked_add(*length).ok_or("queue_byte_limit")?;
        }
        if bytes > MAX_QUEUE_BYTES {
            return Err("queue_byte_limit");
        }
        Ok(())
    }

    fn validate_budget(&self) -> Result<(), &'static str> {
        let mut bytes = 0usize;
        let mut images = BTreeMap::new();
        for state in self.ledger.sessions.values() {
            if state.queue.len() > crate::bounds::MAX_QUEUED_PROMPTS {
                return Err("queue_full");
            }
            for entry in state
                .queue
                .iter()
                .chain(state.active_turn.iter().map(|turn| &turn.entry))
            {
                bytes = bytes
                    .checked_add(entry.text.len())
                    .ok_or("queue_byte_limit")?;
                if entry.images.len() > crate::media::MAX_IMAGES {
                    return Err("queue_byte_limit");
                }
                for image in &entry.images {
                    if image.byte_length > crate::media::MAX_NORMALIZED_IMAGE_BYTES {
                        return Err("queue_byte_limit");
                    }
                    images.insert(&image.sha256, image.byte_length);
                }
            }
        }
        for length in images.values() {
            bytes = bytes.checked_add(*length).ok_or("queue_byte_limit")?;
        }
        if bytes > MAX_QUEUE_BYTES {
            return Err("queue_byte_limit");
        }
        Ok(())
    }

    fn transact<T>(
        &mut self,
        change: impl FnOnce(&mut Ledger) -> Result<T, &'static str>,
    ) -> Result<T, &'static str> {
        let previous = self.ledger.clone();
        let result = change(&mut self.ledger).and_then(|value| {
            self.validate_budget()?;
            self.persist()?;
            Ok(value)
        });
        if result.is_err() {
            self.ledger = previous;
        }
        self.collect_images();
        result
    }

    fn persist(&self) -> Result<(), &'static str> {
        let Some(directory) = &self.directory else {
            return Ok(());
        };
        let bytes = serde_json::to_vec(&self.ledger).map_err(|_| "queue_storage_unavailable")?;
        let temporary = directory.join("session-execution.json.tmp");
        crate::state::write_private(&temporary, &bytes).map_err(|_| "queue_storage_unavailable")?;
        std::fs::rename(&temporary, directory.join("session-execution.json"))
            .map_err(|_| "queue_storage_unavailable")?;
        #[cfg(unix)]
        std::fs::File::open(directory)
            .and_then(|file| file.sync_all())
            .map_err(|_| "queue_storage_unavailable")?;
        Ok(())
    }
}

pub(crate) fn fingerprint(operation: &crate::protocol::Operation) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(operation).expect("operation serializes"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enqueue(store: &mut ExecutionStore, session: &str, principal: &str, key: &str) -> Value {
        store
            .enqueue(session, principal, key, key, &[], key)
            .unwrap()
    }

    use serde_json::Value;

    #[test]
    fn writers_share_one_fifo_and_only_one_turn_can_start() {
        let mut store = ExecutionStore::default();
        let first = enqueue(&mut store, "one", "phone", "same-key");
        let second = enqueue(&mut store, "one", "desktop", "same-key");
        let turn = store.begin_turn("one").unwrap().unwrap();
        assert_eq!(turn.entry.entry_id, first["entryId"]);
        assert!(store.begin_turn("one").unwrap().is_none());
        store
            .finish_turn("one", &turn.turn_id, false, false)
            .unwrap();
        assert_eq!(
            store.begin_turn("one").unwrap().unwrap().entry.entry_id,
            second["entryId"]
        );
    }

    #[test]
    fn retries_are_bound_to_principal_payload_and_session() {
        let mut store = ExecutionStore::default();
        let first = enqueue(&mut store, "one", "phone", "key");
        assert_eq!(
            first["entryId"],
            enqueue(&mut store, "one", "phone", "key")["entryId"]
        );
        assert_eq!(
            store
                .enqueue("two", "phone", "key", "changed", &[], "different")
                .unwrap_err(),
            "idempotency_conflict"
        );
        assert_eq!(store.snapshot("one")["queue"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn cancel_cannot_hit_a_successor_and_fences_delivery_after_turn_completion() {
        let mut store = ExecutionStore::default();
        enqueue(&mut store, "one", "phone", "a");
        enqueue(&mut store, "one", "desktop", "b");
        let turn = store.begin_turn("one").unwrap().unwrap();
        store.mark_dispatched("one", &turn.turn_id).unwrap();
        assert!(store.cancel("one", &turn.turn_id, false).unwrap());
        store
            .finish_turn("one", &turn.turn_id, false, false)
            .unwrap();
        assert!(store.begin_turn("one").unwrap().is_none());
        store.finish_cancel("one", &turn.turn_id, false).unwrap();
        let successor = store.begin_turn("one").unwrap().unwrap();
        assert_ne!(successor.turn_id, turn.turn_id);
        assert_eq!(
            store.cancel("one", &turn.turn_id, true).unwrap_err(),
            "turn_changed"
        );
    }

    #[test]
    fn restart_preserves_pending_and_never_replays_dispatched_work() {
        let root = tempfile::tempdir().unwrap();
        let mut store = ExecutionStore::open(root.path()).unwrap();
        enqueue(&mut store, "one", "phone", "a");
        enqueue(&mut store, "one", "desktop", "b");
        store.begin_turn("one").unwrap().unwrap();
        drop(store);
        let mut restored = ExecutionStore::open(root.path()).unwrap();
        assert!(restored.begin_turn("one").unwrap().is_none());
        assert_eq!(restored.snapshot("one")["queue"][0]["text"], "b");
        assert_eq!(restored.snapshot("one")["activeTurn"]["state"], "uncertain");
        let revision = restored.snapshot("one")["revision"].as_u64().unwrap();
        assert_eq!(
            restored.set_paused("one", false, revision).unwrap_err(),
            "interrupted_needs_review"
        );
    }

    #[test]
    fn clear_queue_does_not_delete_concurrent_messages() {
        let mut store = ExecutionStore::default();
        enqueue(&mut store, "one", "phone", "a");
        let revision = store.snapshot("one")["revision"].as_u64().unwrap();
        enqueue(&mut store, "one", "desktop", "b");
        assert_eq!(
            store.remove("one", None, Some(revision)).unwrap_err(),
            "queue_revision_conflict"
        );
        assert_eq!(store.snapshot("one")["queue"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn revocation_holds_pending_work_without_cancelling_current_turn() {
        let mut store = ExecutionStore::default();
        enqueue(&mut store, "one", "phone", "a");
        let turn = store.begin_turn("one").unwrap().unwrap();
        enqueue(&mut store, "one", "phone", "b");
        store.revoke(Some("phone")).unwrap();
        assert_eq!(store.snapshot("one")["activeTurn"]["turnId"], turn.turn_id);
        assert_eq!(store.snapshot("one")["queue"][0]["state"], "originRevoked");
        store
            .finish_turn("one", &turn.turn_id, false, false)
            .unwrap();
        assert!(store.begin_turn("one").unwrap().is_none());
    }

    #[test]
    fn configuration_reconciliation_does_not_clear_uncertain_turns() {
        let mut store = ExecutionStore::default();
        store.begin_configuration("one").unwrap();
        store.finish_configuration("one", true).unwrap();
        enqueue(&mut store, "one", "phone", "a");
        assert!(store.begin_turn("one").unwrap().is_none());
        store.reconcile_configuration("one").unwrap();
        let revision = store.snapshot("one")["revision"].as_u64().unwrap();
        store.set_paused("one", false, revision).unwrap();
        let turn = store.begin_turn("one").unwrap().unwrap();
        store.finish_turn("one", &turn.turn_id, true, true).unwrap();
        store.reconcile_configuration("one").unwrap();
        assert_eq!(store.snapshot("one")["uncertain"], true);
    }

    #[test]
    fn failed_durability_never_accepts_a_message() {
        let root = tempfile::tempdir().unwrap();
        let mut store = ExecutionStore::open(root.path()).unwrap();
        std::fs::create_dir(root.path().join("session-execution.json.tmp")).unwrap();
        assert_eq!(
            store
                .enqueue("one", "phone", "a", "a", &[], "a")
                .unwrap_err(),
            "queue_storage_unavailable"
        );
        assert!(store.status("phone", "a").is_none());
        assert!(store.begin_turn("one").is_err());
        let restored = ExecutionStore::open(root.path());
        assert!(restored.is_err());
    }

    #[test]
    fn normalized_image_budget_is_global_and_deduplicated() {
        let mut store = ExecutionStore::default();
        let images = (0..8)
            .map(|index| ImageDescriptor {
                byte_length: 1_500_000,
                mime_type: "image/png".into(),
                width: 100,
                height: 100,
                sha256: format!("{index:064x}"),
            })
            .collect::<Vec<_>>();
        let mut execution = Execution::default();
        for n in 0..16 {
            execution.queue.push(Entry {
                entry_id: n.to_string(),
                command_id: n.to_string(),
                origin_device_id: "phone".into(),
                text: "x".into(),
                images: images.clone(),
                state: "pending".into(),
            });
        }
        store.ledger.sessions.insert("one".into(), execution);
        assert!(store.validate_budget().is_ok());
        for (n, entry) in store
            .ledger
            .sessions
            .get_mut("one")
            .unwrap()
            .queue
            .iter_mut()
            .enumerate()
        {
            for (m, image) in entry.images.iter_mut().enumerate() {
                image.sha256 = format!("{:064x}", n * 8 + m);
            }
        }
        assert_eq!(store.validate_budget().unwrap_err(), "queue_byte_limit");
    }
    #[test]
    fn repeating_stop_after_cancel_was_written_does_not_leave_a_new_fence() {
        let mut store = ExecutionStore::default();
        enqueue(&mut store, "one", "phone", "a");
        enqueue(&mut store, "one", "phone", "b");
        let turn = store.begin_turn("one").unwrap().unwrap();
        store.mark_dispatched("one", &turn.turn_id).unwrap();
        assert!(store.cancel("one", &turn.turn_id, false).unwrap());
        store.finish_cancel("one", &turn.turn_id, false).unwrap();
        assert!(!store.cancel("one", &turn.turn_id, false).unwrap());
        store
            .finish_turn("one", &turn.turn_id, false, false)
            .unwrap();
        assert!(store.begin_turn("one").unwrap().is_some());
    }

    #[test]
    fn cancellation_before_prompt_write_is_deferred_until_dispatched() {
        let mut store = ExecutionStore::default();
        enqueue(&mut store, "one", "phone", "first");
        let turn = store.begin_turn("one").unwrap().unwrap();
        assert_eq!(turn.state, "starting");
        assert!(!store.cancel("one", &turn.turn_id, true).unwrap());
        assert!(!store.cancellation_ready("one", &turn.turn_id));
        assert!(store.mark_dispatched("one", &turn.turn_id).unwrap());
        assert!(store.cancellation_ready("one", &turn.turn_id));
        store.finish_cancel("one", &turn.turn_id, false).unwrap();
        store
            .finish_turn("one", &turn.turn_id, false, false)
            .unwrap();
        assert!(store.snapshot("one")["activeTurn"].is_null());
        assert_eq!(store.snapshot("one")["paused"], true);
    }
}
