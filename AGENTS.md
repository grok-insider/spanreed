# AGENTS.md

Instructions for AI agents and contributors working on spanreed.

## Project overview

spanreed is a cross-platform (Linux-first) AI coding subscription usage
tracker: one Rust binary (`spanreed`) that acts as a CLI, a background daemon,
and a data source for status bars. It reads local AI-CLI credentials, queries
each provider's usage API, and renders the result.

- Single crate, no workspace. Binary target `spanreed` (`src/main.rs`).
- No async runtime: probes are blocking I/O fanned out over threads.
- Providers are **native Rust** modules implementing one trait. There is no
  embedded scripting engine and no plugin sandbox.
- Credentials are read from where each CLI stores them: XDG paths, plaintext
  files, SQLite state DBs (`rusqlite`, read-only), the GitHub CLI, `/proc`, and
  the OS secret store — Secret Service via `secret-tool` on Linux, Keychain on
  macOS, Credential Manager on Windows. Linux/Wayland is the primary target; the
  same code compiles, tests, and ships binaries for macOS and Windows.

## Module layout

One file per concern. To add a top-level concern, add a `src/<name>.rs` and
declare it in `src/main.rs`.

| File | Owns |
|------|------|
| `src/main.rs`           | CLI entry + subcommand dispatch (`list`, `probe`, `waybar`, `json`, `serve`, `help`). |
| `src/probe.rs`          | Probe orchestration: runs detected (or all/one) providers concurrently. |
| `src/providers/mod.rs`  | The `Provider` trait, the `all()` registry, and `by_id()`. Register new providers here. |
| `src/providers/*.rs`    | One provider each (`claude`, `codex`, `grok`, ...). |
| `src/model.rs`          | Output contract: `MetricLine` (text/progress/badge), `ProgressFormat`, `ProviderOutput`. |
| `src/creds.rs`          | Linux credential & local-state discovery helpers (paths, files, SQLite, secret-tool, `/proc`). |
| `src/http.rs`           | Blocking HTTP client wrapper: `Request` builder, proxy support, optional insecure TLS. |
| `src/util.rs`           | Time (`now_ms`, `to_iso`, `ms_to_iso`, `local_date_ymd`), `plan_label`, `cents_to_dollars`, `fmt_tokens`, `jwt_payload`/`jwt_exp_ms`, base64. |
| `src/output.rs`         | Renderers: `plain` (terminal + sparkline), `waybar` (custom-module JSON), severity classes. |
| `src/api.rs`            | Local HTTP API on `127.0.0.1:6736` (`/usage`, `/health`) with background refresh. |
| `src/cost.rs`           | Local-log cost engine (Claude/Codex): parallel + `memchr` + mtime pre-filter + dedup + TTL cache; produces `Last 30 Days` + `Usage Trend`. |
| `src/grok_ledger.rs`    | Grok capture ledger (`grok-usage.jsonl`). Dollars = **public API list price** via `pricing` (not SuperGrok `cost_in_usd_ticks`); xAI all-or-nothing ≥200k long-context tier per request. |
| `src/setup/`            | `spanreed setup`: install binary to user PATH, ledger dir, optional capture user service, wire Grok Build + OpenCode xAI to the local capture proxy. |
| `src/capture_log.rs`    | Capture/watchdog log file (`…/spanreed/logs/capture.log`) with size rotation. |
| `src/capture_watchdog.rs` | `capture serve --watchdog`: restart worker when ports die / process exits. |
| `src/forecast.rs`       | Week/month Expected lines from pool-% density samples. |
| `src/pricing.rs`        | Model price table: embedded LiteLLM snapshot (`pricing-data.json`) + runtime-refreshed remote cache (7-day TTL) + user override; model-name matching and tiered cost math. |

## The `Provider` trait

Every provider implements `src/providers/mod.rs::Provider`:

```rust
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;   // stable CLI/API id, e.g. "claude"
    fn name(&self) -> &'static str; // display name, e.g. "Claude"
    fn detect(&self) -> bool;       // any local signal on this machine?
    fn probe(&self) -> ProviderOutput; // fetch usage; never panic
}
```

Rules:

- `detect()` must be cheap and side-effect-free (check a file/env/process). It
  decides whether the provider shows up in `spanreed probe` (no id).
- `probe()` must **never panic**. On any failure return
  `ProviderOutput::error(ID, NAME, "message")`, which renders as a red `Error`
  badge. User-facing messages should say how to fix it
  (e.g. "Run `claude` to log in again.").
- OAuth providers should refresh near expiry and persist the refreshed token
  back to the same source they read it from.

## The output contract (`model.rs`)

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

1. Create `src/providers/<id>.rs`; implement `Provider`. Use `zai.rs` (env-key,
   simplest) or `codex.rs` (OAuth file + refresh) as templates.
