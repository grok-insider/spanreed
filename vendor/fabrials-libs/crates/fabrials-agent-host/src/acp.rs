//! Supervised ACP client over the Grok Build CLI's stdio transport.
//!
//! Implements ADR light 0003. Production spawns
//! `grok agent --no-leader stdio` and speaks newline-delimited JSON-RPC over
//! the child's pipes. `grok agent serve` is never used as a browser-facing
//! listener and never as a production transport.
//!
//! The browser never sees any of this. The host translates ACP into the closed
//! `light.local.v1` surface.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::agent_process::{AgentProcess, scope_error};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{Mutex, mpsc, oneshot, watch};
use tokio::time::timeout;

/// ACP wire version this host implements.
pub const ACP_PROTOCOL_VERSION: u32 = 1;

/// Client identifier Light presents to the agent.
///
/// The agent resolves any unrecognised identifier to its `Generic` client
/// type, which is the conservative option presentation. See ADR light 0007.
pub const CLIENT_IDENTIFIER: &str = "grok-light";

/// Environment variable the CLI reads to determine the client identifier.
pub const CLIENT_NAME_ENV: &str = "GROK_CLIENT_NAME";

/// Maximum accepted length of one JSON-RPC line from the agent, in bytes.
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// Default timeout for a single ACP request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_mins(1);

/// Flags Light must never pass to the agent.
///
/// `--always-approve` disables prompting entirely and `--plugin-dir` is
/// documented by the CLI as an always-trusted scope whose hooks and MCP servers
/// activate without a prompt. Neither is reachable from the browser, and the
/// host does not use them either.
pub const FORBIDDEN_FLAGS: [&str; 2] = ["--always-approve", "--plugin-dir"];

/// Errors produced by the ACP adapter.
#[derive(Debug, thiserror::Error)]
pub enum AcpError {
    /// The agent executable could not be spawned.
    #[error("failed to spawn the agent process: {0}")]
    Spawn(#[source] std::io::Error),
    #[allow(missing_docs)]
    #[error(
        "shared agent failed while starting in an independent systemd user scope: {0}; a working user manager is required when the host runs as a service"
    )]
    SharedScope(#[source] Box<AcpError>),
    /// A pipe to the child was unavailable.
    #[error("agent stdio pipe was unavailable")]
    Pipe,
    /// Writing to or reading from the child failed.
    #[error("agent transport failure: {0}")]
    Transport(#[source] std::io::Error),
    /// A line exceeded [`MAX_LINE_BYTES`].
    #[error("agent sent an oversized message")]
    OversizedMessage,
    /// The agent closed its output before answering.
    #[error("agent closed the transport")]
    Closed,
    /// A response could not be parsed as JSON-RPC.
    #[error("agent sent malformed json-rpc")]
    Malformed,
    /// The agent returned a JSON-RPC error.
    ///
    /// The code is kept because it is the only field that says *why* in terms
    /// the host may act on. The message is agent-supplied text: it is carried
    /// for diagnosis and never used to decide control flow.
    #[error("agent returned error {code}")]
    Agent {
        /// JSON-RPC error code.
        code: i64,
        /// Agent-supplied description. Untrusted.
        message: String,
    },
    /// The request exceeded its deadline.
    #[error("agent request timed out")]
    Timeout,
    #[error("agent rejected the model id")]
    #[allow(missing_docs)]
    InvalidModel,
    /// A caller attempted to pass a forbidden flag.
    #[error("refused to pass a forbidden flag to the agent")]
    ForbiddenFlag,
}

/// JSON-RPC's code for a method the peer does not implement.
///
/// The qualified CLI is the user's own install, so it may be older or newer
/// than the one Light was built against. This code is how it says "I do not
/// have that", and is what separates a capability this build lacks from a
/// request that genuinely failed.
pub const METHOD_NOT_FOUND: i64 = -32601;

/// Build an [`AcpError::Agent`] from a JSON-RPC `error` member.
///
/// A missing or non-integer code becomes `0`, which matches no known code and
/// so is never mistaken for a meaningful one.
fn agent_error(error: &Value) -> AcpError {
    if error.get("code").and_then(Value::as_i64) == Some(-32602)
        && error.get("data").and_then(Value::as_str) == Some("unknown model id")
    {
        return AcpError::InvalidModel;
    }
    AcpError::Agent {
        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
        message: error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .chars()
            .take(1024)
            .collect(),
    }
}

impl AcpError {
    /// Whether the qualified CLI simply does not implement the method.
    ///
    /// This is a capability answer, not a failure: the caller is expected to
    /// degrade rather than report the feature as broken.
    #[must_use]
    pub fn is_unsupported_method(&self) -> bool {
        matches!(
            self,
            Self::Agent {
                code: METHOD_NOT_FOUND,
                ..
            }
        )
    }
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentMode {
    #[default]
    Isolated,
    Shared,
}

/// Where the agent executable lives and how it is invoked.
#[derive(Debug, Clone)]
pub struct AgentCommand {
    #[allow(missing_docs)]
    pub mode: AgentMode,
    /// Path or program name of the qualified executable.
    pub program: String,
    /// Working directory for the child.
    pub working_directory: Option<std::path::PathBuf>,
    /// Extra environment variables for the child, on top of the inherited ones.
    pub env: Vec<(String, String)>,
}

impl AgentCommand {
    /// Build a command for the qualified executable.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            mode: AgentMode::Isolated,
            working_directory: None,
            env: Vec::new(),
        }
    }

    #[allow(missing_docs)]
    #[must_use]
    pub fn with_mode(mut self, mode: AgentMode) -> Self {
        self.mode = mode;
        self
    }

    /// Add environment variables for the spawned agent.
    #[must_use]
    pub fn with_env(mut self, env: impl IntoIterator<Item = (String, String)>) -> Self {
        self.env.extend(env);
        self
    }

    /// Set the working directory for the spawned agent.
    #[must_use]
    pub fn with_working_directory(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.working_directory = Some(path.into());
        self
    }

    /// The exact production argument vector.
    ///
    /// Options precede the subcommand, matching `grok agent [OPTIONS] [COMMAND]`.
    #[must_use]
    pub fn argv(&self) -> Vec<&'static str> {
        match self.mode {
            AgentMode::Isolated => vec!["agent", "--no-leader", "stdio"],
            AgentMode::Shared => vec!["agent", "--leader", "stdio"],
        }
    }
}

