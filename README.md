# spanreed

Track AI coding subscription usage from the terminal.

One Rust binary (`spanreed`) reads credentials that your local AI CLIs already
store, probes each provider’s usage API, and prints:

- plain text for the terminal
- JSON for status bars (e.g. Waybar)
- a small local HTTP API for dashboards

Linux-first (Hyprland/Wayland); the same code also builds for macOS and Windows.

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
spanreed probe                 # probe every detected provider
spanreed probe claude          # force one provider
spanreed waybar                # one-shot status-bar JSON
spanreed json                  # raw JSON
spanreed serve [--interval S]  # HTTP API on 127.0.0.1:6736
spanreed auth copilot          # opt-in GitHub token for Copilot
spanreed help
```

With no arguments, `spanreed` runs `probe`.

### Example

```
$ spanreed probe
Claude (Max 20x)
  Session: 15% · resets in 4h 47m
  Weekly: 22% · resets in 3d 1h
  …

Codex (Free)
  Session: 5% · resets in 29d 23h
  …
```

## How it works

1. **Detect** — look for local signals (credential file, env, SQLite state, process).
2. **Probe** — query each provider’s usage endpoint (refresh OAuth when needed).
3. **Render** — plain text, Waybar JSON, raw JSON, or the local HTTP API.

For Claude and Codex, spanreed can also estimate spend from **local session
logs** (not your invoice). Details: cost/history behaviour is documented in the
binary help and source; set `SPANREED_OFFLINE=1` to skip remote price-table
refresh.

## Providers

| Id | Credential source (typical) |
|----|-----------------------------|
| `claude` | `~/.claude/` / `$CLAUDE_CONFIG_DIR` |
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
