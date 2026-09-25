//! The local HTTP API on 127.0.0.1:6736 (`spanreed serve`).
use crate::context::AppContext;
use crate::model::ProviderOutput;

/// Port: the local HTTP API server and its client.
pub trait LocalApiServer: Send + Sync {
    /// Serve cached probe results, refreshing every `refresh_secs`.
    fn serve(&self, ctx: &AppContext, refresh_secs: u64) -> std::io::Result<()>;
    /// The running daemon's cached outputs, if one is up.
    fn cached(&self) -> Option<Vec<ProviderOutput>>;
}

pub fn serve(ctx: &AppContext, refresh_secs: u64) -> std::io::Result<()> {
    ctx.services().local_api.serve(ctx, refresh_secs)
}
