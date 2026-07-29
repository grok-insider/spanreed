# spanreed

[![CI](https://github.com/grok-insider/spanreed/actions/workflows/ci.yml/badge.svg)](https://github.com/grok-insider/spanreed/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/grok-insider/spanreed?sort=semver)](https://github.com/grok-insider/spanreed/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platforms-Linux%20%C2%B7%20macOS%20%C2%B7%20Windows-blue)

A cross-platform AI coding subscription usage tracker (Linux-first). A single Rust
binary that reads your local AI CLI credentials, queries each provider's usage
API, and prints the result for the terminal, a status bar (Waybar/EWW), or a local
HTTP API.

No webview, no tray dependency, no Electron — just a fast CLI/daemon that fits a
Wayland/Hyprland setup. Credentials are read from where each CLI already stores
them: XDG paths and plaintext files, SQLite state DBs, the GitHub CLI, and the OS
secret store (Secret Service via `secret-tool` on Linux, Keychain on macOS,
Credential Manager on Windows).

```
$ spanreed probe
Claude (Max 20x)
  Session: 15% · resets in 4h 47m
  Weekly: 22% · resets in 3d 1h
  Sonnet: 0% · resets in 3d 1h
  Last 30 Days: ~$681.84 · 1.1B tokens
  Usage Trend: ▁▁▁▁▁▁▁▁▁▁▁▁▁▁▅█▂▁

Codex (Free)
  Session: 5% · resets in 29d 23h
  Last 30 Days: ~$2413.06 · 3.4B tokens
  Usage Trend: ▁▃▃▁█▆▃▁▂▁▃▁█▅▁

Grok (SuperGrok Heavy)
  Weekly: 1% · resets in 2d 2h
  Pay as you go: Disabled

Copilot (Individual)
  Premium: 0%
  Chat: 0%
  Completions: 0%
```

Progress lines show when each window resets (`· resets in 3h 12m`) whenever the
provider reports a reset time. The window length depends on your plan/model
(5-hour, weekly, monthly, ...), so the countdown reflects whichever limit that
line tracks.

## How it works

1. **Detect** — each provider checks for a local signal (a credential file, an
   env var, a SQLite state DB, a running process). Providers with no signal stay
   hidden.
2. **Probe** — detected providers run concurrently. Each reads its local
   credentials, refreshes short-lived OAuth tokens when needed (persisting the
   new token back to its source), and calls the provider's usage endpoint.
3. **Render** — results are emitted as human text, Waybar JSON, or raw JSON, and
   optionally cached behind a local HTTP API.

## Cost estimation

For **Claude** and **Codex**, spanreed estimates spend from the CLIs' local
session logs (Claude: `$CLAUDE_CONFIG_DIR/projects`, `~/.config/claude/projects`,
`~/.claude/projects`; Codex: `~/.codex/sessions`) — no API needed. It prices each
message's token usage against a model price table built from
[LiteLLM](https://github.com/BerriAI/litellm)'s pricing data — an embedded
snapshot for offline use, refreshed from upstream at most once a week (cached in
`~/.cache/spanreed/`) so newly released models are priced without updating the
binary. Set `SPANREED_OFFLINE=1` to skip the weekly remote refresh (the embedded
snapshot and any existing cache still price). The result is a `Last 30 Days` total
and a `Usage Trend` daily sparkline.

It's built to be cheap on repeated runs: append-only logs older than the window
are skipped by mtime, only token-bearing lines are parsed (via a `memchr`
pre-filter), files are read in parallel, duplicate messages are de-duped, and
the 30-day aggregate is cached for a few minutes.

Besides the total, probe also shows a local breakdown when the logs carry it:

- **Models** — top models by token volume over the same 30-day window (e.g.
  `Models: claude-opus-4 1.1B · claude-sonnet-4 0.7B · (+1)`).
- **Cache** — prompt-cache hit rate from `cache_read` vs uncached input (e.g.
  `Cache: 71% of input (read 1.2B · create 40M)`). Claude reports cache create
  and read; Codex/Grok typically only report cached input reads.

These are **observed** from local session logs (or Grok capture), not the
subscription pool size. The official Session/Weekly **%** lines remain the
source of truth for rate limits.

When the provider exposes a weekly epoch boundary, probe also shows
**Since weekly reset** — local tokens/cost since that epoch started (Codex:
`reset_at − limit_window_seconds`, so a mid-cycle force-reset restarts the
cutoff; Grok: `currentPeriod.start`; Claude: estimated from `resets_at − 7d`).

Figures are **estimates** (prefixed `~$`) and a lower bound when a model is
missing from the price table (shown as `(partial)`). Override or extend prices
with `~/.config/spanreed/pricing.json` (same shape as the LiteLLM data, e.g.
`{ "my-model": { "input_cost_per_token": 1e-6, "output_cost_per_token": 5e-6 } }`).

Other providers surface dollar figures wherever their API returns them (Cursor
credits/on-demand, Amp balance, Claude extra usage, OpenCode Go local spend).

### Grok (SuperGrok) Last 30 Days

Grok's subscription endpoint only exposes a **weekly usage pool** (and optional
per-product %). It does **not** write per-call `input_tokens` / `output_tokens`
into `~/.grok/sessions/` — so spanreed never invents token totals from
context-size telemetry (that double-counts badly).

For accurate **Last 30 Days** token/cost lines, run the dual capture service
(records official API `usage` into `~/.local/share/spanreed/grok-usage.jsonl`):

```sh
spanreed capture serve
#   127.0.0.1:18736 → cli-chat-proxy.grok.com  (Grok CLI)
#   127.0.0.1:18737 → api.x.ai                 (OpenCode xAI)
```

Wire clients **once** (not per invocation):

| Client | Setting |
|--------|---------|
| Grok CLI | `GROK_CLI_CHAT_PROXY_BASE_URL=http://127.0.0.1:18736/v1` in your wrapper / session env |
| OpenCode | `provider.xai.options.baseURL`: `http://127.0.0.1:18737/v1` in `opencode.json` |

Home Manager: `programs.spanreed.capture.enable = true` (optional
`egressProxy = "http://127.0.0.1:7897"` so upstream still uses a geo VPN).
Until capture has seen traffic, probe shows an enable hint instead of a fake
total. Weekly pool % works without capture.

## Install

### Prebuilt binaries

Each [GitHub Release](https://github.com/grok-insider/spanreed/releases) attaches
a prebuilt `spanreed` for **Linux** (x86_64/aarch64, static musl), **macOS**
(x86_64/arm64), and **Windows** (x86_64) — `tar.gz` on Unix, `zip` on Windows,
each with a `.sha256`. Download the archive for your platform, verify, extract,
and put `spanreed` on your `PATH`:

```sh
sha256sum -c spanreed-*-x86_64-unknown-linux-musl.tar.gz.sha256
tar -xzf spanreed-*-x86_64-unknown-linux-musl.tar.gz
install -Dm755 spanreed ~/.local/bin/spanreed
```

### Nix / NixOS

spanreed ships as a Nix flake with prebuilt closures on a public
[Cachix](https://cachix.org) cache, so installing does **not** require building
Rust locally. The Nix package targets `x86_64-linux` and `aarch64-linux`.

Trust the binary cache once (otherwise Nix rebuilds from source):

```nix
# NixOS / nix.conf
nix.settings = {
  substituters = [ "https://grok-insider.cachix.org" ];
  trusted-public-keys = [
    "grok-insider.cachix.org-1:ZxLVOxJ1CjdY3vQl1I99qCtwNZwIU4+/QwqSvntB/5w="
  ];
};
```

Or, with the Cachix CLI: `cachix use grok-insider`. Then run or install:

```sh
nix run github:grok-insider/spanreed -- probe     # try it without installing
nix profile install github:grok-insider/spanreed
```

The flake also advertises the cache via `nixConfig.extra-substituters`, so
`nix run github:grok-insider/spanreed` offers to use it (accept the prompt or pass
`--accept-flake-config`).

#### Home Manager

```nix
{
  inputs.spanreed.url = "github:grok-insider/spanreed";

  # in your home-manager config:
  imports = [ inputs.spanreed.homeManagerModules.default ];

  programs.spanreed = {
    enable = true;
    serve.enable = true;     # run `spanreed serve` as a user service
    serve.interval = 300;    # refresh seconds (min 30)
  };
}
```

### From source

```sh
cargo build --release
# binary: ./target/release/spanreed
```

On Linux, `secret-tool` (from `libsecret`) is only needed if a provider stores its
token in the Secret Service rather than a file.

## Usage

```sh
spanreed list                 # show providers and whether they're detected
spanreed probe                # probe every detected provider (human output)
spanreed probe claude         # probe a single provider (forces it)
spanreed probe --force        # probe ALL providers, detected or not
spanreed waybar               # emit Waybar custom-module JSON (one shot)
spanreed json                 # raw JSON of all detected provider outputs
spanreed serve [--interval S] # run the local HTTP API on 127.0.0.1:6736
spanreed auth copilot         # opt-in: link a GitHub token for Copilot
spanreed auth logout copilot  # forget the stored Copilot credential
spanreed update-pricing [out] # fetch + filter the LiteLLM price table (advanced)
```

With no arguments, `spanreed` runs `probe`. `probe` without an id only runs
providers it detects on this machine; pass an id (e.g. `spanreed probe cursor`)
to force a specific one, or `--force` to run them all. `update-pricing` is a
maintenance command used to refresh the embedded `src/pricing-data.json`.

### Copilot (opt-in)

Copilot is **not** auto-detected from `gh`. Being logged into GitHub is not the
same as having Copilot, and multi-account `gh` setups often have the wrong
account active. Link once:

```sh
spanreed auth copilot                 # interactive: pick a gh user or paste a token
spanreed auth copilot --user 0xfell   # import that gh account's token
spanreed auth copilot --token-stdin   # non-interactive token on stdin
# or: SPANREED_GITHUB_TOKEN=… / GH_TOKEN=… spanreed auth copilot
spanreed auth logout copilot
```

Auth validates against the Copilot usage API (rejects `no_access`) and stores the
token under spanreed's own keyring item (`spanreed:copilot`) plus a mode-0600
fallback file at `~/.config/spanreed/copilot.token`.

## Providers

| Provider                 | Credential source                                                | Verified |
|--------------------------|-----------------------------------------------------------------|----------|
| `claude`                 | `~/.claude/.credentials.json` (or `$CLAUDE_CONFIG_DIR`; Keychain on macOS) | live     |
| `codex`                  | `$CODEX_HOME` / `~/.config/codex` / `~/.codex` `auth.json`       | live     |
| `grok`                   | `~/.grok/auth.json` (weekly SuperGrok pool); optional local capture ledger for Last 30 Days | live |
| `copilot`                | **opt-in** via `spanreed auth copilot` (own keyring/file token) | live     |
| `cursor`                 | `~/.config/Cursor/.../state.vscdb` (SQLite)                      | code     |
| `opencode-go`            | `~/.local/share/opencode/opencode.db` (SQLite)                 | code     |
| `amp`                    | `~/.local/share/amp/secrets.json`                               | code     |
| `zai`                    | `ZAI_API_KEY` / `GLM_API_KEY` env                               | code     |
| `minimax`                | `MINIMAX_API_KEY` / `MINIMAX_CN_API_KEY` env (region-aware)     | code     |
| `synthetic`              | `~/.pi`, Factory, OpenCode auth, or `SYNTHETIC_API_KEY` env     | code     |
| `kimi`                   | `~/.kimi/credentials/kimi-code.json`                           | code     |
| `factory`                | `~/.factory/auth.json` (plain JSON; encrypted variants n/a)     | code     |
| `devin`                  | `~/.local/share/devin/credentials.toml`                        | code     |
| `jetbrains-ai-assistant` | `~/.config/JetBrains/<IDE>/options/AIAssistantQuotaManager2.xml`| code     |
| `kiro`                   | `~/.config/Kiro/.../state.vscdb` + `~/.aws/sso/cache`           | code     |
| `antigravity`            | local language server (discovered via `/proc`)                  | code     |
| `perplexity`             | macOS desktop-app cache (currently inactive)                    | n/a      |

**Verified** column: `live` = validated against the real API; `code` =
implemented to the documented API shape but not yet confirmed against a live
account; `n/a` = no usable data source yet. If a `code` provider misreports, open
an issue with its `spanreed probe <id>` output.

## Local HTTP API

`spanreed serve` exposes:

- `GET /usage` (also `GET /`) — JSON array of the latest provider outputs
- `GET /health` — `{"status":"ok"}`

```sh
spanreed serve --interval 300 &
curl -s http://127.0.0.1:6736/usage | jq
```

The daemon re-probes on the given interval (seconds, min 30) and serves the
cached result, so readers (status bars, scripts) get an instant, non-blocking
response. It binds to `127.0.0.1` only.

## Waybar

`spanreed waybar` prints `{text, tooltip, class, percentage}`. The `text` is the
**last-used** **paid** Claude/Codex/Grok plan (cheap local activity signals:
Claude/Codex `history.jsonl` mtime, Grok `active_sessions.json` mtime — a few
`stat`s, no session-tree walk). For that provider, the shown window is Session
(5h), escalating to Weekly once Weekly crosses 80%. If no activity signal is
available, the bar falls back to the highest-utilization eligible provider.
Other providers (Copilot, Cursor, …) and free/guest plans still appear in the
tooltip but never drive the bar text. `class` is `ok` / `warning` / `critical`
(≥80% → warning, ≥95% → critical) for CSS styling; it shows `no data` when nothing
is eligible.

```jsonc
// ~/.config/waybar/config
"custom/spanreed": {
  "exec": "spanreed waybar",
  "return-type": "json",
  "interval": 300,
  "tooltip": true,
  "on-click": "ghostty -e sh -c 'spanreed probe; read -n 1'"
}
```

```css
/* style.css */
#custom-spanreed.ok       { color: #a6e3a1; }
#custom-spanreed.warning  { color: #f9e2af; }
#custom-spanreed.critical { color: #f38ba8; }
```

> Tip: instead of polling with `interval`, run `spanreed serve` once and have a
> small wrapper curl `127.0.0.1:6736/usage` for a near-instant refresh. The same
> JSON drives an EWW module.

## Proxy

Optional. Create `~/.config/spanreed/config.json`:

```json
{ "proxy": { "enabled": true, "url": "socks5://127.0.0.1:9050" } }
```

`localhost` is always bypassed.

## Security

- spanreed reads **local** credential files and state DBs that the AI CLIs
  themselves write. It never copies, uploads, or transmits those credentials
  anywhere except, as a bearer token, to **that provider's own HTTPS endpoint**
  — exactly as the official CLI would.
- Refreshed OAuth tokens are written back only to the same local source they
  were read from.
- The local HTTP API binds to `127.0.0.1` only.
- No telemetry, no analytics, no phone-home.

## Caveats

- **Unofficial / unaffiliated.** spanreed is an independent tool and is not
  endorsed by or affiliated with Anthropic, OpenAI, GitHub, xAI, Google, or any
  other provider.
- **Reverse-engineered APIs.** Most provider usage endpoints are undocumented
  and used internally by their own apps. They can change or break at any time,
  which will surface as an error line for that provider until the parser is
  updated.
- **Platform-specific providers.** `perplexity` targets a macOS desktop-app cache
  and currently never activates. Most other sources are Linux/XDG paths; on macOS
  and Windows, credentials come from the OS secret store where applicable.

## Architecture & contributing

The codebase is small and modular — one file per concern under `src/`, one file
per provider under `src/providers/`. See [`AGENTS.md`](AGENTS.md) for the module
map and the `Provider` trait contract, and [`CONTRIBUTING.md`](CONTRIBUTING.md)
for a step-by-step guide to adding a provider.

## License

[MIT](LICENSE) © 2026 Grok Insider.
