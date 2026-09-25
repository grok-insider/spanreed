//! The agent host for desktop.grok.me (`spanreed agent`, formerly
//! grok-bridge), started and stopped by the desktop or the tray.
use crate::context::AppContext;

pub use crate::desktop_runtime::AgentStatus;
pub use crate::desktop_runtime::agent_defaults as defaults;

pub fn status(ctx: &AppContext) -> Result<AgentStatus, String> {
    ctx.agent().status()
}

/// Serve on the agent host's own loopback port in this process.
pub fn start(ctx: &AppContext) -> Result<AgentStatus, String> {
    ctx.agent().start()
}

pub fn stop(ctx: &AppContext) -> Result<AgentStatus, String> {
    ctx.agent().stop()
}
