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

## Providers

| Provider      | Source on Linux                                             | Status |
|---------------|------------------------------------------------------------|--------|
| `claude`      | `~/.claude/.credentials.json` (or `$CLAUDE_CONFIG_DIR`)     | works  |
| `codex`       | `$CODEX_HOME` / `~/.config/codex` / `~/.codex` `auth.json`  | works  |
| `cursor`      | `~/.config/Cursor/.../state.vscdb` (SQLite)                 | works  |
| `grok`        | `~/.grok/auth.json` (Grok CLI / SuperGrok Build)            | works  |
| `opencode-go` | `~/.local/share/opencode/opencode.db` (SQLite)             | works  |

Each provider auto-refreshes short-lived OAuth tokens when needed and persists
the refreshed token back to its source.

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
