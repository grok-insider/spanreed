# AGENTS.md

Instructions for AI agents and contributors working on spanreed.

## Project overview

spanreed is a cross-platform (Linux-first) AI coding subscription usage
tracker: one Rust binary (`spanreed`) that acts as a CLI, a background daemon,
and a data source for status bars. It reads local AI-CLI credentials, queries
each provider's usage API, and renders the result.

- Cargo workspace (see **Module layout**) with an optional Tauri host under `desktop/`. Shared DTOs/pricing/accounts live in the
  `fabrials-libs` workspace, carried in-tree at `vendor/fabrials-libs` (git subtree, `path` deps). Change the
  crates in `~/dev/fabrials/libs/fabrials-libs`, then run `scripts/sync-fabrials-libs.sh`; CI runs
  `scripts/check-fabrials-libs.sh` and rejects direct edits to the copy.
  `spanreed_domain::model` re-exports the `fabrials-types` output model.
- `spanreed capture serve` runs the shared `fabrials-fabric` engine (provider adapters from `fabrials-upstreams`, SQLite stores from `fabrials-store-sqlite`) directly; no ai-relay executable is required. The optional xAI compatibility listener shares the runtime.
- The Cargo workspace excludes `vendor/fabrials-libs` from its own members; CI tests it with `--manifest-path`. The binary target `spanreed` (`src/main.rs`) is the root package; `default-members` include every Spanreed crate, so plain `cargo test`/`cargo clippy` cover them all.
- Probes are blocking I/O fanned out over threads. The shared runtime contains Tokio transport; Tauri runs blocking probes off the renderer thread.
- Providers are **native Rust** modules implementing one trait. There is no
  embedded scripting engine and no plugin sandbox.
- **Accounts** are host-owned (`crates/spanreed-adapters/src/accounts/`, `spanreed account`). Drivers
  (`crates/spanreed-adapters/src/drivers/`) implement login/refresh/billing. Grok uses native device-code.
- **Addons** are a session-shaped protocol (today still one-shot JSON + inproc
  trait) for extra CLI prefixes. See `docs/addons.md`.
- Credentials are read from where each CLI stores them: XDG paths, plaintext
  files, SQLite state DBs (`rusqlite`, read-only), the GitHub CLI, `/proc`, and
  the OS secret store — Secret Service via `secret-tool` on Linux, Keychain on
  macOS, Credential Manager on Windows. Linux/Wayland is the primary target; the
  Windows builds and native smoke checks are verified. macOS/ARM qualification
  is deferred pending a contributor with Mac hardware; configuration alone is
  not evidence of support (see `desktop/README.md`).

## Module layout

Dependency direction: `spanreed-domain` ← `spanreed-app` ← `spanreed-adapters` and
`spanreed-tray`; the CLI (root package) and `desktop/src-tauri` are the
composition roots. `spanreed-app` performs no I/O: every outbound dependency is a
port trait (`crates/spanreed-app/src/ports.rs` and one port per facade area), and
`spanreed-adapters` implements them. The roots build
`AppContext::new(spanreed_adapters::services::standard())` (`src/compose.rs`,
`desktop/src-tauri/src/main.rs`); the tray and the CLI subcommands only call
`spanreed_app::app`. `cargo tree -e normal -p spanreed-app | grep spanreed-adapters`
must stay empty.

