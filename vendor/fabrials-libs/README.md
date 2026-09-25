# fabrials-libs

Shared Rust crates for Spanreed (local) and ai-relay (hosted). One Cargo
workspace, one version line, never published to crates.io (`publish = false`).

| Crate | Purpose |
|-------|---------|
| `fabrials-types` | Data types and portable contracts: hop and consumption records, metric lines, share snapshots, observations, provider capabilities, migration |
| `fabrials-pricing` | Price tables fed by an injectable `PricingSource`, data overlays, list-price cost, output rate |
| `fabrials-share` | Community snapshot economics from hops and from probed CLI output (`probed::from_output`, full-week estimates) |
| `fabrials-accounts` | Account registry types and provider-neutral autosteer (`PlanRanking`, `RankTable`, `PlanRanks`) |
| `fabrials-providers` | Provider protocols over an injected `HttpPort` (`ReqwestHttp` behind the default `reqwest` feature): OAuth and device flows, Grok CLI auth, SSE usage parsers, `SubscriptionProbe` |
| `fabrials-usage-import` | Local session readers (`SessionReader` registry), incremental Codex parser, remote usage protocols, normalization to `ConsumptionRecord` |
| `fabrials-fabric` | Proxy engine: listener, routing, forwarding; defines the provider roles (`Router`, `CredentialInjector`, `UsageExtractor`, `Translator`, `BodyShaper`, bundled as `Provider`/`ProviderParts`), the one Responses<->Chat translator (`wire_compat`), the configurable `ControlPrefix`, and the storage ports (`ports::{HopStore, HistoryStore, UsageStore, CredentialJournal, DeliveryStore}`); no SQLite |
| `fabrials-store-sqlite` | SQLite and file implementations of the storage ports, JSONL hop ledger, account-file transactions and API-key import |
| `fabrials-upstreams` | Provider adapters implementing the engine's provider roles, provider path routing (`routes`), the pinned provider catalog and the Grok model-list rewrite |
| `fabrials-agent-host` | Agent host that supervises the Grok Build CLI over ACP and serves the desktop.grok.me contract (edition 2024, MSRV 1.95) |

Dependencies point one way:

```
types <- pricing <- share
accounts (no internal dependency)
types <- providers
types <- usage-import
types, pricing, accounts <- fabric <- store-sqlite (implements fabric's ports)
fabric, providers, accounts <- upstreams (implements fabric's provider roles)
agent-host (stands alone)
```

The engine (`fabric`) names no provider and opens no database; provider
knowledge lives in `providers` and `upstreams`, persistence in `store-sqlite`.
`usage-import` needs nothing from `providers` today, so it depends on `types`
only.

## Consumers

Spanreed and ai-relay carry this workspace in-tree at `vendor/fabrials-libs`
(`git subtree`) and depend on the crates by `path`. Change code here first,
then pull it into each consumer with its `scripts/sync-fabrials-libs.sh`; the
consumer's CI rejects an in-tree copy that differs from the recorded revision.

Crate manifests spell out edition, license and path dependencies instead of
inheriting `workspace.*` fields: consumers load them as plain path
dependencies from inside their own workspace, where that inheritance does not
resolve.

## Migration

Phase 2 moved and renamed public API. Old path -> new path:

