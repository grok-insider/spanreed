//! The local proxy owned by this GUI process.
use crate::context::AppContext;

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyState {
    Running,
    Stopping,
    Stopped,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: ProxyState,
    pub bind: String,
    pub error: Option<String>,
}

/// Port: the capture relay run inside this process.
pub trait ProxyRuntime: Send + Sync {
    fn status(&self) -> Result<Status, String>;
    fn start(&self, bind: &str) -> Result<Status, String>;
    fn stop(&self) -> Result<Status, String>;
}

pub fn status(ctx: &AppContext) -> Result<Status, String> {
    ctx.services().proxy.status()
}

pub fn start(ctx: &AppContext, bind: &str) -> Result<Status, String> {
    ctx.services().proxy.start(bind)
}

pub fn stop(ctx: &AppContext) -> Result<Status, String> {
    ctx.services().proxy.stop()
}
