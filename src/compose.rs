//! Composition root of the `spanreed` binary: the production adapters from
//! `spanreed-adapters` behind the `spanreed-app` ports.

use spanreed_app::context::AppContext;

pub(crate) fn context() -> AppContext {
    AppContext::new(spanreed_adapters::services::standard())
}
