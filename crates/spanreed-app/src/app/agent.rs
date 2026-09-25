//! The agent host for desktop.grok.me (`spanreed agent`, formerly
//! grok-bridge), started and stopped by the desktop or the tray.
use crate::app::proxy::ProxyState;
use crate::context::AppContext;

/// How `/healthz` and the CLI name this host.
pub const HOST_NAME: &str = "spanreed";
pub const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The command users type to reach the agent host.
pub const COMMAND: &str = "spanreed agent";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub state: ProxyState,
    /// Loopback origin once listening, e.g. `http://<id>.grok-light.localhost:23456`.
    pub origin: Option<String>,
    pub port: Option<u16>,
    pub error: Option<String>,
}

/// Port: `spanreed agent serve` inside this process, on its own port.
pub trait AgentRuntime: Send + Sync {
    fn status(&self) -> Result<AgentStatus, String>;
    fn start(&self) -> Result<AgentStatus, String>;
    fn stop(&self) -> Result<AgentStatus, String>;
}

pub fn status(ctx: &AppContext) -> Result<AgentStatus, String> {
    ctx.services().agent.status()
}

/// Serve on the agent host's own loopback port in this process.
pub fn start(ctx: &AppContext) -> Result<AgentStatus, String> {
    ctx.services().agent.start()
}

pub fn stop(ctx: &AppContext) -> Result<AgentStatus, String> {
    ctx.services().agent.stop()
}

/// Whether the host is running (or starting) in this process.
pub fn running(ctx: &AppContext) -> bool {
    status(ctx).is_ok_and(|status| status.state == ProxyState::Running)
}
