# spanreed

Linux-native AI coding subscription usage tracker. A single Rust binary that
reads your local AI CLI credentials, queries each provider's usage API, and
prints the result for the terminal, a Waybar/EWW module, or a local HTTP API.

No webview, no tray dependency, no Electron — just a fast CLI/daemon that fits a
Wayland/Hyprland setup.

This is a Linux-native re-implementation inspired by
[spanreed](https://github.com/robinebers/spanreed) (macOS, Tauri). It shares
no code; providers are written natively in Rust and credentials are read the
Linux way (XDG paths, plaintext credential files, SQLite state DBs, and the
Secret Service via `secret-tool`).

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

## Providers

| Provider                 | Source on Linux                                                  |
|--------------------------|-----------------------------------------------------------------|
| `claude`                 | `~/.claude/.credentials.json` (or `$CLAUDE_CONFIG_DIR`)          |
| `codex`                  | `$CODEX_HOME` / `~/.config/codex` / `~/.codex` `auth.json`       |
| `cursor`                 | `~/.config/Cursor/.../state.vscdb` (SQLite)                      |
| `grok`                   | `~/.grok/auth.json` (Grok CLI / SuperGrok Build)                 |
| `opencode-go`            | `~/.local/share/opencode/opencode.db` (SQLite)                  |
| `amp`                    | `~/.local/share/amp/secrets.json`                               |
| `zai`                    | `ZAI_API_KEY` / `GLM_API_KEY` env                               |
| `minimax`                | `MINIMAX_API_KEY` / `MINIMAX_CN_API_KEY` env (region-aware)     |
| `synthetic`              | `~/.pi`, Factory, OpenCode auth, or `SYNTHETIC_API_KEY` env     |
| `kimi`                   | `~/.kimi/credentials/kimi-code.json`                           |
| `copilot`                | `gh auth token` / Secret Service / `~/.config/gh/hosts.yml`     |
| `factory`                | `~/.factory/auth.json` (plain JSON; encrypted variants n/a)     |
| `devin`                  | `~/.local/share/devin/credentials.toml`                        |
| `jetbrains-ai-assistant` | `~/.config/JetBrains/<IDE>/options/AIAssistantQuotaManager2.xml`|
| `kiro`                   | `~/.config/Kiro/.../state.vscdb` + `~/.aws/sso/cache`           |
| `antigravity`            | local language server (discovered via `/proc`)                  |
| `perplexity`             | macOS-only desktop cache — not available on Linux               |

Each OAuth provider auto-refreshes short-lived tokens when needed and persists
the refreshed token back to its source. Credentials are read the Linux way:
XDG paths, plaintext credential files, SQLite state DBs, the GitHub CLI, and the
Secret Service via `secret-tool`.

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

## Local HTTP API

`spanreed serve` exposes:

- `GET /usage` — JSON array of the latest provider outputs
- `GET /health` — `{"status":"ok"}`

```sh
spanreed serve --interval 300 &
curl -s http://127.0.0.1:6736/usage | jq
```

The daemon re-probes on the given interval (seconds, min 30) and serves the
cached result.

## Waybar

`spanreed waybar` prints `{text, tooltip, class, percentage}`. The `text` is the
single highest-utilization metric across providers; `class` is `ok`/`warning`/
`critical` for CSS styling. Add to `~/.config/waybar/config`:

```jsonc
"custom/spanreed": {
  "exec": "spanreed waybar",
  "return-type": "json",
  "interval": 300,
  "tooltip": true,
  "on-click": "spanreed probe | foot --hold spanreed probe"
}
```

And style it in `style.css`:

```css
#custom-spanreed.ok       { color: #a6e3a1; }
#custom-spanreed.warning  { color: #f9e2af; }
#custom-spanreed.critical { color: #f38ba8; }
```

> Tip: instead of `interval`, run `spanreed serve` once and have the Waybar
> module curl the local API for a near-instant refresh.

## Proxy

Optional. Create `~/.config/spanreed/config.json`:

```json
{ "proxy": { "enabled": true, "url": "socks5://127.0.0.1:9050" } }
```

`localhost` is always bypassed.

## Build

```sh
cargo build --release
# binary: ./target/release/spanreed
```

Requires the `secret-tool` binary (from `libsecret`) only if a provider stores
its token in the Secret Service rather than a file.

## License

MIT.