| Crate / path | Owns |
|------|------|
| `src/main.rs`, `src/lib.rs`, `src/compose.rs` | The `spanreed` binary: `run_cli`, composition root. Root package `spanreed` (release version, `tray` and `contracts` features forwarding to the crates, `tests/`). |
| `src/cli/` | Command line: `mod.rs` (SIGPIPE, logging, the `COMMANDS` table, addon prefix fallback, `grok-bridge` argv0 alias), `args.rs`, `help.rs`, and one file per subcommand (`probe`, `capture`, `agent`, `setup`, `share`, `usage`, `sync`, …). Hand-written parsing; no clap/lexopt in the lock. |
| `crates/spanreed-domain` | No I/O: `model` (re-exports `fabrials-types`), `output` (plain/Waybar renderers), `tray_format`, `usage_stats`, `provider_icons`, `util` (time, formatting, JWT), `ports` (`CostSource`, `AfterProbe`, `Notifier`, `SnapshotProvider`, `ProbePorts`). |
| `crates/spanreed-app/src/context.rs` | `AppContext`, built once per process from a `ports::Services` bundle; owns the probe snapshot cache and runs probes with the cost port and probe hooks. |
| `crates/spanreed-app/src/ports.rs` | `Provider`, `ProviderCatalog` (provider drivers/probes), `Services`, `AppPaths`, and re-exports of the domain probe ports and of each area port. |
| `crates/spanreed-app/src/probe.rs` | Probe orchestration over `ProviderCatalog` (tested with fakes). |
| `crates/spanreed-app/src/app/` | The facade used by the CLI, the tray, the desktop host and the local API. Each area owns its DTOs and its port: `usage` (`UsageStore`, `PricingSource`), `accounts` (`AccountStore`, `DeviceLogins`), `routing` (`RoutingStore`), `notifications` (settings, delivery, OS notifier), `sharing`, `sync`, `fabrials` (HTTP), `migration`, `clients` (`ClientConfigurator`), `proxy`/`agent` (in-process runtimes), `capture` and `setup` (`Installer`: OS service managers), `updates`, `window` (`DesktopBridge`), `addons`, `profiles`, `local_api`. `desktop_contracts.rs` generates the renderer types. |
| `crates/spanreed-adapters/src/services.rs` | One implementation per port and `standard()`, the production `Services`. |
| `crates/spanreed-adapters/src/*` | The adapters: the rest of this table. |
| `crates/spanreed-tray` | `spanreed tray` (feature `tray`; empty without it, so workspace builds need no GTK): `menu`, `state`, `actions`, `visual`, `popover`, `platform`. Nix package builds this; musl GH zips do not. |
| `desktop/` | Tauri + React local console (`src-tauri/src/{main,commands,notifications,shell}.rs`); `main.rs` composes the `AppContext`; commands include `agent_status`/`agent_start`/`agent_stop` (`AgentStatus` in `src/contracts.ts`) and the app tray menu has a Start/Stop agent host entry. Shared styles/components from `@fabrials/ui`. |
| `privacy.rs` | Independent, default-off metrics publication and history synchronization consent. |
| `history.rs` | Quota observations in `runtime.sqlite3` through shared `fabrials-store-sqlite` (`SqliteHistoryStore`; reset/duplicate rules in `fabrials_fabric::history`); one-time import preserves `usage-history.jsonl`. CLI and desktop read the same store. |
| `local_tokens.rs` | Grok refresh through shared durable rotation journal and scoped advisory locks; journal is separate from usage data. Nous uses the same journal from its driver. |
| `profiles.rs` | Built-in Waybar, Eww and SketchyBar fragments; explicit new-file installation. |
| `desktop_runtime/` | Controllers owned by a GUI process: the local proxy (`ProxyControl`) and the embedded agent host (`agent.rs`, `AgentControl`: `spanreed agent serve` on its own thread/runtime and its own port, never 18736). |
| `addons/` | Addon protocol, host (PATH/toml/inproc), `grok-accounts` shim (`spanreed grok` → `spanreed account`). |
| `accounts/` | Host identity registry (`mod.rs`), secrets/keyring/rotation (`vault.rs`), quota and billing snapshots (`snapshot.rs`); shared recoverable file transactions coordinate mutations with refreshes. Account generations prevent stale authorization writes. |
| `drivers/` | First-party identity drivers (`grok` login/probe/fabric token). |
| `product.rs` | Product identity (`spanreed`): dirs, bin name, GitHub repo. |
| `probe.rs` | Probe orchestration: runs detected (or all/one) providers concurrently with `ProbePorts`. |
| `providers/mod.rs` | The `Provider` trait, the `all()` registry, and `by_id()`. Register new providers here. |
| `providers/*.rs` | One provider each (`claude`, `codex`, `grok`, ...). |
| `creds.rs` | Linux credential & local-state discovery helpers (paths, files, SQLite, secret-tool, `/proc`, Antigravity process discovery). |
| `http.rs` | Blocking HTTP client wrapper: `Request` builder, proxy support (read per client), optional insecure TLS. |
| `api.rs` | Local HTTP API on `127.0.0.1:6736` (`/usage`, `/health`) with background refresh through a `SnapshotProvider` and a `Notifier`. |
| `cost.rs` | Cost presentation (Claude/Codex) from the shared local consumption store; `LocalCost` is the `CostSource`. |
| `grok_ledger.rs` | Capture ledger in shared `runtime.sqlite3`; legacy `grok-usage.jsonl` is imported once and preserved. Grok metrics filter by provider. Dollars = **public API list price** via `pricing` (not SuperGrok `cost_in_usd_ticks`); xAI all-or-nothing ≥200k long-context tier per request. |
| `setup/` | Install binary to user PATH, ledger dir, optional capture user service, optional tray autostart, share schedule, wire Grok Build + OpenCode xAI to the local capture proxy. `apply.rs` applies a `SetupPlan`; the interactive wizard is `src/cli/setup.rs`. |
| `self_update.rs` | GitHub Releases check + sha256-verified binary replace (`spanreed self-update`). |
| `tray_card/` | Tray usage card: model (`mod.rs`), HTML sections (`html.rs`), `card.css`, `card.js`. |
| `capture_log.rs` | Capture/watchdog log file (`…/spanreed/logs/capture.log`) with size rotation. |
| `capture_watchdog.rs` | `capture serve --watchdog`: restart worker when ports die / process exits. |
| `forecast.rs` | Week/month Expected lines from pool-% density samples (projection math from `fabrials_share::probed`). |
| `share_economics.rs` | Share economics: `fabrials_share::probed::from_output` with Spanreed's pool baseline, plus `model_breakdown_for` from the ledger and local logs. |
| `epoch.rs` | Early weekly reset detection (gift/outage) vs scheduled rollover. |
| `pool_baseline.rs` | First-seen Weekly % per provider/week for span scaling. |
| `pricing.rs` | Model price `Catalog`: embedded LiteLLM snapshot + runtime refresh (LiteLLM families, then models.dev for the OpenCode Go channel) cached 7 days + user override. The same refresh stores context windows in `limits-remote.json`. |
| `sync/` | Private history synchronization; `run.rs` is one run (push, pull, usage snapshots); `HistorySync` is the `AfterProbe` hook. |
| `local_relay.rs` | Local composition of the shared proxy runtime (admit, control endpoints, authorize, forward). |