- `fabrials_runtime::*` -> `fabrials_fabric::*` (crate `fabrials-runtime` renamed to `fabrials-fabric`)
- `fabrials_metrics::*` -> `fabrials_pricing::*` (crate `fabrials-metrics` renamed to `fabrials-pricing`)
- `fabrials_metrics::{usage_from_messages_body, usage_from_response_body, UsagePartial}`, `fabrials_metrics::sse::*` -> `fabrials_providers::sse::*`
- `fabrials_metrics::{append, read_all}`, `fabrials_metrics::ledger::*` -> `fabrials_store_sqlite::ledger::{append, read_all}`
- `fabrials_providers::usage::*` -> `fabrials_usage_import::*` (`catalog`, `codex`, `files`, `remote` keep their names)
- `fabrials_providers::usage::files::SessionReader` (struct with `client` field and a `parse` fn pointer) -> `fabrials_usage_import::SessionReader` trait (`client()`, `read(path, sqlite)`); `READERS` is now `&[&dyn SessionReader]` and `files::reader()` returns `&'static dyn SessionReader`
- `fabrials_providers::usage::claude::Claude` (incremental Claude `UsageParser`, unused by any host) -> removed; Claude logs go through `files::read("claude", path)`
- Grok Build transcripts: `fabrials_usage_import::grok_build::{Reader, parse_grok_file, parse_grok_unified_log_file, parse_grok_updates_file}`
- Storage moved out of the engine into `fabrials-store-sqlite` and implements `fabrials_fabric::ports` (import the port trait, or keep calling the inherent methods):
  - `fabrials_runtime::hops::HopStore` -> `fabrials_store_sqlite::SqliteHopStore` (port `fabrials_fabric::ports::HopStore`)
  - `fabrials_runtime::history::HistoryStore` -> `fabrials_store_sqlite::SqliteHistoryStore` (port `ports::HistoryStore`); `history::{is_reset_event, is_duplicate, prepare_sample}` stay in `fabrials_fabric::history`
  - `fabrials_runtime::local_usage::{UsageStore, StoredSource, ImportBatch}` -> `fabrials_store_sqlite::SqliteUsageStore`, `fabrials_fabric::ports::{StoredSource, ImportBatch}` (also re-exported from `fabrials_store_sqlite::local_usage`)
  - `fabrials_runtime::credential_journal::{Rotation, Scope, Recovery, RotationSource}` -> `fabrials_store_sqlite::credential_journal::Rotation` (port `ports::CredentialJournal`), `fabrials_fabric::ports::{Scope, Recovery, RotationSource}`
  - `fabrials_runtime::notifications::{DeliveryStore, Claim}` -> `fabrials_store_sqlite::notifications::{SqliteDeliveryStore, Claim}` (port `ports::DeliveryStore`)
  - `fabrials_runtime::{file_set, files}` -> `fabrials_store_sqlite::{file_set, files}`
  - `fabrials_runtime::migration::import_files` -> `fabrials_store_sqlite::migration::import_files` (validation stays in `fabrials_fabric::migration`)
