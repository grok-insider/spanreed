// Adapted from Tokscale 0d621cae343af9b057fb4a38c84d089524ac4378.
// Copyright (c) 2025 Junho Yeo. MIT; see TOKSCALE-LICENSE.
use once_cell::sync::Lazy;
use std::collections::HashMap;

static CURSOR_PRICING_ALIASES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut aliases = HashMap::new();
    for tier in [
        "cursor-grok-4.6-high",
        "cursor-grok-4.6-high-fast",
        "cursor-grok-4.6-low",
        "cursor-grok-4.6-low-fast",
        "cursor-grok-4.6-medium",
        "cursor-grok-4.6-medium-fast",
        "cursor-grok-4.6-xhigh",
    ] {
        aliases.insert(tier, "grok-4.6");
    }
    aliases.insert("grok-composer-2.5", "composer-2.5");
    aliases.insert("grok-composer-2.5-fast", "composer-2.5-fast");
    aliases
});

static MODEL_ALIASES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("big-pickle", "glm-4.7");
    m.insert("big pickle", "glm-4.7");
    m.insert("bigpickle", "glm-4.7");
    m.insert("k2p5", "kimi-k2-thinking");
    m.insert("k2-p5", "kimi-k2-thinking");
    m.insert("k2p6", "kimi-k2.6");
    m.insert("k2-p6", "kimi-k2.6");
    m.insert("kimi-k2p6", "kimi-k2.6");
    m.insert("kimi-k2.5-thinking", "kimi-k2-thinking");
    // Kimi CLI reports `kimi-for-coding` for Kimi K2.7 Code (its config sets
    // `display_name = "K2.7 Coding"`). It was previously aliased to k2.5, which
    // priced it about a third under the real rate.
    m.insert("kimi-for-coding", "kimi-k2.7-code");
    m.insert("kimi-for-coding-highspeed", "kimi-k2.7-code-highspeed");
    m.insert("k3", "kimi-k3");
    // models.dev also publishes `kimi-for-coding/k3-256k` at $0.00, so the
    // long-context spelling must resolve to the same real moonshotai row as
    // bare `k3` instead of landing on the zero-priced subscription namespace.
    m.insert("k3-256k", "kimi-k3");
    // Kimi Work (the Kimi desktop app's agent mode) embeds the same kimi-code
    // kernel and writes the same wire protocol, but reports its own ids.
    // Unaliased they fuzzy-match badly: `k2d6-agent` landed on
    // `xai/grok-4.20-multi-agent-beta-0309`, and the `k3-agent*` ids fell into
    // the Models.dev `kimi-for-coding/*` subscription namespace, which prices
    // at $0.00.
    m.insert("k2d6-agent", "kimi-k2.6");
    m.insert("k3-agent", "kimi-k3");
    m.insert("k3-agent-swarm", "kimi-k3");

    // MiniMax M3: Ollama Cloud and other routers report the model with the
    // lowercase bare id `minimax-m3` (and mixed-case variants), while the
    // authoritative dataset key is `minimax/MiniMax-M3` (litellm). The bare id
    // has no exact hit in any dataset, so with no usable provider hint it falls
    // through to model-part matching across every row whose model part is
    // `minimax-m3` — and models.dev publishes that model part under dozens of
    // third parties, several at 0.0/0.0 (`kenari/minimax-m3`,
    // `nvidia/minimaxai/minimax-m3`). Electing one of those prices real usage
    // at exactly $0, which is the "pricing missing" symptom in #935. Pin the
    // canonical first-party key so the id prices deterministically.
    m.insert("minimax-m3", "minimax/MiniMax-M3");

    m.insert("model_placeholder_m26", "claude-opus-4-6");
    m.insert("model_placeholder_m35", "claude-sonnet-4-6");
    m.insert("model_placeholder_m36", "gemini-3.1-pro");
    m.insert("model_placeholder_m37", "gemini-3.1-pro");
    // Antigravity uses opaque placeholder IDs in IDE metadata and shorter
    // responseModel aliases in CLI conversation protobufs. The evidence has
    // two distinct roles:
    //
    // - Antigravity Manager is a third-party account/quota manager. Its quota
    //   client documents the server-side metadata source and response shape:
    //   model IDs and display names come from Google Cloud Code Assist's
    //   fetchAvailableModels API.
    //   https://github.com/lbjlaq/Antigravity-Manager/blob/dfe876548d572237da92fe4c3e070a9db33c0910/src-tauri/src/modules/quota.rs
    // - The concrete placeholder and responseModel mappings below come from
    //   Antigravity Context Window Monitor's GetUserStatus/session registry.
    //   https://github.com/AGI-is-going-to-arrive/Antigravity-Context-Window-Monitor/blob/603e3ea00a0ee94f1beecc162cf47a4ed68d3a6f/src/models.ts
    //
    // Keep these as machine-ID aliases. Do not use server-provided display
    // labels as pricing keys because labels may be renamed or localized.
    //
    // M133/`gemini-3-flash-b`, `gemini-3-flash-a`, and M187/raw
    // `gemini-3.5-flash-low` are cases where the obvious mapping is wrong,
    // verified against the pinned Antigravity Context Window Monitor SHA
    // above (models.ts@603e3ea):
    //
    // - M133 was renamed from "Gemini 3 Flash" to "Gemini 3.5 Flash (High)"
    //   ("MODEL_PLACEHOLDER_M133": 'Gemini 3.5 Flash (High)', // gemini-3-flash-agent
    //   (renamed from "Gemini 3 Flash")"), and `responseModelAliases` maps
    //   BOTH `gemini-3-flash-agent` and `gemini-3-flash-b` to M133. So M133
    //   and `gemini-3-flash-b` must resolve identically to `gemini-3-flash-agent`
    //   (gemini-3.5-flash-high), not to the retired gemini-3-flash-preview tier.
    // - `responseModelAliases['gemini-3-flash-a'] = 'MODEL_PLACEHOLDER_M132'`
    //   ("legacy responseModel for 3.5 Flash"), and
    //   `STATIC_MODEL_NAME_FALLBACKS['MODEL_PLACEHOLDER_M132'] =
    //   'Gemini 3.5 Flash (High)' // retired predecessor of M133`. So
    //   `gemini-3-flash-a` prices as the retired-predecessor High tier
    //   (gemini-3.5-flash-high) — the same catalog entry as M133/M132/
    //   `gemini-3-flash-b` — not as the unrelated gemini-3-flash-preview
    //   family (M18/M84), which is a different, older backend command model.
    // - M20's `activeModelSpecs` entry has `modelId: 'gemini-3.5-flash-low'`
    //   with `displayName: 'Gemini 3.5 Flash (Medium)'` — the wire string
    //   says "low" but the tier is actually Medium. M187 is a distinct
    //   placeholder whose own `activeModelSpecs` entry has
    //   `modelId: 'gemini-3.5-flash-extra-low'` and
    //   `displayName: 'Gemini 3.5 Flash (Low)'` — the true Low tier. M187
    //   and M20/raw `gemini-3.5-flash-low` must NOT collapse to the same
    //   canonical alias target: M187 maps to `gemini-3.5-flash-extra-low`
    //   (its own machine ID), while M20 and the raw wire string map to
    //   `gemini-3.5-flash-medium`.
    m.insert("model_placeholder_m16", "gemini-3.1-pro");
    m.insert("model_placeholder_m18", "gemini-3-flash-preview");
    m.insert("model_placeholder_m84", "gemini-3-flash-preview");
    m.insert("model_placeholder_m132", "gemini-3.5-flash-high");
    m.insert("model_placeholder_m133", "gemini-3.5-flash-high");
    m.insert("model_placeholder_m187", "gemini-3.5-flash-extra-low");
    m.insert("model_placeholder_m20", "gemini-3.5-flash-medium");
    m.insert("gemini-pro-default", "gemini-3.1-pro");
    m.insert("gemini-pro-agent", "gemini-3.1-pro");
    m.insert("gemini-3-flash-agent", "gemini-3.5-flash-high");
    m.insert("gemini-3-flash-b", "gemini-3.5-flash-high");
    m.insert("gemini-3.5-flash-low", "gemini-3.5-flash-medium");
    m.insert("model_placeholder_m47", "gemini-3-flash-preview");
    m.insert("model_openai_gpt_oss_120b_medium", "gpt-oss-120b-medium");
    m.insert("claude-opus-4-6-thinking", "claude-opus-4-6");
    m.insert("claude-sonnet-4-6-thinking", "claude-sonnet-4-6");
    m.insert("claude-opus-4.6-thinking", "claude-opus-4-6");
    m.insert("claude-sonnet-4.6-thinking", "claude-sonnet-4-6");
    m.insert("claude-opus-4-6", "claude-opus-4-6");
    m.insert("claude-sonnet-4-6", "claude-sonnet-4-6");
    m.insert("claude-haiku-4-6", "claude-haiku-4-6");
    m.insert("claude-opus-4.6", "claude-opus-4-6");
    m.insert("claude-sonnet-4.6", "claude-sonnet-4-6");
    m.insert("claude-haiku-4.6", "claude-haiku-4-6");
    // Anthropic's "-0" suffix is their documented moving alias for the latest
    // snapshot of a model line (claude-opus-4-0 -> newest Opus 4). Datasets
    // publish the dated key instead, so the alias form resolved to nothing and
    // real first-party usage was excluded from submission as unpriced.
    m.insert("claude-opus-4-0", "claude-opus-4");
    m.insert("claude-sonnet-4-0", "claude-sonnet-4");
    // GitHub Copilot reports Claude 4.1 without the separator. Copilot usage is
    // priced at the underlying model's rates (its own $0.00 subscription rows
    // are filtered out by EXCLUDED_LITELLM_PREFIXES), so this must resolve the
    // same way github_copilot/gpt-4o already resolves to gpt-4o.
    // Deliberately opus-only: `claude-sonnet-4-1` currently resolves to
    // `databricks/databricks-claude-sonnet-4-1` via a cross-vendor fuzzy match
    // (#1062), so aliasing the Copilot spelling onto it would route Sonnet 4.1
    // usage to Databricks rates. Add it once #1062 makes that target safe.
    m.insert("claude-opus-41", "claude-opus-4-1");
    m.insert("anthropic/claude-4-5-opus", "claude-opus-4-5");
    m.insert("anthropic/claude-4-5-sonnet", "claude-sonnet-4-5");
    m.insert("anthropic/claude-4-5-haiku", "claude-haiku-4-5");
    m.insert("anthropic/claude-4-6-opus", "claude-opus-4-6");
    m.insert("anthropic/claude-4-6-sonnet", "claude-sonnet-4-6");
    m.insert("anthropic/claude-4-6-haiku", "claude-haiku-4-6");
    m.insert("gemini-3.1-pro-high", "gemini-3.1-pro");
    m.insert("gemini-3.1-pro-low", "gemini-3.1-pro");
    m.insert("gemini-3-pro-high", "gemini-3-pro");
    m.insert("gemini-3-pro-low", "gemini-3-pro");
    m.insert("gemini-3-flash", "gemini-3-flash-preview");
    m.insert("gemini-3-flash-c", "gemini-3-flash-preview");
    m.insert("gemini-3-flash-a", "gemini-3.5-flash-high");
    // OpenAI documents the API spelling below as a moving alias for
    // `gpt-5.6-sol`; Codex records the same alias with its `gpt-` prefix.
    // Keep the API, Codex, and provider-qualified spellings pinned to the
    // currently documented target so the upstream GPT-5.6 Sol row supplies
    // all token-bucket rates. The qualified form must be explicit because
    // provider-prefix stripping does not run alias resolution a second time.
    // Sources (accessed 2026-08-17):
    // https://developers.openai.com/api/docs/guides/safety-checks/cybersecurity
    // https://developers.openai.com/api/docs/pricing
    m.insert("daybreak-blue-latest", "gpt-5.6-sol");
    m.insert("gpt-daybreak-blue-latest", "gpt-5.6-sol");
    m.insert("openai/gpt-daybreak-blue-latest", "gpt-5.6-sol");
    m.insert("openai/daybreak-blue-latest", "gpt-5.6-sol");

    // Stealth preview shorthands for the August 2026 Z.AI GLM-5.3-Flash free
    // preview. Sessions record the bare `ox-alpha` (and the router-qualified
    // `stealth/ox-alpha` gateways emit), while upstream models.dev tracks the
    // canonical free incarnations `opencode-go/ox-alpha-free` and
    // `opencode/x-preview-f-free` at $0.00 (both deprecated). Pin the
    // shorthand spellings to those canonical keys -- the same "canonical
    // first-party key" pattern as `minimax-m3` above -- so the live upstream
    // $0 row prices them instead of an unverified reseller guess. The
    // qualified `stealth/` form must be explicit because provider-prefix
    // stripping does not run alias resolution a second time.
    // Sources (accessed 2026-09-03):
    // https://openrouter.ai/stealth/ox-alpha ("free to use", ZAI reveal)
    // https://docs.z.ai/guides/vlm/glm-5.3-flash ("tested anonymously as ox-alpha")
    // https://models.dev/api.json (providers.opencode.models.x-preview-f-free
    // and providers.opencode-go.models.ox-alpha-free at input = 0,
    // output = 0, cache_read = 0, both deprecated)
    m.insert("ox-alpha", "opencode-go/ox-alpha-free");
    m.insert("stealth/ox-alpha", "opencode-go/ox-alpha-free");
    m.insert("x-preview-f-free", "opencode/x-preview-f-free");

    // Synthetic model variants (only where resolver needs help)
    m.insert("kimi-k2.5-nvfp4", "kimi-k2.5"); // Quantization variant → base model pricing
    m.insert("kimi-k2-instruct-0905", "kimi-k2.5"); // Specific version → base (avoids reseller)
    m
});

pub fn resolve_alias(model_id: &str) -> Option<&'static str> {
    let lowered = model_id.to_lowercase();
    if let Some(target) = MODEL_ALIASES.get(lowered.as_str()) {
        return Some(target);
    }
    if let Some(target) = CURSOR_PRICING_ALIASES.get(lowered.as_str()) {
        return Some(target);
    }
    // kimi-code reports some rows as `kimi-code/<id>`. The Kimi parser strips
    // that prefix before pricing, but any other path reaching pricing with the
    // qualified form would otherwise miss every alias above and fall through to
    // the Models.dev `kimi-for-coding/*` namespace, which prices at $0.00 — so
    // the qualified and bare spellings of the same model would disagree.
    let bare = lowered.strip_prefix("kimi-code/")?;
    MODEL_ALIASES.get(bare).copied()
}

pub fn uses_cursor_pricing(model_id: &str) -> bool {
    CURSOR_PRICING_ALIASES.contains_key(model_id.to_lowercase().as_str())
}
