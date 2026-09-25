//! Provider adapters for the fabrials-runtime proxy engine. The engine owns
//! routing and forwarding; each adapter supplies its upstream, credentials,
//! usage parsing and request shaping through the engine's `Provider` port.
pub mod catalog;
pub mod claude;
pub mod grok;
pub mod kimi;
pub mod nous;
pub mod openai;
pub mod opencode_go;

pub mod codex;