/// The agent's answer to `initialize`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// Negotiated ACP version.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: u32,
    /// Capabilities the agent advertises.
    #[serde(rename = "agentCapabilities", default)]
    pub agent_capabilities: Value,
    /// Authentication methods the agent offers.
    #[serde(rename = "authMethods", default)]
    pub auth_methods: Vec<Value>,
    /// Vendor metadata.
    #[serde(rename = "_meta", default)]
    pub meta: Value,
}

impl InitializeResult {
    #[allow(missing_docs)]
    #[must_use]
    pub fn supports_qualified_images(&self) -> bool {
        self.supports_images()
            || (self.meta.get("grokShell").and_then(Value::as_bool) == Some(true)
                && matches!(
                    self.meta.get("agentVersion").and_then(Value::as_str),
                    Some("1.0.50" | "1.0.52")
                ))
    }

    #[allow(missing_docs)]
    #[must_use]
    pub fn supports_images(&self) -> bool {
        self.agent_capabilities
            .pointer("/promptCapabilities/image")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    #[allow(missing_docs)]
    #[must_use]
    pub fn supports_audio(&self) -> bool {
        self.agent_capabilities
            .pointer("/promptCapabilities/audio")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    #[allow(missing_docs)]
    #[must_use]
    pub fn supports_embedded_context(&self) -> bool {
        self.agent_capabilities
            .pointer("/promptCapabilities/embeddedContext")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Whether the agent supports loading previous sessions.
    #[must_use]
    pub fn supports_load_session(&self) -> bool {
        self.agent_capabilities
            .get("loadSession")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// The agent version string, when the agent reports one.
    #[must_use]
    pub fn agent_version(&self) -> Option<&str> {
        self.meta.get("agentVersion").and_then(Value::as_str)
    }
}

/// Method the agent calls on the client to ask for a permission decision.
pub const REQUEST_PERMISSION: &str = "session/request_permission";

/// Notification the agent emits for streaming session state.
pub const SESSION_UPDATE: &str = "session/update";

/// Something the agent sent that the host must act on.
#[derive(Debug)]
pub enum AgentEvent {
    #[allow(missing_docs)]
    Barrier(oneshot::Sender<()>),
    #[allow(missing_docs)]
    InteractionRequest {
        method: String,
        request_id: Value,
        params: Value,
    },
    /// A streaming session update notification (`session/update`).
    Update(Value),
    /// An extension notification (`x.ai/*` or `_x.ai/*`).
    ///
    /// The host classifies known methods (tasks, workflows, …) and drops the
    /// rest. Carrying method + params keeps NotificationHub open for new kinds
    /// without growing the enum per method.
    ExtNotification {
        /// JSON-RPC method name as the agent sent it.
        method: String,
        /// Raw params object.
        params: Value,
    },
    /// The agent asked for a permission decision and awaits an answer.
    PermissionRequest {
        /// JSON-RPC id the answer must carry.
        request_id: Value,
        /// Raw request parameters, including the offered options.
        params: Value,
    },
    /// The agent process ended.
    Exited,
}

/// A running agent child with a message loop.
///
/// The loop separates three inbound shapes that ACP mixes on one pipe:
/// responses to host requests, notifications, and requests the agent makes of
/// the host. Only the last two reach the caller, as [`AgentEvent`].
#[derive(Debug)]
pub struct AgentHandle {
    stdin: Mutex<ChildStdin>,
    events: mpsc::Sender<AgentEvent>,
    transport_closed: watch::Sender<bool>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    echoes: Arc<Mutex<HashMap<u64, PromptEcho>>>,
    next_id: AtomicU64,
    child: Mutex<AgentProcess>,
    scoped: bool,
}

#[derive(Debug)]
struct PromptEcho {
    session_id: String,
    prompt_id: Option<String>,
    remaining: String,
    images: Vec<[u8; 32]>,
}

fn image_echo_hash(block: &Value) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    if block.get("type").and_then(Value::as_str) != Some("image") {
        return None;
    }
    Some(Sha256::digest(block.get("data")?.as_str()?.as_bytes()).into())
}

impl AgentHandle {
    /// Spawn the agent and start its message loop.
    ///
    /// Returns the handle and the stream of agent-initiated events.
    ///
    /// # Errors
    ///
    /// Returns [`AcpError::Spawn`] when the executable cannot start and
    /// [`AcpError::Pipe`] when a standard stream is unavailable.
    pub fn spawn(
        command: &AgentCommand,
    ) -> Result<(Arc<Self>, mpsc::Receiver<AgentEvent>), AcpError> {
        let argv = command.argv();
        if argv.iter().any(|arg| FORBIDDEN_FLAGS.contains(arg)) {
            return Err(AcpError::ForbiddenFlag);
        }

        let mut process = AgentProcess::spawn(command)?;
        let scoped = process.scoped;
        let stdin = process.child.stdin.take().ok_or(AcpError::Pipe)?;
        let stdout = process.child.stdout.take().ok_or(AcpError::Pipe)?;

        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (events_tx, events_rx) = mpsc::channel(256);
        let (transport_closed, closed_rx) = watch::channel(false);
        let echoes = Arc::new(Mutex::new(HashMap::new()));

        let handle = Arc::new(Self {
            stdin: Mutex::new(stdin),
            events: events_tx.clone(),
            transport_closed,
            pending: Arc::clone(&pending),
            echoes: Arc::clone(&echoes),
            next_id: AtomicU64::new(0),
            child: Mutex::new(process),
            scoped,
        });

        tokio::spawn(read_loop(stdout, pending, echoes, events_tx, closed_rx));
        Ok((handle, events_rx))
    }

    #[allow(missing_docs)]
    pub async fn flush_events(&self) -> Result<(), AcpError> {
        timeout(REQUEST_TIMEOUT, async {
            let (ack, received) = oneshot::channel();
            self.events
                .send(AgentEvent::Barrier(ack))
                .await
                .map_err(|_| AcpError::Closed)?;
            received.await.map_err(|_| AcpError::Closed)
        })
        .await
        .map_err(|_| AcpError::Timeout)?
    }

    /// Send a JSON-RPC request and await its response.
    ///
    /// # Errors
    ///
    /// Propagates transport, timeout, and agent errors.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, AcpError> {
        self.request_with_timeout(method, params, Some(REQUEST_TIMEOUT))
            .await
    }

    async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        deadline: Option<Duration>,
    ) -> Result<Value, AcpError> {
        self.request_with_timeout_started(method, params, deadline, None)
            .await
    }

    async fn request_with_timeout_started(
        &self,
        method: &str,
        params: Value,
        deadline: Option<Duration>,
        started: Option<oneshot::Sender<()>>,
    ) -> Result<Value, AcpError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if method == "session/prompt" {
            if let (Some(session), Some(text)) = (
                params.get("sessionId").and_then(Value::as_str),
                params.pointer("/prompt/0/text").and_then(Value::as_str),
            ) {
                self.echoes.lock().await.insert(
                    id,
                    PromptEcho {
                        session_id: session.into(),
                        prompt_id: params
                            .pointer("/_meta/promptId")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        remaining: text.into(),
                        images: params
                            .get("prompt")
                            .and_then(Value::as_array)
                            .map(|blocks| blocks.iter().filter_map(image_echo_hash).collect())
                            .unwrap_or_default(),
                    },
                );
            }
        }

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if let Err(error) = self.write_line(&payload).await {
            self.pending.lock().await.remove(&id);
            self.echoes.lock().await.remove(&id);
            return Err(error);
        }
        if let Some(started) = started {
            let _ = started.send(());
        }

        let response = match deadline {
            Some(deadline) => timeout(deadline, rx).await,
            None => Ok(rx.await),
        };
        self.echoes.lock().await.remove(&id);
        let message = match response {
            Ok(Ok(message)) => message,
            Ok(Err(_)) => {
                self.pending.lock().await.remove(&id);
                return Err(AcpError::Closed);
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                return Err(AcpError::Timeout);
            }
        };

        if let Some(error) = message.get("error") {
            return Err(agent_error(error));
        }
        message.get("result").cloned().ok_or(AcpError::Malformed)
    }

