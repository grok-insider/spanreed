# spanreed

Track AI coding subscription usage from the terminal.

One Rust binary (`spanreed`) reads credentials that your local AI CLIs already
store, probes each provider’s usage API, and prints:

- plain text for the terminal
- JSON for status bars (e.g. Waybar)
- a small local HTTP API for dashboards

Linux-first (Hyprland/Wayland); the same code also builds for macOS and Windows.

## Install

### From grokinsider.net (one command)

Canonical install is on **[grokinsider.net](https://grokinsider.net)** — not the
GitHub Release page. Releases only publish platform binaries + checksums; the
website serves the bootstrap scripts.

```sh
# Linux / macOS
curl -fsSL https://grokinsider.net/install/spanreed.sh | sh

# Windows (PowerShell)
irm https://grokinsider.net/install/spanreed.ps1 | iex
```

The installer downloads the binary from GitHub Releases, then runs
**`spanreed setup`**, which asks:

- install CLI to user PATH  
- create the Grok capture ledger  
- start the capture proxy at login (optional; **windowless** on Windows)  
- wire **Grok Build** → `http://127.0.0.1:18736/v1`  
- wire **OpenCode xAI** → `http://127.0.0.1:18737/v1`  

On Windows, capture autostart uses a user **Scheduled Task** (Hidden) when
allowed, plus an **HKCU Run** fallback. The watchdog detaches from the console
(`FreeConsole`) so login does **not** leave a visible `cmd` window; logs go to
`%LOCALAPPDATA%\spanreed\logs\capture.log`.

### With Nix (Linux)

Install or update by pointing Nix at this repository (gnu build **with tray**).
GitHub Release musl zips stay headless; use this flake on a desktop.

```sh
# stable = last GitHub Release tag (created with the release, not with master)
nix run github:grok-insider/spanreed/v0.1.0 -- probe
nix profile install github:grok-insider/spanreed/v0.1.0

# unreleased integration line
nix run github:grok-insider/spanreed/dev -- probe
```

Do **not** use `github:grok-insider/spanreed` without a tag: that follows
`master`, which can sit ahead of the last release.

First `nix run`/`nix profile` may ask to trust `nixConfig` (Cachix
`grok-insider.cachix.org`). Accept it, or add the substituter in your NixOS
`nix.settings`.

Flake input + Home Manager:

```nix
{
  inputs.spanreed.url = "github:grok-insider/spanreed/v0.1.0";
  # optional: inputs.spanreed.inputs.nixpkgs.follows = "nixpkgs";

  # home-manager:
  imports = [ inputs.spanreed.homeManagerModules.default ];
  programs.spanreed = {
    enable = true;
    serve.enable = true;
    capture.enable = true;
    tray.enable = true; # needs Waybar `tray` (or another SNI host)
  };
}
```

After a new GitHub Release, bump the tag in the input URL, then
`nix flake update spanreed` and rebuild. Overlay: `overlays.default`
exposes `pkgs.spanreed`.

Nix-managed installs refuse `self-update --yes` (the store path is immutable).

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
spanreed self-update --check      # compare to latest GitHub Release
spanreed self-update              # download, verify .sha256, replace binary
spanreed tray                     # system tray (build with --features tray)
```

If Grok Build or OpenCode “stops working” while wired to the local proxy, check
capture first (`setup status` / `capture status`). A dead proxy with live wiring
looks like a CLI failure. Fix: `spanreed capture ensure`.

### Updates

```sh
spanreed self-update --check      # exit 0 if current, 2 if newer
spanreed self-update --dry-run    # download + checksum only
spanreed self-update --yes        # apply without prompt
```

Assets come from GitHub Releases (same as the installer). Checksums are verified
before replace; capture is restarted via `capture ensure` when possible.

### System tray (optional)

```sh
cargo build --release --features tray
spanreed tray                     # Spanreed icon: capture health + % remaining
```

Windows/macOS release binaries include the tray feature. Linux **musl** release
builds stay headless (use Waybar + `spanreed waybar`). The Nix package and
local `linux-gnu` builds enable `--features tray` (GTK3 + Ayatana SNI). On
Hyprland the icon appears in Waybar’s `tray` module next to `custom/spanreed`.
Home Manager: `programs.spanreed.tray.enable = true`. Nix-managed installs
refuse `self-update --yes` — upgrade the flake input or `nix profile upgrade`.

Capture logs (Windows): `%LOCALAPPDATA%\spanreed\logs\capture.log`.
After upgrading spanreed on Windows, re-run `spanreed setup` (or
`setup --yes --service`) so autostart re-registers the windowless watchdog.

## Build

```sh
cargo build --release
# binary: ./target/release/spanreed
```

From a checkout: `nix build` / `nix run . -- probe`. From GitHub, see **With Nix** above.

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
