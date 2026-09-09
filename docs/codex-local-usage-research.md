# Codex local usage: ccusage and Tokscale review

Reviewed 2026-09-09. This is a source audit, not a claim that the parser improvements below have shipped.

## No proxy required

Spanreed already reads two independent sources:

- Account quota and resets: the authenticated ChatGPT usage/reset endpoints, through `src/providers/codex.rs` and the reset inventory helpers.
- Local tokens and estimated API-equivalent cost: Codex rollout JSONL, through `src/cost.rs`. This does not require routing requests through ai-relay.

The proxy measures requests actually routed through it. Local logs can include those same requests, so the two totals must remain separate. A subscription quota percentage cannot be reconstructed by dividing local tokens by a fixed token allowance. Local logs also cannot describe usage on other machines unless those machines synchronize their own observations.

## Pinned reference checkouts

| Project | Checkout | Reviewed commit |
| --- | --- | --- |
| [ccusage](https://github.com/ccusage/ccusage) | `/tmp/spanreed-ccusage-review` | `05cd43670fe2388a888812257f1d154b2d8870fe` |
| [Tokscale](https://github.com/junhoyeo/tokscale) | `/tmp/spanreed-tokscale-review` | `0d621cae343af9b057fb4a38c84d089524ac4378` |

ccusage's current Codex implementation is Rust (`rust/adapters/codex`), despite older documentation and examples referring to its TypeScript package. Tokscale's parser is `crates/tokscale-core/src/sessions/codex.rs`. Both projects use local records; Tokscale also has a separate live Codex quota command. Tokscale is MIT licensed; retain attribution if copying code. This audit copies no implementation code.

## Confirmed Spanreed gaps

`parse_codex_lines` sums every `last_token_usage`, without comparing consecutive `total_token_usage`. It has no Codex dedup key or inherited-parent-history handling. `collect_files(Source::Codex)` only scans `~/.codex/sessions`, ignoring `CODEX_HOME` and archived sessions. The 300-second cache can delay changes appearing. Local log presentation is currently coupled to a successful credential/quota probe, even though logs can be read offline.

A read-only sample of the five most recently modified local rollouts contained 13,593 token events and 216 repeated cumulative snapshots. The current summation attributed 17,898,745 tokens to those repeated snapshots. This is a diagnostic sample, not the account total, and it does not yet correct inherited parent history. Do not use it as a corrected billing figure.

## What to adopt

1. **Explicit parser state.** Track cumulative snapshots before applying the report date cutoff. Use reported last-request usage when valid; suppress unchanged cumulative snapshots and support cumulative-only events. Do not blindly subtract all cumulative values: compaction, corrections and out-of-order snapshots require distinct handling.
2. **Fork ownership.** Identify the file's own session metadata, parent session and child turn boundary. Replayed parent prefixes establish a baseline, not new usage. Tokscale handles inherited snapshots, nested forks and human-initiated forks. ccusage also has compatibility handling for older rewritten replay streams. Avoid treating timing heuristics as universal protocol guarantees.
3. **Discovery.** Honor the real Codex home and archived rollouts. Deduplicate overlapping roots and active/archive copies by source/session identity. A multi-root analytics setting should not change the actual Codex credential home.
4. **Token semantics.** Cached input is a subset of input; reasoning is a subset of output. Preserve breakdowns without charging either twice. Retain model identity and recorded service tier per event; label missing pricing rather than implying zero cost.
5. **Incremental imports.** Tokscale's `message_cache.rs` tracks fingerprints and resumable state. A Spanreed importer should persist parser version, file identity, offset and partial-line state; detect truncation, replacement and archive moves. Parsing and storage must remain separate from presentation.
6. **Useful views.** Daily/model/project/session breakdowns and source freshness would make local usage discoverable. Tokscale's aggregator and TUI are useful references. Start with native GUI/JSON outputs; a social leaderboard or prompt-summarizing service is not needed for local accounting.

## Suggested shared architecture

`LocalUsageSource` discovers inputs → a versioned pure Codex parser emits normalized usage records → an idempotent importer stores them → queries expose local daily/model/session projections → Spanreed renders and optionally synchronizes private aggregates. Keep quota probes and relay accounting as distinct ports with explicit provenance.

Host filesystem paths and credentials stay in Spanreed. Share normalized DTOs and aggregation rules with ai-relay, not a requirement for ai-relay to read workstation files. Do not upload prompts or responses to compute token usage.

Before replacing existing numbers, add fixtures for duplicate events, cumulative-only usage, cutoff baselines, compaction/regression, nested and user forks, archive moves, overlapping roots, reasoning/cache subsets, unknown models and partially written JSONL. Compare against pinned reference implementations and representative local metadata without publishing conversation contents.

References: [ccusage Codex guide](https://github.com/ccusage/ccusage/blob/05cd43670fe2388a888812257f1d154b2d8870fe/docs/guide/codex/index.md), [ccusage parser](https://github.com/ccusage/ccusage/blob/05cd43670fe2388a888812257f1d154b2d8870fe/rust/adapters/codex/src/parser.rs), [Tokscale Codex parser](https://github.com/junhoyeo/tokscale/blob/0d621cae343af9b057fb4a38c84d089524ac4378/crates/tokscale-core/src/sessions/codex.rs), [Tokscale cache](https://github.com/junhoyeo/tokscale/blob/0d621cae343af9b057fb4a38c84d089524ac4378/crates/tokscale-core/src/message_cache.rs).
