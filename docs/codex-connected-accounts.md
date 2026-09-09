# Codex accounts and synchronized usage

Spanreed can upload selected local quota observations and daily log summaries to the connected Fabrials account. These appear on ai-relay Overview and Accounts even when no hosted provider grant exists. Synchronization does not upload OAuth credentials or enable proxy inference. API list-price estimates from local logs are separate from relay traffic: logs can span multiple CLI accounts and overlap captured requests.

A hosted Codex account can be authorized independently through OpenAI's device flow in Accounts. Spanreed provides the same flow for local managed accounts and the hosted workspace. Renew authorization on the existing account to retain its identity. Account fingerprints, device flows, quota, model catalogs and proxy keys are scoped to the Fabrials owner.

After authorizing the same account on ai-relay, select **Verify identity match** in Spanreed's synchronization settings. The one-time proof is an OpenAI-signed ID token with profile claims, never an access or refresh token. The relay checks its RS256 signature against OpenAI's fixed JWKS endpoint, issuer and Codex client audience, then requires an already-owned hosted account and a matching current source claim. An expired ID token is only historical identity evidence; it cannot authorize sign-in or import credentials. Proofs are not persisted. A new source identity invalidates the link; old quota backfill cannot replace the current identity. Older Spanreed source selections require confirmation when upgrading to identity binding.

## Proxy compatibility

| Surface | Behavior |
| --- | --- |
| `GET /codex/v1/models` | Owner/key-scoped subscription catalog, normalized to an OpenAI models list |
| `POST /codex/v1/responses` | Subscription Responses, streamed upstream; `store=false` |
| `POST /codex/v1/chat/completions` | Chat translation, text/images/function tools, stream or collected JSON |
| `GET /codex/v1/responses` with WebSocket upgrade | Native upstream WebSocket; sequential `response.create` requests; permissions and recorded-spend budgets checked before every creation |
| `/acct/<alias>/codex/v1/...` | Same surfaces pinned to the selected account |

Only one response may be in progress per WebSocket. Multiplexed lanes and mid-turn steering are not implemented. Files, batches, arbitrary credential endpoints and unknown provider routes remain blocked. Codex subscriptions reject output-token caps (`max_output_tokens`, `max_tokens`, `max_completion_tokens`); HTTP translation rejects these before calling the provider. Use an API provider when a client requires a token cap. Subscription transport is distinct from general OpenAI API access and may reject additional API-specific parameters. Key budgets are pre-request checks of recorded spend, not hard concurrent spending caps.

Quota and reset inventories come from the provider. Expiry dates and stale or unavailable readings retain their provenance. Review quotas remain separate from main routing utilization. Shared provider logos cover OpenAI/Codex, Anthropic/Claude, Cursor, Grok/xAI and Nous; unknown providers have a generic accessible fallback. SVG artwork provenance is recorded in the shared UI license.

## Client setup and exclusive relocation

Spanreed's hosted Connections page can review and save Codex or OpenCode configuration with an existing ai-relay proxy key. The key must belong to the connected Fabrials account and permit this account's model catalog. The host validates the model, keeps secrets out of the review DTO, preserves unrelated TOML/JSONC settings, saves a private backup and refuses a file changed after review. Codex uses Responses and enables WebSocket support; OpenCode adds an independent provider without changing its default model. Profile, project and command-line overrides may take precedence.

An optional **Move a local Codex session** flow transfers refresh ownership exclusively. Independent device authorization is the alternative when exclusivity cannot be established. Close Codex, OpenCode and other Spanreed processes first. Only a single known file-backed authorization is supported; system credential stores, multiple discovered files, managed local grants and potentially shared OpenCode OAuth authorizations block relocation. The review identifies the exact source and configuration paths and asks the user to confirm that other copies have been removed.

The native host journals private originals, configures Codex for the pinned hosted endpoint, durably retires the local authorization, then uploads the grant. Provider validation before commit is read-only. The server atomically creates the inactive hosted account, a client-generated 256-bit proxy key hash and an idempotent receipt. The plaintext client key stays local. A completed receipt removes retained OAuth from the local recovery journal. The local quota probe observes the retirement marker and cannot fall back to a hidden credential store.

If a response is lost, use **Check hosted receipt**. An absent receipt is not permission to restore. **Cancel and restore safely** must receive a durable server cancellation fence before restoring original files; a delayed import cannot cross that fence. Completed transfers cannot restore local refresh ownership. Changed local files cause recovery to stop instead of overwriting user changes. Do not manually delete an unresolved recovery record.

## Compatibility and validation

New local summaries and identity metadata are capability-negotiated. Existing v1 private events and old-client pull streams are unchanged. Local summaries replace older snapshots instead of incrementing totals. PostgreSQL queries and receipts include owner/device/source scope.

Fixtures cover fragmented SSE, Chat tools/images, once-only accounting, WebSocket per-creation policy and changed permissions, signed identity proof validation, latest-source projection, identity invalidation, daily snapshot replacement, cancellation fencing, retirement recovery and changed-file refusal. Real read-only Codex catalog/quota/reset checks succeeded on 2026-09-09. One real Chat SSE request and two sequential real WebSocket requests returned successful completions with separate usage records (91 tokens total; approximately USD 0.0000332 API list-price equivalent). No real session was relocated during this verification.

macOS/ARM qualification remains deferred until a contributor with Mac hardware can validate native storage, process checks and UI behavior. Windows signing remains deferred. Build configurations alone do not establish platform qualification; release records track native Linux/Windows checks separately.

Protocol references: [OpenAI Codex device auth](https://github.com/openai/codex/blob/main/codex-rs/login/src/device_code_auth.rs), [Codex provider configuration](https://github.com/openai/codex/blob/main/codex-rs/model-provider-info/src/lib.rs).
