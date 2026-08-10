# spanreed

Track AI coding subscription usage from the terminal.

One Rust binary (`spanreed`) reads credentials that your local AI CLIs already
store, probes each provider’s usage API, and prints:

- plain text for the terminal
- JSON for status bars (e.g. Waybar)
- a small local HTTP API for dashboards

Linux-first (Hyprland/Wayland); the same code also builds for macOS and Windows.

## Install

### From a release (one command)

```sh
# Linux / macOS
curl -fsSL https://github.com/grok-insider/spanreed/releases/latest/download/install.sh | sh

# Windows (PowerShell)
irm https://github.com/grok-insider/spanreed/releases/latest/download/install.ps1 | iex
```

The installer downloads the binary, then runs **`spanreed setup`**, which asks:

- install CLI to user PATH  
- create the Grok capture ledger  
- start the capture proxy at login (optional)  
- wire **Grok Build** → `http://127.0.0.1:18736/v1`  
- wire **OpenCode xAI** → `http://127.0.0.1:18737/v1`  

### From a local build

```sh
cargo build --release

# Linux / macOS
./scripts/install.sh --from-path ./target/release/spanreed

# Windows
.\scripts\install.ps1 -FromPath .\target\release\spanreed.exe

# Or run setup on the binary you just built:
./target/release/spanreed setup
# non-interactive (no service unless --service):
./target/release/spanreed setup --yes --from-current-exe
```

Useful setup commands:

```sh
spanreed setup status
spanreed setup uninstall          # unwire + stop capture service
spanreed capture serve            # run capture in the foreground
spanreed capture serve --watchdog # auto-restart worker; log under spanreed/logs
spanreed capture ensure           # start capture+watchdog if ports 18736/18737 are down
spanreed capture status           # exit 0 if listening, 1 if DOWN; shows log path
spanreed probe grok --cost        # quotas + captured tokens / $ estimate
```

If Grok Build or OpenCode “stops working” while wired to the local proxy, check
capture first (`setup status` / `capture status`). A dead proxy with live wiring
looks like a CLI failure. Fix: `spanreed capture ensure`.

Capture logs (Windows): `%LOCALAPPDATA%\spanreed\logs\capture.log`.

## Build

```sh
cargo build --release
# binary: ./target/release/spanreed
```

With Nix:

```sh
nix build
# or, from a checkout:
nix run . -- probe
```

On Linux, `secret-tool` (libsecret) is only needed if a provider keeps its token
in the Secret Service instead of a file.

## Usage

```sh
spanreed list                  # which providers are detected
spanreed probe                 # quotas only (default)
spanreed probe --plan          # + plan renews / PAYG
spanreed probe --cost          # + Last 30 Days / spend
spanreed probe --models --cache --trend
spanreed probe --all           # full detail (same as before)
spanreed probe claude          # one provider
spanreed waybar                # one-shot status-bar JSON (full detail)
spanreed json                  # raw JSON (full detail, includes kind)
spanreed serve [--interval S]  # HTTP API on 127.0.0.1:6736
spanreed setup                 # install + optional capture service + wire clients
spanreed auth copilot          # opt-in GitHub token for Copilot
spanreed help
```

With no arguments, `spanreed` runs `probe`. Default probe shows **rate-limit
quotas only**; add flags for extra blocks. `json` / `waybar` / `serve` always
return the full metric set.

### Example

Snapshot from this machine (default `spanreed probe`):

```
$ spanreed probe
Claude (Max 20x)
  Session: 4% · resets in 4h 22m
  Weekly: 1% · resets in 6d 22h

Codex (Pro)
  Session: 4% · resets in 5d 21h

Grok (SuperGrok Heavy)
  Weekly: 52% · resets in 15h 17m
  Build: 49% · resets in 15h 17m
  Chat: 2% · resets in 15h 17m
  Api: 1% · resets in 15h 17m
```

## How it works

1. **Detect** — look for local signals (credential file, env, SQLite state, process).
2. **Probe** — query each provider’s usage endpoint (refresh OAuth when needed).
3. **Render** — plain text, Waybar JSON, raw JSON, or the local HTTP API.

For Codex (and Grok via capture ledger), spanreed can estimate spend from
**local logs** (not your invoice). Grok dollars use **public API list prices**
(e.g. grok-4.5: $2 / $0.30 cached / $6 per MTok, with xAI’s ≥200k long-context
tier) — not SuperGrok subscription-internal `cost_in_usd_ticks`. With `--cost`,
when a weekly pool % is available, it also projects **Expected this week /
month** using pool-% density (tokens per point of weekly usage), not only
wall-clock pace. Set `SPANREED_OFFLINE=1` to skip remote price-table refresh.

## Providers

| Id | Credential source (typical) |
|----|-----------------------------|
| `codex` | `~/.codex` / `$CODEX_HOME` |
| `grok` | `~/.grok/auth.json` (+ optional local capture) |
| `copilot` | opt-in via `spanreed auth copilot` |
| `cursor` | Cursor state DB under `~/.config/Cursor/` |
| `opencode-go` / `amp` / `zai` / `minimax` / … | see `spanreed list` and source |

## Config

Optional JSON at `~/.config/spanreed/config.json` (proxy, etc.).
Pricing overrides: `~/.config/spanreed/pricing.json`.

## License

MIT — see [LICENSE](LICENSE).
