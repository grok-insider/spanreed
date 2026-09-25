//! The local HTTP API on 127.0.0.1:6736 (`spanreed serve`).
use crate::context::AppContext;

/// Serve cached probe results, refreshing every `refresh_secs`.
pub fn serve(ctx: &AppContext, refresh_secs: u64) -> std::io::Result<()> {
    crate::api::serve(refresh_secs, ctx.api_services())
}