- `fabrials_runtime::provider::Provider` (one trait, 11 methods) -> five role traits in `fabrials_fabric::provider`: `Router` (`id`, `resolve`, `classify`, `allows_request`), `CredentialInjector` (`inject`, `inject_for`, `bind_credential`, `credential_headers`), `UsageExtractor` (`parse_usage`), `Translator` (`translated_hop`, `websocket_hop`, new `rewrites_models_list`), `BodyShaper` (new `shape_request`). `Provider` is now implemented automatically for any type with all five; implement `Translator`/`BodyShaper` as empty impls to keep the defaults, or assemble a `ProviderParts`. Callers calling role methods on a concrete adapter import the role trait.
- `fabrials_runtime::forward::HopObserver::authorize_response` no longer has an `Err` default: every observer implements it.
- The engine no longer rewrites Grok model listings by route name; `GrokAdapter` opts in through `Translator::rewrites_models_list`, and `ClaudeAdapter` now shapes the identity `system` block itself for `sk-ant-oat` sessions (`BodyShaper`), so hosts can drop their own `shape_identity_system` call (it stays public and idempotent).
- `fabrials_runtime::routes::{parse_fabric_path, FabricRoute, UPSTREAM_OPENCODE_GO, UPSTREAM_GROK_CLI, UPSTREAM_XAI_API, is_xai_media_path}` -> `fabrials_upstreams::routes::*`; `routes::is_safe_request_target` stays in `fabrials_fabric::routes`
- `fabrials_runtime::routes::{is_health_path, is_environment_path, is_autosteer_path, is_limits_path, is_ingest_path, is_sync_path}` -> `fabrials_fabric::routes::ControlPrefix::{is_health, is_environment, is_autosteer, is_limits, is_ingest, is_sync}` (`ControlPrefix::default()` is `/__spanreed`; `ControlPrefix::new("/__relay")` moves it). `ConnectionHost::control_prefix()` tells the listener which health path to admit under load.
- `fabrials_runtime::catalog::*` -> `fabrials_upstreams::catalog::*`
- `fabrials_runtime::models::*` (Grok model catalog rewrite) -> `fabrials_upstreams::grok::models::*`
- `fabrials_upstreams::codex::translation::chat_response` -> `fabrials_fabric::wire_compat::responses_to_chat_response`; the chat-to-Responses half of `codex::translation::responses_request` is `fabrials_fabric::wire_compat::chat_request_to_responses` (Codex keeps only its subscription policy)
- Protocol clients take an `Arc<dyn fabrials_providers::http::HttpPort>`: `claude::Client::with_http(http, origins)`, `kimi::Client::with_http(http, origins)`, `codex::auth::Client::with_http(http)`, `nous::Client::with_http(http, client_id)`, `grok::device::Client::with_http(http)`, `codex::auth::identity::verify_with(http, token, now)`. The old constructors (`new`, `with_origins`, `identity::verify`) remain behind the default `reqwest` feature and use `http::ReqwestHttp`. Every `HttpPort` is also a `grok_cli::TokenHttp`.
- Claude/Kimi plan-window metadata (hosts' `probe_metadata`) -> `fabrials_providers::subscription::{probe_metadata, ProbeMetadata, SubscriptionProbe}`, implemented by `claude::Subscription` and `kimi::Subscription`
- `fabrials_accounts::{grok_plan_rank, grok_burn_rank, cursor_plan_rank, GROK_PLAN_RANK_MAX}` -> `fabrials_upstreams::rankings::{GROK_PLANS, GROK_BURN, CURSOR_PLANS}` (`RankTable`s; call `.rank(slug)` with `fabrials_accounts::PlanRanking` in scope, or look up `rankings::default_rankings().rank(provider, slug)`); `GROK_PLAN_RANK_MAX` is `GROK_PLANS.max()`
- `fabrials_accounts::{load, save, index_path}` -> `fabrials_store_sqlite::accounts::{load, save, index_path}` (the registry types stay serde types in `fabrials-accounts`)
- Spanreed `share_economics::{estimate_full_week, FullWeekEst, parse_usd_and_tokens, from_output}` -> `fabrials_share::probed::*` (also re-exported at the crate root). `from_output(o, by_model, first_pct)` takes the pool baseline as `&dyn Fn(&ProviderOutput) -> Option<f64>` instead of reading Spanreed's `pool_baseline`/`forecast` files; the pure forecast helpers `density_oneshot`, `origin_is_near_zero`, `project_week_to_full`, `scale_span_to_full`, `MIN_PCT_DELTA`, `MIN_PCT_ONESHOT`, `NEAR_ORIGIN_PCT` and `WeekProjection` live in `fabrials_share::probed`. `model_breakdown_for` stays in Spanreed (it reads its ledger and logs).
- Price layers: `build_table(embedded, remote, user)` still works; hosts that own their layers implement `fabrials_pricing::PricingSource` (or fill `PricingLayers`) and call `PricingMap::from_source`. The GPT-6 Astra, Claude 5, xAI media and OpenCode Go rates now come from `pricing-overlays.json` (`embedded_overlays_json()`).

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## License

AGPL-3.0-or-later (`LICENSE`). Session readers adapted from Tokscale keep
their MIT notice in `crates/fabrials-usage-import/src/formats/TOKSCALE-LICENSE`.
