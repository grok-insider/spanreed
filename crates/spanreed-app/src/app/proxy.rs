//! The local proxy owned by this GUI process.
use crate::context::AppContext;

pub use crate::desktop_runtime::{ProxyState, Status};

pub fn status(ctx: &AppContext) -> Result<Status, String> {
    ctx.proxy().status()
}

pub fn start(ctx: &AppContext, bind: &str) -> Result<Status, String> {
    ctx.proxy().start(bind)
}

pub fn stop(ctx: &AppContext) -> Result<Status, String> {
    ctx.proxy().stop()
}