2. Register it in `src/providers/mod.rs`: add `pub mod <id>;` and a
   `Box::new(<id>::Type)` entry in `all()`.
3. `cargo build` then `spanreed probe <id>` to test against your account.

See `CONTRIBUTING.md` for the full walkthrough.

## Key commands

```sh
cargo build                 # debug build
cargo build --release       # release binary at target/release/spanreed
cargo test                  # unit tests (in-module) + tests/cli.rs integration
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
- `tests/cli.rs` runs the built binary in an isolated `HOME`/XDG for `list` /
  `json` / `waybar` / `help`, and asserts the SIGPIPE fix (no panic on a closed
  pipe). Keep it std-only (no extra dev-deps).
- Pricing layers (later wins): embedded `src/pricing-data.json` → remote cache
  `~/.cache/spanreed/pricing-remote.json` (refreshed from LiteLLM's
  `model_prices_and_context_window.json` at most weekly by
  `pricing::ensure_fresh()`, silent on failure, disabled by
  `SPANREED_OFFLINE`) → user `~/.config/spanreed/pricing.json`. New models
  are priced without a new binary.
- Grok capture costs use that table (public API list: grok-4.5 $2/$0.30
  cached/$6 per MTok, ×2 above 200k prompt — same as xAI docs and OpenRouter
  `x-ai/grok-4.5`). SuperGrok `cost_in_usd_ticks` are stored but not shown.
- The embedded snapshot is the offline fallback; refresh it occasionally with
  `spanreed update-pricing src/pricing-data.json` (same Rust filter as the
  runtime refresh) and commit the result (no build-time network — Nix-sandbox
  safe). `tests/cli.rs` sets `SPANREED_OFFLINE=1` so tests/CI never fetch.

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
SPANREED_OFFLINE=1 cargo test
```

Conventional Commits (`feat:` / `fix:` / `docs:` / …). Subjects: no version
numbers and no PR ids.

## Validation status

Only `claude`, `codex`, `grok`, and `copilot` have been validated against live
APIs. The other providers are implemented to the documented API shapes but are
not yet confirmed against real accounts — treat field parsing as unverified
until someone runs `spanreed probe <id>` against a live account.

## Branch model (Model A)

Remote: **https://github.com/grok-insider/spanreed** (public).

| Branch | Role |
|--------|------|
| `dev` | Integration line for human work (`feat/*` → PR → `dev`) |
| `master` | Ship line; only `dev` or release-bot heads (`release-plz-*`, `release-plz-manual-*`) |

Local gate before any PR:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
SPANREED_OFFLINE=1 cargo test --all
```

### Releases

- **Patch (Z):** automatic after a merge into `master` with `feat`/`fix` since the
  last tag — bot opens `release-plz-vX.Y.Z` (AI changelog via
  `release-changelog-action@v1`). Merge that PR to tag + attach binaries.
- **Minor / major (Y / X):** Actions → **Manual Version Bump** (repo admin only).
  Opens `release-plz-manual-v…` into `master`. A normal `dev` → `master` PR does
  **not** choose major/minor by itself.
- Do **not** hand-edit `CHANGELOG.md` outside a Release PR.
- No crates.io (`publish = false` only in `release-plz.toml`).

### Distribution (install vs binaries vs share)

| Surface | Role |
|---------|------|
| **GitHub Releases** | Binaries + `.sha256` + release notes only. **No** `install.sh` / `install.ps1` assets. |
| **grokinsider.net** | Canonical install UX and one-liners (`/install/spanreed.sh` · `.ps1`). |
| **`scripts/install.*` in this repo** | Dev/`--from-path` and source of truth copied into the web `public/install/` tree. |
| **api.grokinsider.net** | Opt-in **anonymous share** (`spanreed share` → `POST /v1/usage/snapshots`, no auth) into a public plan metric pool for **grokinsider.net/spanreed**. Not install hosting. |

Do not re-attach install scripts to GH Releases. Keep public one-liners pointing at
the website.

### Share → public plan pool (anonymous)

- **No login / no share token.** Contributions are not tied to a user.
- **Automatic:** `spanreed setup` enables **once-per-day** auto-share
  (`src/share_schedule.rs`): preferred evening timer (**23:00 Europe/Madrid** /
  local) **plus** login / missed-run catch-up (systemd `Persistent` + login
  oneshot; macOS `RunAtLoad`; Windows `StartWhenAvailable` + logon).
- **Due-gate:** client records `last_share_day` (product day UTC+1 Madrid);
  second trigger same day no-ops. Manual: `spanreed share` (`--force` retries).
- Optional `SPANREED_API_BASE`. Payload: quota/plan/cost/error + economics
  (`src/share.rs`). Multi-provider.
- Site `/spanreed` is **metrics only** (public summary). No install CTA.
- Workspace notes: `grok-insider/docs/spanreed.md`.
