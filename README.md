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

Snapshot from this machine (`spanreed probe`):

```
$ spanreed probe
Claude (Max 20x)
  Session: 4% · resets in 4h 39m
  Weekly: 1% · resets in 6d 22h
  Plan renews: 2026-08-17 · in 18d 1h · est.
  Last renew: 2026-07-17 · est.
  Last 30 Days: ~$2017.98 · 1.9B tokens (estimated)
  Since weekly reset: 27K tokens · ~$0.01
  Models: claude-fable-5 1.2B · claude-opus-4-8 710M · claude-haiku-4-5 906K
  Cache: 100% of input (read 1.8B · create 40M)
  Usage Trend: ▁▁▁▁▁▂▂▂▁▂▂▁▁█▁▁▁▁▁▁▁▁

Codex (Pro)
  Session: 4% · resets in 5d 21h
  Last 30 Days: ~$95669.31 · 155B tokens (partial, estimated)
  Models: gpt-5.6-sol 148B · unknown 6.3B · gpt-5.5 163M
  Cache: 98% of input (read 151B)
  Usage Trend: ▁▁▁▁▁▂▄▃▁▁█▆

Grok (SuperGrok Heavy)
  Weekly: 51% · resets in 15h 34m
  Build: 48% · resets in 15h 34m
  Chat: 2% · resets in 15h 34m
  Api: 1% · resets in 15h 34m
  Pay as you go: Disabled
  Plan renews: 2026-08-21 · in 21d 22h
  Last renew: 2026-07-21
  Last 30 Days: $96.3509 · 18M tokens
  Since weekly reset: 18M tokens · ~$96.35
  Models: grok-4.5-build 18M
  Cache: 96% of input (read 18M)
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