    /// Answer a permission request the agent is waiting on.
    ///
    /// # Errors
    ///
    /// Returns a transport error when the answer cannot be written.
    pub async fn answer_permission(
        &self,
        request_id: &Value,
        option_id: &str,
    ) -> Result<(), AcpError> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "outcome": { "outcome": "selected", "optionId": option_id }
            }
        });
        self.write_line(&payload).await
    }

    #[allow(missing_docs)]
    pub async fn answer_interaction(
        &self,
        request_id: &Value,
        result: Value,
    ) -> Result<(), AcpError> {
        self.write_line(&serde_json::json!({"jsonrpc":"2.0", "id":request_id, "result":result}))
            .await
    }

    /// Perform the ACP `initialize` handshake.
    ///
    /// # Errors
    ///
    /// Propagates any transport, timeout, or agent error.
    pub async fn initialize(&self) -> Result<InitializeResult, AcpError> {
        let params = serde_json::json!({
            "protocolVersion": ACP_PROTOCOL_VERSION,
            "clientCapabilities": {
                "fs": { "readTextFile": false, "writeTextFile": false },
                "_meta": { "x.ai/userMessageEcho": true }
            },
            "_meta": { "clientIdentifier": CLIENT_IDENTIFIER }
        });
        let result = self
            .request("initialize", params)
            .await
            .map_err(|error| scope_error(self.scoped, error))?;
        serde_json::from_value(result).map_err(|_| AcpError::Malformed)
    }

    /// Create an agent session bound to a working directory.
    ///
    /// The directory is chosen by the host from an enrolled workspace; the
    /// browser never supplies a path.
    ///
    /// # Errors
    ///
    /// Propagates transport and agent errors, and returns
    /// [`AcpError::Malformed`] when the agent omits a session identifier.
    pub async fn new_session(&self, cwd: &std::path::Path) -> Result<(String, Value), AcpError> {
        let params = serde_json::json!({
            "cwd": cwd.to_string_lossy(),
            "mcpServers": [],
            "_meta": { "clientUserMessageEcho": true },
        });
        let result = self.request("session/new", params).await?;
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(AcpError::Malformed)?;
        Ok((session_id, result))
    }

    /// Resume a session the agent already persisted.
    ///
    /// The working directory comes from the enrolled workspace, never from the
    /// browser. Sessions live in the agent's own store, so Light replays what
    /// the CLI kept rather than keeping a second copy of the transcript.
    ///
    /// # Errors
    ///
    /// Propagates transport and agent errors, including the agent's refusal
    /// when the identifier names no session it holds.
    pub async fn load_session(
        &self,
        session_id: &str,
        cwd: &std::path::Path,
    ) -> Result<Value, AcpError> {
        let params = serde_json::json!({
            "sessionId": session_id,
            "cwd": cwd.to_string_lossy(),
            "mcpServers": [],
            "_meta": { "clientUserMessageEcho": true },
        });
        self.request("session/load", params).await
    }

    /// Ask the agent to enumerate the sessions it holds.
    ///
    /// Not every qualified CLI implements this. A build that does not answers
    /// with [`METHOD_NOT_FOUND`], which the caller is expected to read through
    /// [`AcpError::is_unsupported_method`] and degrade on, rather than report
    /// as a fault.
    ///
    /// # Errors
    ///
    /// Propagates transport and agent errors.
    pub async fn list_sessions(&self) -> Result<Value, AcpError> {
        self.request("session/list", serde_json::json!({})).await
    }

    /// Send a prompt to an existing session (ordinary agent chat only).
    ///
    /// Bash / bang-mode shell is **not** sent here: Grok Build treats it as a
    /// client-local drain. Light runs those turns via [`crate::bash`].
    ///
    /// # Errors
    ///
    /// Propagates transport, timeout, and agent errors.
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<Value, AcpError> {
        self.prompt_with_images(session_id, text, &[]).await
    }

    #[allow(missing_docs)]
    pub async fn prompt_with_images(
        &self,
        session_id: &str,
        text: &str,
        images: &[crate::media::PromptImage],
    ) -> Result<Value, AcpError> {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random)
            .map_err(|error| AcpError::Transport(std::io::Error::other(error)))?;
        random[6] = (random[6] & 0x0f) | 0x40;
        random[8] = (random[8] & 0x3f) | 0x80;
        let hex: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let prompt_id = format!(
            "{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        );
        self.prompt_target(session_id, &prompt_id, text, images)
            .await
    }

    #[allow(missing_docs)]
    pub async fn prompt_target(
        &self,
        session_id: &str,
        prompt_id: &str,
        text: &str,
        images: &[crate::media::PromptImage],
    ) -> Result<Value, AcpError> {
        self.prompt_target_with_started(session_id, prompt_id, text, images, None)
            .await
    }

    #[allow(missing_docs)]
    pub async fn prompt_target_with_started(
        &self,
        session_id: &str,
        prompt_id: &str,
        text: &str,
        images: &[crate::media::PromptImage],
        started: Option<oneshot::Sender<()>>,
    ) -> Result<Value, AcpError> {
        if prompt_id.is_empty()
            || prompt_id.len() > 128
            || !prompt_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(AcpError::Malformed);
        }
        crate::media::validate_prompt_images(images).map_err(|_| AcpError::OversizedMessage)?;
        let mut content = vec![serde_json::json!({"type": "text", "text": text})];
        content.extend(images.iter().map(crate::media::PromptImage::acp_block));
        let params = serde_json::json!({
            "sessionId": session_id,
            "prompt": content,
            "_meta": { "promptId": prompt_id },
        });
        self.request_with_timeout_started("session/prompt", params, None, started)
            .await
    }

    /// Switch model (and optional reasoning effort) via ACP `session/set_model`.
    ///
    /// # Errors
    ///
    /// Propagates transport and agent errors; callers refuse non-Grok ids first.
    pub async fn set_session_model(
        &self,
        session_id: &str,
        model_id: &str,
        reasoning_effort: Option<&str>,
    ) -> Result<Value, AcpError> {
        let mut params = serde_json::json!({
            "sessionId": session_id,
            "modelId": model_id,
        });
        if let Some(effort) = reasoning_effort {
            params["_meta"] = serde_json::json!({ "reasoningEffort": effort });
        }
        self.request("session/set_model", params).await
    }

    #[allow(missing_docs)]
    pub async fn set_reasoning_effort(
        &self,
        session_id: &str,
        effort: &str,
    ) -> Result<Value, AcpError> {
        self.request(
            "session/set_config_option",
            serde_json::json!({
                "sessionId": session_id,
                "configId": "reasoning_effort",
                "value": effort,
            }),
        )
        .await
    }

    #[allow(missing_docs)]
    pub async fn set_model_config(
        &self,
        session_id: &str,
        model_id: &str,
    ) -> Result<Value, AcpError> {
        self.request(
            "session/set_config_option",
            serde_json::json!({
                "sessionId": session_id,
                "configId": "model",
                "value": model_id,
            }),
        )
        .await
    }

    /// Ask the agent to cancel the in-flight turn.
    ///
    /// # Errors
    ///
    /// Returns a transport error when the notification cannot be written.
    pub async fn cancel(&self, session_id: &str) -> Result<(), AcpError> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": { "sessionId": session_id },
        });
        self.write_line(&payload).await
    }

    #[allow(missing_docs)]
    pub async fn cancel_target(&self, session_id: &str, prompt_id: &str) -> Result<(), AcpError> {
        if prompt_id.is_empty()
            || prompt_id.len() > 128
            || !prompt_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(AcpError::Malformed);
        }
        self.write_line(&serde_json::json!({
            "jsonrpc": "2.0", "method": "session/cancel",
            "params": {"sessionId": session_id, "_meta": {"promptId": prompt_id, "rewindIfNoOutput": true, "cancelTrigger": "dashboard_stop", "cancelSubagents": false}}
        })).await
    }

    async fn write_line(&self, payload: &Value) -> Result<(), AcpError> {
        let mut line = serde_json::to_string(payload).map_err(|_| AcpError::Malformed)?;
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        if *self.transport_closed.borrow() {
            return Err(AcpError::Closed);
        }
        let result = async {
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await
        }
        .await;
        if let Err(error) = result {
            self.transport_closed.send_replace(true);
            drop(stdin);
            let _ = self.shutdown().await;
            return Err(AcpError::Transport(error));
        }
        Ok(())
    }

    /// Terminate the agent and reap it.
    ///
    /// # Errors
    ///
    /// Returns [`AcpError::Transport`] when the child cannot be signalled.
    pub async fn shutdown(&self) -> Result<(), AcpError> {
        self.transport_closed.send_replace(true);
        self.child.lock().await.shutdown().await
    }
}

