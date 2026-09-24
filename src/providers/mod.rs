//! Provider registry. Each provider is a native Rust module implementing
//! [`Provider`].

use crate::model::ProviderOutput;

pub mod aiand;
pub mod amp;
pub mod antigravity;
pub mod atlascloud;
pub mod bifrost;
pub mod chutes;
pub mod claude;
pub mod clawrouter;
pub mod clinepass;
pub mod codebuff;
pub mod coderabbit;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod deepgram;
pub mod deepinfra;
pub mod deepseek;
pub mod devin;
pub mod devpass;
pub mod elevenlabs;
pub mod factory;
pub mod fireworks;
pub mod gemini;
pub mod gitkraken;
pub mod grok;
pub mod groq;
pub mod huggingface;
pub mod hyper;
pub mod ibmbob;
pub mod jetbrains;
pub mod kilo;
pub mod kimi;
pub mod kiro;
pub mod litellm;
pub mod llmman;
pub mod llmproxy;
pub mod manus;
pub mod minimax;
pub mod moonshot;
pub mod muse;
pub mod neuralwatt;
pub mod nous;
pub mod ollama;
pub mod openai;
pub mod opencode_go;
pub mod openrouter;
pub mod perplexity;
pub mod poe;
pub mod session_only;
pub mod sub2api;
pub mod synthetic;
pub mod v0;
pub mod venice;
pub mod vercel;
pub mod warp;
pub mod wayfinder;
pub mod xai;
pub mod zai;
pub mod zenmux;

mod json_api;

/// A usage provider (Claude, Codex, ...).
pub trait Provider: Send + Sync {
    /// Stable id used on the CLI and local API (e.g. "claude").
    fn id(&self) -> &'static str;

    /// Human-friendly name (e.g. "Claude").
    fn name(&self) -> &'static str;

    /// Whether this provider has any local signal (creds/state) on this machine.
    /// Used to hide providers the user doesn't use. Probing a non-detected
    /// provider is allowed but typically yields an error line.
    fn detect(&self) -> bool;

    /// Fetch current usage. Implementations should return an error *line*
    /// (via `ProviderOutput::error`) rather than panicking.
    fn probe(&self) -> ProviderOutput;
}

fn noted(id: &'static str) -> Box<dyn Provider> {
    session_only::by_id(id).unwrap_or_else(|| panic!("missing session-only provider {id}"))
}

/// All known providers, in display order.
pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(codex::Codex),
        Box::new(claude::Claude),
        Box::new(cursor::Cursor),
        Box::new(grok::Grok),
        Box::new(nous::Nous),
        Box::new(opencode_go::OpenCodeGo),
        Box::new(amp::Amp),
        Box::new(zai::Zai),
        Box::new(minimax::MiniMax),
        Box::new(synthetic::Synthetic),
        Box::new(kimi::Kimi),
        Box::new(copilot::Copilot),
        Box::new(factory::Factory),
        Box::new(devin::Devin),
        Box::new(jetbrains::JetBrains),
        Box::new(kiro::Kiro),
        Box::new(antigravity::Antigravity),
        Box::new(perplexity::Perplexity),
        Box::new(openai::OpenAI),
        noted("azureopenai"),
        Box::new(clinepass::ClinePass),
        noted("opencode"),
        noted("alibaba"),
        noted("alibabatokenplan"),
        noted("qwencloud"),
        Box::new(fireworks::Fireworks),
        Box::new(gemini::Gemini),
        Box::new(manus::Manus),
        Box::new(kilo::Kilo),
        noted("vertexai"),
        noted("augment"),
        Box::new(moonshot::Moonshot),
        noted("t3chat"),
        Box::new(ollama::Ollama),
        Box::new(openrouter::OpenRouter),
        Box::new(elevenlabs::ElevenLabs),
        Box::new(warp::Warp),
        noted("windsurf"),
        noted("zed"),
        noted("mimo"),
        noted("doubao"),
        noted("sakana"),
        noted("abacus"),
        noted("mistral"),
        Box::new(deepseek::DeepSeek),
        Box::new(deepinfra::DeepInfra),
        Box::new(codebuff::Codebuff),
        Box::new(venice::Venice),
        noted("commandcode"),
        noted("qoder"),
        noted("stepfun"),
        noted("bedrock"),
        Box::new(groq::Groq),
        Box::new(llmproxy::LlmProxy),
        Box::new(litellm::LiteLLM),
        Box::new(bifrost::Bifrost),
        Box::new(deepgram::Deepgram),
        Box::new(poe::Poe),
        Box::new(chutes::Chutes),
        Box::new(neuralwatt::Neuralwatt),
        noted("helmcode"),
        Box::new(clawrouter::ClawRouter),
        noted("longcat"),
        Box::new(sub2api::Sub2Api),
        Box::new(wayfinder::Wayfinder),
        Box::new(zenmux::ZenMux),
        Box::new(aiand::AiAnd),
        noted("zoommate"),
        Box::new(xai::Xai),
        noted("notion"),
        Box::new(ibmbob::IbmBob),
        Box::new(muse::Muse),
        Box::new(coderabbit::CodeRabbit),
        noted("replicate"),
        Box::new(huggingface::HuggingFace),
        noted("pi"),
        Box::new(v0::V0),
        noted("typesafe"),
        Box::new(hyper::Hyper),
        Box::new(gitkraken::GitKraken),
        Box::new(devpass::DevPass),
        Box::new(atlascloud::AtlasCloud),
        Box::new(vercel::Vercel),
        Box::new(llmman::Llmman),
    ]
}

/// Look up a single provider by id.
pub fn by_id(id: &str) -> Option<Box<dyn Provider>> {
    all().into_iter().find(|p| p.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_unique() {
        let ids: Vec<_> = all().iter().map(|provider| provider.id()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        assert_eq!(ids.len(), 84);
        for id in [
            "openai",
            "clinepass",
            "azureopenai",
            "opencode-go",
            "jetbrains-ai-assistant",
            "llmman",
            "perplexity",
        ] {
            assert!(ids.contains(&id), "{id}");
        }
    }
}