Process-wide statics are limited to: the local UTC offset captured before threads start (`util`), the `config.json` write guard (`local_control`), the usage import guard (`usage/importer`), the two credential-recovery RAM queues (`drivers/oauth`, `local_tokens`), and test-only locks/counters. Everything else belongs to `AppContext` or to the object that uses it.

## The `Provider` trait

Every provider implements `crates/spanreed-adapters/src/providers/mod.rs::Provider`:

```rust
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;   // stable CLI/API id, e.g. "claude"
    fn name(&self) -> &'static str; // display name, e.g. "Claude"
    fn detect(&self) -> bool;       // any local signal on this machine?
    fn probe(&self, ports: ProbePorts<'_>) -> ProviderOutput; // fetch usage; never panic
}
```

`ports.cost` supplies local-log and capture-ledger cost lines (priced with the
context's catalog); providers that do not show cost ignore `ports`.

Rules:

- `detect()` must be cheap and side-effect-free (check a file/env/process). It
  decides whether the provider shows up in `spanreed probe` (no id).
- `probe()` must **never panic**. On any failure return
  `ProviderOutput::error(ID, NAME, "message")`, which renders as a red `Error`
  badge. User-facing messages should say how to fix it
  (e.g. "Run `claude` to log in again.").
- OAuth providers should refresh near expiry and persist the refreshed token
  back to the same source they read it from.

## The output contract (`spanreed_domain::model`)

A `probe()` returns a `ProviderOutput` (`provider_id`, `display_name`, optional
`plan`, and `lines`). Build lines with the `MetricLine` constructors:

| Constructor | Renders as |
|-------------|------------|
| `MetricLine::percent(label, used, resets_at)` | progress bar, 0–100% |
| `MetricLine::dollars(label, used, limit, resets_at)` | progress bar, `$used / $limit` |
| `MetricLine::text(label, value)` | plain `label: value` |
| `MetricLine::error(text)` / `ProviderOutput::error(...)` | red `Error` badge |
| `MetricLine::Progress { format: ProgressFormat::Count { suffix }, .. }` | `used/limit <suffix>` |
| `MetricLine::Badge { label, text, color, .. }` | colored badge |

`ProgressFormat` is `Percent` | `Dollars` | `Count { suffix }`. The Waybar
"primary" metric is the highest-utilization progress line across all providers.

## Credential helpers (`creds.rs`)

Prefer these over hand-rolling path/IO logic:

- `expand("~/...")`, `config_home()`, `data_home()` — XDG-aware paths.
- `read_file(path)`, `read_json(path)`, `first_existing(&[paths])`.
- `sqlite_query_one(db, sql, params)`, `sqlite_query_rows_i64_f64(db, sql)` —
  read-only SQLite (handles WAL + immutable fallback).
- `secret_tool_lookup(&[("service", "...")])` — Secret Service via `secret-tool`.
- `env("VAR")` — trimmed, non-empty env var.
- `find_processes(&["name", "marker"])`, `extract_flag(cmdline, "--flag")`,
  `listening_ports(pid)` — local language-server discovery via `/proc`.

## HTTP (`http.rs`)

```rust
let resp = Request::get(url)            // or ::post(url)
    .bearer(&token)                     // or .header("k", "v")
    .header("Accept", "application/json")
    .body(payload)                      // POST body
    .insecure()                         // accept self-signed (local LS only)
    .send()?;                           // -> Result<Response, String>
if resp.is_auth_error() { /* 401/403 */ }
let json = resp.json();                 // Option<serde_json::Value>
```

Proxy comes from `~/.config/spanreed/config.json` automatically; localhost is
bypassed.

## Adding a provider (quick version)

1. Create `crates/spanreed-adapters/src/providers/<id>.rs`; implement `Provider`. Use `zai.rs` (env-key,
   simplest) or `codex.rs` (OAuth file + refresh) as templates.
2. Register it in `crates/spanreed-adapters/src/providers/mod.rs`: add `pub mod <id>;` and a
   `Box::new(<id>::Type)` entry in `all()`.
3. `cargo build` then `spanreed probe <id>` to test against your account.

See `CONTRIBUTING.md` for the full walkthrough.

## Key commands

```sh
cargo build                 # debug build
cargo build --release       # release binary at target/release/spanreed
cargo test                  # every crate's unit tests + tests/cli.rs, tests/agent_host.rs
cargo fmt && cargo clippy   # format + lint before committing
spanreed list              # providers + detection state
spanreed probe <id>        # force-probe one provider
```

## Testing

Logic is split so it's testable without network or real credentials:

- Each provider keeps a pure `parse_*(json) -> Vec<MetricLine>` function tested
  with the documented sample response; `probe()` only does the IO around it.
- Unit tests live in `#[cfg(test)] mod tests` in each file (`util`, `model`,
  `output`, `creds` (temp SQLite DB), `pricing`, `cost` (fixture log lines →
  exact cost + dedup), and every provider parser).
- The agent-host controller tests (`desktop_runtime/agent.rs`) build and run the
  `fabrials-agent-host` `fake_agent` example; they never use the real `grok` CLI.
- `tests/cli.rs` runs the built binary in an isolated `HOME`/XDG for `list` /
  `json` / `waybar` / `help`, and asserts the SIGPIPE fix (no panic on a closed
  pipe). Keep it std-only (no extra dev-deps).
- Pricing layers (later wins): the embedded `fabrials-pricing` snapshot and overlays
  (`vendor/fabrials-libs/crates/fabrials-pricing/src/pricing-data.json`, `pricing-overlays.json`) → remote cache
  `~/.cache/spanreed/pricing-remote.json` (refreshed at most weekly by
  `pricing::ensure_fresh()` through `Catalog::refresh`, silent on failure, disabled by
  `SPANREED_OFFLINE`) → user `~/.config/spanreed/pricing.json`. `AppContext` owns the
  `Catalog`; tables change only on `refresh`/`reload_pricing`. The refresh
  merges LiteLLM's `model_prices_and_context_window.json` with the OpenCode Go
  channel from `https://models.dev/api.json` (LiteLLM wins ids it already
  prices). The same fetch writes context windows to
  `~/.cache/spanreed/limits-remote.json`, overridable with
  `~/.config/spanreed/limits.json`. New models are priced without a new binary.
- Grok capture costs use that table (public API list: grok-4.5 $2/$0.30
  cached/$6 per MTok, ×2 above 200k prompt — same as xAI docs and OpenRouter
  `x-ai/grok-4.5`). SuperGrok `cost_in_usd_ticks` are stored but not shown.
- The embedded snapshot is the offline fallback; refresh it occasionally with
  `spanreed update-pricing crates/fabrials-pricing/src/pricing-data.json` in
  `~/dev/fabrials/libs/fabrials-libs` (same compose as the runtime refresh, so the
  snapshot picks up the OpenCode Go channel), commit it there and sync the vendor
  copy (no build-time network — Nix-sandbox safe). `tests/cli.rs` sets
  `SPANREED_OFFLINE=1` so tests/CI never fetch.

## Conventions

- No comments unless they explain non-obvious intent.
- `probe()` never panics; all errors become badge lines.
- Match each provider's real API field names and shapes faithfully; these APIs
  are undocumented and reverse-engineered, so be precise.
- Keep messages actionable and never log raw tokens.

## Local quality gates

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
nix develop --command bash -c 'SPANREED_OFFLINE=1 cargo clippy --all-targets --features tray -- -D warnings'
SPANREED_OFFLINE=1 cargo test
(cd desktop/src-tauri && cargo check --locked)   # needs GTK/WebKit: inside `nix develop`
```

Conventional Commits (`feat:` / `fix:` / `docs:` / …). Subjects: no version
numbers and no PR ids.

## Validation status

Only `claude`, `codex`, `grok`, `copilot`, and `nous` have been validated against live
APIs. Nous validation covers OAuth, quota and model discovery, without paid inference.
The other providers are implemented to the documented API shapes but are
not yet confirmed against real accounts — treat field parsing as unverified
until someone runs `spanreed probe <id>` against a live account.

## Branch model (Model A)

Remote: **https://github.com/grok-insider/spanreed** (public).

| Branch | Role |
|--------|------|
| `dev` | Integration line (`feat/*` → PR → `dev`). May sit ahead of the last tag. |
| `master` | Last release only. PRs from `release-plz-*` / `release-plz-manual-*` (not a naked `dev` head). |

Local gate before any PR:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
SPANREED_OFFLINE=1 cargo test --all
```

### Releases

- **One merge to `master` = one release.** Do not land bare `dev` on `master`.
- **Patch (Z):** push to `dev` (or workflow_dispatch) opens `release-plz-vX.Y.Z`
  **from `dev`** (bump + deterministic changelog). Merge that PR → tag `vX.Y.Z` + GH Release
  + binaries.
- **Minor / major (Y / X):** Actions → **Manual Version Bump** from `dev`
  (`release-plz-manual-v…` → `master`). Both flows use `scripts/release.py`.
- Do **not** hand-edit `CHANGELOG.md` outside a Release PR.
- No crates.io publication. Release orchestration uses `gh` with `GITHUB_TOKEN`; no personal token fallback.
- Publish from the exact release merge SHA on `master`, after all four builds pass. Keep the release draft until all eight assets and checksums verify.
- Operations and recovery: `docs/release-automation.md`.

### Distribution (install vs binaries vs share)

| Surface | Role |
|---------|------|
| **GitHub flake (stable)** | Linux gnu+tray: pin the **release tag** — `nix run` / `nix profile install github:grok-insider/spanreed/vX.Y.Z` and `homeManagerModules.default`. The tag is created **with** the GitHub Release. Floating `github:grok-insider/spanreed` follows `master` and is **not** stable. Org rule: [`../AGENTS.md`](../AGENTS.md). |
| **GitHub Releases** | Binaries + `.sha256` + release notes only. **No** `install.sh` / `install.ps1` assets. |
| **fabrials.com** | Canonical install UX and one-liners (`/install/spanreed.sh` · `.ps1`). |
| **`scripts/install.*` in this repo** | Dev/`--from-path` and source of truth copied into the web `public/install/` tree. |
| **fabrials.com/api/spanreed** | Opt-in **authenticated share** (`share login` + Bearer `POST /v1/usage/snapshots`) into the public Usage AI pool. Not install hosting. |

Do not re-attach install scripts to GH Releases. Keep public one-liners pointing at
the website.

### Share → public plan pool (X-linked)

- **Requires Grok Insider account** (Sign in with X once via device flow).
- **Login:** `spanreed share login` → browser `/spanreed/link` → approve.
- **Opt-in:** `spanreed setup` can enable **once-per-day** auto-share only after an affirmative choice; `--yes` keeps sharing and client wiring disabled. `spanreed privacy metrics on` explicitly enables publication consent. The schedule uses
  (`crates/spanreed-adapters/src/setup/share_schedule.rs`): preferred evening timer (**23:00 Europe/Madrid** /
  local) **plus** login / missed-run catch-up (systemd `Persistent` + login
  oneshot; macOS `RunAtLoad`; Windows `StartWhenAvailable` + logon).
- **Due-gate:** client records `last_share_day` (product day UTC+1 Madrid);
  server **upserts** same day. Manual: `spanreed share` (`--force` retries).
- Optional `SPANREED_API_BASE`. Payload: quota/plan/cost/error + economics
  (`crates/spanreed-adapters/src/share.rs`). Vote = server `user_id`; install id is device-only.
- Site `/spanreed` is **metrics only** (public summary). Link page for CLI.
- Workspace notes: `grok-insider/docs/spanreed.md`.