/// Route every inbound line to a pending response, or to the event stream.
async fn read_loop(
    stdout: ChildStdout,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    echoes: Arc<Mutex<HashMap<u64, PromptEcho>>>,
    events: mpsc::Sender<AgentEvent>,
    mut transport_closed: watch::Receiver<bool>,
) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut running_prompts = HashMap::<String, String>::new();
    loop {
        line.clear();
        let read = tokio::select! {
            biased;
            _ = transport_closed.wait_for(|closed| *closed) => break,
            read = reader.read_line(&mut line) => read,
        };
        match read {
            Ok(0) | Err(_) => break,
            Ok(read) if read > MAX_LINE_BYTES => break,
            Ok(_) => {}
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
            // The agent may emit non-JSON diagnostics; skip rather than fail.
            continue;
        };

        let is_response = message.get("result").is_some() || message.get("error").is_some();
        if is_response {
            if let Some(id) = message.get("id").and_then(Value::as_u64)
                && let Some(sender) = pending.lock().await.remove(&id)
            {
                let _ = sender.send(message);
            }
            continue;
        }

        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let (method, params) =
            crate::interactions::normalize(method, message.get("params").unwrap_or(&Value::Null));
        if method == "x.ai/queue/changed"
            && let Some(session_id) = params.get("sessionId").and_then(Value::as_str)
        {
            if let Some(prompt_id) = params.get("runningPromptId").and_then(Value::as_str) {
                if session_id.len() <= 512
                    && prompt_id.len() <= 512
                    && (running_prompts.contains_key(session_id) || running_prompts.len() < 256)
                {
                    running_prompts.insert(session_id.into(), prompt_id.into());
                }
            } else {
                running_prompts.remove(session_id);
            }
        }
        if method == SESSION_UPDATE
            && params
                .pointer("/update/sessionUpdate")
                .and_then(Value::as_str)
                == Some("user_message_chunk")
        {
            if params
                .pointer("/update/_meta/hideFromScrollback")
                .and_then(Value::as_bool)
                == Some(true)
            {
                continue;
            }
            if params.pointer("/_meta/isReplay").and_then(Value::as_bool) != Some(true)
                && let (Some(session_id), Some(hash)) = (
                    params.get("sessionId").and_then(Value::as_str),
                    params.pointer("/update/content").and_then(image_echo_hash),
                )
            {
                let mut echoes = echoes.lock().await;
                let prompt_id = params
                    .pointer("/_meta/promptId")
                    .and_then(Value::as_str)
                    .or_else(|| running_prompts.get(session_id).map(String::as_str));
                if let Some(echo) = echoes.values_mut().find(|echo| {
                    echo.session_id == session_id
                        && prompt_id.is_none_or(|id| echo.prompt_id.as_deref() == Some(id))
                        && echo.images.contains(&hash)
                }) {
                    let index = echo
                        .images
                        .iter()
                        .position(|candidate| candidate == &hash)
                        .expect("matched image");
                    echo.images.remove(index);
                    continue;
                }
            }
            if params.pointer("/_meta/isReplay").and_then(Value::as_bool) != Some(true)
                && let (Some(session_id), Some(text)) = (
                    params.get("sessionId").and_then(Value::as_str),
                    params
                        .pointer("/update/content/text")
                        .and_then(Value::as_str),
                )
            {
                let mut echoes = echoes.lock().await;
                let prompt_id = params.pointer("/_meta/promptId").and_then(Value::as_str);
                if let Some(echo) = echoes.values_mut().find(|echo| {
                    echo.session_id == session_id
                        && !text.is_empty()
                        && echo.remaining.starts_with(text)
                        && prompt_id
                            .or_else(|| running_prompts.get(session_id).map(String::as_str))
                            .is_none_or(|id| echo.prompt_id.as_deref() == Some(id))
                }) {
                    echo.remaining.drain(..text.len());
                    continue;
                }
            }
        }
        let event = if matches!(
            method,
            "x.ai/ask_user_question" | "x.ai/exit_plan_mode" | "x.ai/mcp/elicit"
        ) && message.get("id").is_some()
        {
            Some(AgentEvent::InteractionRequest {
                method: method.into(),
                request_id: message["id"].clone(),
                params: params.clone(),
            })
        } else if method == REQUEST_PERMISSION {
            message
                .get("id")
                .map(|request_id| AgentEvent::PermissionRequest {
                    request_id: request_id.clone(),
                    params: params.clone(),
                })
        } else if method == SESSION_UPDATE {
            Some(AgentEvent::Update(
                message.get("params").cloned().unwrap_or(Value::Null),
            ))
        } else if is_ext_notification_method(method) {
            // NotificationHub: every x.ai extension rides one variant so new
            // runtime surfaces (tasks, workflows, …) do not fork the reader.
            Some(AgentEvent::ExtNotification {
                method: method.to_owned(),
                params: params.clone(),
            })
        } else {
            None
        };
        if let Some(event) = event
            && events.send(event).await.is_err()
        {
            break;
        }
    }

    // The agent is gone. Fail every in-flight request now rather than letting
    // callers wait out the full request timeout: a dead agent is a known
    // outcome, and the host must classify it while the turn is still fresh.
    pending.lock().await.clear();
    let _ = events.send(AgentEvent::Exited).await;
}

