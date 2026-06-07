# spanreed

Linux-native AI coding subscription usage tracker. A single Rust binary that
reads your local AI CLI credentials, queries each provider's usage API, and
prints the result for the terminal, a Waybar/EWW module, or a local HTTP API.

No webview, no tray dependency, no Electron — just a fast CLI/daemon that fits a
Wayland/Hyprland setup. Credentials are read the Linux way: XDG paths, plaintext
credential files, SQLite state DBs, the GitHub CLI, and the Secret Service via
`secret-tool`.

```
$ spanreed probe
Claude (Max 20x)
  Session: 15%
  Weekly: 22%
  Sonnet: 0%

Codex (Free)
  Session: 5%

Grok (SuperGrok Heavy)
  Credits used: 0%
  Pay as you go: Disabled

Copilot (Individual)
  Premium: 0%
  Chat: 0%
  Completions: 0%
```

## How it works

1. **Detect** — each provider checks for a local signal (a credential file, an
   env var, a SQLite state DB, a running process). Providers with no signal stay
   hidden.
2. **Probe** — detected providers run concurrently. Each reads its local
   credentials, refreshes short-lived OAuth tokens when needed (persisting the
   new token back to its source), and calls the provider's usage endpoint.
3. **Render** — results are emitted as human text, Waybar JSON, or raw JSON, and
   optionally cached behind a local HTTP API.

## Install (Nix / NixOS)

spanreed ships as a Nix flake with prebuilt closures on a public
[Cachix](https://cachix.org) cache, so installing does **not** require building
Rust locally. Prebuilt for `x86_64-linux` and `aarch64-linux`.

Trust the binary cache once (otherwise Nix rebuilds from source):

```nix
# NixOS / nix.conf
nix.settings = {
  substituters = [ "https://0xfell.cachix.org" ];
  trusted-public-keys = [
    "0xfell.cachix.org-1:0VSPKbe/Eilt+WTT/0faSQeQnnhDOH7PxkUvoRtvPPo="
  ];
};
```

Or, with the Cachix CLI: `cachix use 0xfell`.

Then run or install:

```sh
nix run github:0xfell/spanreed -- probe     # try it without installing
nix profile install github:0xfell/spanreed
```

The flake also advertises the cache via `nixConfig.extra-substituters`, so
`nix run github:0xfell/spanreed` offers to use it (accept the prompt or pass
`--accept-flake-config`).

### Home Manager

```nix
{
  inputs.spanreed.url = "github:0xfell/spanreed";

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

`secret-tool` (from `libsecret`) is only needed if a provider stores its token
in the Secret Service rather than a file.

## Usage

```sh
spanreed list                 # show providers and whether they're detected
spanreed probe                # probe every detected provider (human output)
spanreed probe claude         # probe a single provider (forces it)
spanreed waybar               # emit Waybar custom-module JSON (one shot)
spanreed json                 # raw JSON of all detected provider outputs
spanreed serve [--interval S] # run the local HTTP API on 127.0.0.1:6736
```

`probe` without an id only runs providers it detects on this machine. Pass an id
(e.g. `spanreed probe cursor`) to force a specific one even if undetected.

## Providers

| Provider                 | Source on Linux                                                  | Verified |
|--------------------------|-----------------------------------------------------------------|----------|
| `claude`                 | `~/.claude/.credentials.json` (or `$CLAUDE_CONFIG_DIR`)          | live     |
| `codex`                  | `$CODEX_HOME` / `~/.config/codex` / `~/.codex` `auth.json`       | live     |
| `grok`                   | `~/.grok/auth.json` (Grok CLI / SuperGrok Build)                 | live     |
| `copilot`                | `gh auth token` / Secret Service / `~/.config/gh/hosts.yml`     | live     |
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
| `perplexity`             | macOS-only desktop cache — not available on Linux               | n/a      |

**Verified** column: `live` = validated against the real API; `code` =
implemented to the documented API shape but not yet confirmed against a live
account; `n/a` = no Linux data source. If a `code` provider misreports, open an
issue with its `spanreed probe <id>` output.

## Local HTTP API

`spanreed serve` exposes:

- `GET /usage` — JSON array of the latest provider outputs
- `GET /health` — `{"status":"ok"}`

```sh
spanreed serve --interval 300 &
curl -s http://127.0.0.1:6736/usage | jq
```

The daemon re-probes on the given interval (seconds, min 30) and serves the
cached result, so readers (status bars, scripts) get an instant, non-blocking
response.

## Waybar

`spanreed waybar` prints `{text, tooltip, class, percentage}`. The `text` is the
single highest-utilization metric across providers; `class` is
`ok` / `warning` / `critical` (≥80% → warning, ≥95% → critical) for CSS styling.

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
> small wrapper curl `127.0.0.1:6736/usage` for a near-instant refresh.

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
- **macOS-only providers.** `perplexity` reads a macOS desktop-app cache that
  has no Linux equivalent, so it never activates on Linux.

## Architecture & contributing

The codebase is small and modular — one file per concern under `src/`, one file
per provider under `src/providers/`. See [`AGENTS.md`](AGENTS.md) for the module
map and the `Provider` trait contract, and [`CONTRIBUTING.md`](CONTRIBUTING.md)
for a step-by-step guide to adding a provider.

## License

[MIT](LICENSE) © 2026 0xfell.
