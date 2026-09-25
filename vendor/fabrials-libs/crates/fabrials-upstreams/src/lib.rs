//! Provider adapters for the fabrials-fabric proxy engine. The engine owns
//! routing and forwarding; each adapter supplies its upstream, credentials,
//! usage parsing and request shaping through the engine's `Provider` port.
pub mod catalog;
mod catalog_routes;
pub mod claude;
pub mod grok;
pub mod kimi;
pub mod nous;
pub mod openai;
pub mod opencode_go;
pub mod rankings;
pub mod routes;

pub mod codex;