/// Whether a JSON-RPC method is an agent extension notification we may decode.
///
/// Accepts both `x.ai/…` and `_x.ai/…` (stdio transport sometimes underscores
/// extension methods). Unknown methods under that prefix still become
/// [`AgentEvent::ExtNotification`] and are dropped later by the runtime
/// decoder — better than never seeing them.
fn is_ext_notification_method(method: &str) -> bool {
    let normalized = method.strip_prefix('_').unwrap_or(method);
    normalized.starts_with("x.ai/")
}

/// A supervised agent child speaking ACP over stdio.
#[derive(Debug)]
pub struct AgentSession {
    child: AgentProcess,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    scoped: bool,
}

impl AgentSession {
    /// Spawn the agent and take ownership of its pipes.
    ///
    /// The child runs in its own process group on Unix so that group can be
    /// terminated, and is killed when this value is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`AcpError::Spawn`] when the executable cannot start and
    /// [`AcpError::Pipe`] when a standard stream is unavailable.
    pub fn spawn(command: &AgentCommand) -> Result<Self, AcpError> {
        let argv = command.argv();
        if argv.iter().any(|arg| FORBIDDEN_FLAGS.contains(arg)) {
            return Err(AcpError::ForbiddenFlag);
        }

        let mut child = AgentProcess::spawn(command)?;
        let scoped = child.scoped;
        let stdin = child.child.stdin.take().ok_or(AcpError::Pipe)?;
        let stdout = child.child.stdout.take().ok_or(AcpError::Pipe)?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            scoped,
        })
    }

    /// Perform the ACP `initialize` handshake.
    ///
    /// # Errors
    ///
    /// Propagates any transport, timeout, or agent error.
    pub async fn initialize(&mut self) -> Result<InitializeResult, AcpError> {
        let params = serde_json::json!({
            "protocolVersion": ACP_PROTOCOL_VERSION,
            "clientCapabilities": {
                "fs": { "readTextFile": false, "writeTextFile": false }
            },
            "_meta": { "clientIdentifier": CLIENT_IDENTIFIER }
        });
        let result = self
            .request("initialize", params)
            .await
            .map_err(|error| scope_error(self.scoped, error))?;
        serde_json::from_value(result).map_err(|_| AcpError::Malformed)
    }

    /// Send a JSON-RPC request and await its matching response.
    ///
    /// Notifications and server-initiated requests received while waiting are
    /// skipped; a full implementation routes them to the session event loop.
    ///
    /// # Errors
    ///
    /// Propagates transport, timeout, parse, and agent errors.
    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value, AcpError> {
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut line = serde_json::to_string(&payload).map_err(|_| AcpError::Malformed)?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(AcpError::Transport)?;
        self.stdin.flush().await.map_err(AcpError::Transport)?;

        timeout(REQUEST_TIMEOUT, self.await_response(id))
            .await
            .map_err(|_| AcpError::Timeout)?
    }

    async fn await_response(&mut self, id: u64) -> Result<Value, AcpError> {
        loop {
            let message = self.read_message().await?;
            let matches_id = message
                .get("id")
                .and_then(Value::as_u64)
                .is_some_and(|value| value == id);
            if !matches_id {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(agent_error(error));
            }
            return message.get("result").cloned().ok_or(AcpError::Malformed);
        }
    }

    async fn read_message(&mut self) -> Result<Value, AcpError> {
        let mut line = String::new();
        loop {
            line.clear();
            let read = self
                .stdout
                .read_line(&mut line)
                .await
                .map_err(AcpError::Transport)?;
            if read == 0 {
                return Err(AcpError::Closed);
            }
            if read > MAX_LINE_BYTES {
                return Err(AcpError::OversizedMessage);
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // The agent may emit non-JSON diagnostics; skip rather than fail.
            if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
                return Ok(value);
            }
        }
    }

    /// Terminate the agent and reap it.
    ///
    /// # Errors
    ///
    /// Returns [`AcpError::Transport`] when the child cannot be signalled.
    pub async fn shutdown(mut self) -> Result<(), AcpError> {
        drop(self.stdin);
        self.child.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn image_compatibility_override_is_limited_to_qualified_grok_versions() {
        for version in ["1.0.50", "1.0.52"] {
            let initialized: super::InitializeResult = serde_json::from_value(serde_json::json!({"protocolVersion":1,"agentCapabilities":{"promptCapabilities":{"image":false}},"_meta":{"grokShell":true,"agentVersion":version}})).unwrap();
            assert!(!initialized.supports_images());
            assert!(initialized.supports_qualified_images());
            assert!(!initialized.supports_audio());
        }
        for metadata in [
            serde_json::json!({"grokShell":true,"agentVersion":"1.0.51"}),
            serde_json::json!({"grokShell":true,"agentVersion":"1.0.53"}),
            serde_json::json!({"agentVersion":"1.0.52"}),
            serde_json::json!({"grokShell":true,"agentVersion":"1.0.52-patched"}),
        ] {
            let initialized: super::InitializeResult = serde_json::from_value(
                serde_json::json!({"protocolVersion":1,"agentCapabilities":{},"_meta":metadata}),
            )
            .unwrap();
            assert!(!initialized.supports_qualified_images());
        }
    }
    use super::{
        AgentCommand, CLIENT_IDENTIFIER, CLIENT_NAME_ENV, FORBIDDEN_FLAGS, MAX_LINE_BYTES,
    };

    #[test]
    fn prompt_capabilities_require_explicit_upstream_booleans() {
        for capabilities in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!({"promptCapabilities":{"image":false,"audio":false,"embeddedContext":false}}),
            serde_json::json!({"promptCapabilities":{"image":"true","audio":1,"embeddedContext":{}}}),
        ] {
            let initialized: super::InitializeResult = serde_json::from_value(serde_json::json!({"protocolVersion":1,"agentCapabilities":capabilities,"_meta":{"promptCapabilities":{"image":true,"audio":true,"embeddedContext":true}}})).unwrap();
            assert!(!initialized.supports_images());
            assert!(!initialized.supports_audio());
            assert!(!initialized.supports_embedded_context());
        }
        let initialized: super::InitializeResult = serde_json::from_value(serde_json::json!({"protocolVersion":1,"agentCapabilities":{"promptCapabilities":{"image":true,"audio":true,"embeddedContext":true}},"_meta":{"promptCapabilities":{"image":false,"audio":false,"embeddedContext":false}}})).unwrap();
        assert!(initialized.supports_images());
        assert!(initialized.supports_audio());
        assert!(initialized.supports_embedded_context());
    }

    #[test]
    fn production_argv_is_exact_and_ordered() {
        let command = AgentCommand::new("grok");
        // Options precede the subcommand: `grok agent [OPTIONS] [COMMAND]`.
        assert_eq!(command.argv(), vec!["agent", "--no-leader", "stdio"]);
    }

    #[test]
    fn production_argv_never_contains_a_forbidden_flag() {
        let command = AgentCommand::new("grok");
        for flag in FORBIDDEN_FLAGS {
            assert!(
                !command.argv().contains(&flag),
                "{flag} must never be passed"
            );
        }
    }

    #[test]
    fn production_argv_never_uses_the_websocket_server() {
        // ADR light 0003: `agent serve` is not a production transport.
        let command = AgentCommand::new("grok");
        assert!(!command.argv().contains(&"serve"));
        assert!(!command.argv().contains(&"headless"));
        assert!(!command.argv().contains(&"leader"));
    }

    #[test]
    fn client_identity_is_the_light_identifier() {
        assert_eq!(CLIENT_IDENTIFIER, "grok-light");
        assert_eq!(CLIENT_NAME_ENV, "GROK_CLIENT_NAME");
    }

    #[test]
    fn client_identity_never_impersonates_another_product() {
        // ADR light 0007: Light must not pose as a recognised client to unlock
        // a richer permission option set.
        for impersonation in [
            "grok-web",
            "grok-desktop",
            "grok-pager",
            "grok-code-extension",
            "nebula",
        ] {
            assert_ne!(CLIENT_IDENTIFIER, impersonation);
        }
    }

    #[test]
    fn working_directory_is_optional_and_settable() {
        let command = AgentCommand::new("grok").with_working_directory("/tmp");
        assert_eq!(
            command.working_directory.as_deref(),
            Some(std::path::Path::new("/tmp"))
        );
    }

    #[test]
    fn line_bound_is_enforced_by_a_constant() {
        const { assert!(MAX_LINE_BYTES > 0) };
        const { assert!(MAX_LINE_BYTES <= 16 * 1024 * 1024) };
    }

    mod agent_errors {
        use super::super::{AcpError, METHOD_NOT_FOUND, agent_error};

        #[test]
        fn the_code_is_kept_alongside_the_message() {
            let error = agent_error(&serde_json::json!({
                "code": -32601,
                "message": "Method not found",
            }));
            assert!(matches!(
                error,
                AcpError::Agent { code: -32601, ref message } if message == "Method not found"
            ));
        }

        #[test]
        fn a_method_the_cli_lacks_is_recognised_as_unsupported() {
            // The user's CLI may predate a capability. That is an answer, not
            // a fault, and the caller is expected to degrade.
            let error = agent_error(&serde_json::json!({ "code": METHOD_NOT_FOUND }));
            assert!(error.is_unsupported_method());
        }

        #[test]
        fn another_failure_is_not_mistaken_for_a_missing_method() {
            let error = agent_error(&serde_json::json!({
                "code": -32000,
                "message": "not authenticated",
            }));
            assert!(!error.is_unsupported_method());
        }

        #[test]
        fn a_message_alone_never_makes_a_failure_look_unsupported() {
            // The message is agent-supplied text. If it could steer the
            // decision, an agent could talk the host into skipping a feature
            // that actually failed.
            let error = agent_error(&serde_json::json!({
                "code": -32000,
                "message": "Method not found",
            }));
            assert!(!error.is_unsupported_method());
        }

        #[test]
        fn a_missing_code_matches_nothing() {
            let error = agent_error(&serde_json::json!({ "message": "broke" }));
            assert!(matches!(error, AcpError::Agent { code: 0, .. }));
            assert!(!error.is_unsupported_method());
        }

        #[test]
        fn a_non_integer_code_is_not_coerced_into_a_real_one() {
            let error = agent_error(&serde_json::json!({ "code": "-32601" }));
            assert!(!error.is_unsupported_method());
        }
    }
}
