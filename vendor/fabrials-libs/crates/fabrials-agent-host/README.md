# fabrials-agent-host

Agent host for Fabrials programs. It supervises the user's own Grok Build CLI
(`grok agent --no-leader stdio`, over ACP) and serves the `light.local.v1`
loopback API that the hosted Work UI at `https://desktop.grok.me` drives.

This crate is the library form of `grok-bridge` from grok-desktop-portable,
moved here with its history. The SPA, the protocol documentation
(`docs/protocol.md`), the threat model and the ADRs referenced in the code
("ADR light NNNN") stay in that repository.

Edition 2024, MSRV 1.95, AGPL-3.0-or-later, `publish = false`.

## Embedding

Run the whole host:

```rust
use fabrials_agent_host::{HostConfig, HostIdentity, serve, state_dir};

let dirs = state_dir::resolve_state_dirs()?;
let mut config = HostConfig::new(
    dirs.state_dir,
    HostIdentity::new("spanreed", env!("CARGO_PKG_VERSION")),
);
config.legacy_state_dir = dirs.legacy_state_dir; // adopt grok-bridge state
serve(config).await?;
```

`start(config)` returns a `RunningHost` once the listener is bound (origin,
IPv6 status, migration outcome), and `RunningHost::wait` serves until a
shutdown is requested through the control channel or `RunningHost::shutdown`.

Or expose the command line as a subcommand, e.g. `spanreed agent <command>`:

```rust
use fabrials_agent_host::{CliDefaults, HostIdentity, cli};

let defaults = CliDefaults::new(HostIdentity::new("spanreed", "0.7.0"), "spanreed agent");
cli::run(&args, defaults) // args after `agent`; returns ExitCode
```

Commands: `serve`, `open`, `status`, `doctor`, `stop`, `repair`,
`workspace add <path> | list | remove <id>`. `cli::run` builds its own Tokio
runtime; inside an existing runtime use `cli::run_async`.

## Wire contract

Unchanged from grok-bridge, so the hosted SPA keeps working:

- Routes `/healthz`, `/pair`, `/session`, `/command`, `/events`; protocol
  `light.local.v1`, `protocolVersion` 2.
- Headers `x-gl-session` and `x-grok-light-csrf`, cookie `gl_session`,
  WebSocket subprotocol `light.local.v1` plus `gls.<session-token>`.
- Origin `http://<install-id>.grok-light.localhost:<port>`, port derived from
  the install id in 20000–32767; hosted origin allow-list
  `https://desktop.grok.me` (configurable through `HostConfig::allowed_origins`).
- `/healthz` adds `host` and `hostVersion` from `HostIdentity`.

## State directory

| Platform | Default | Legacy (grok-bridge) |
|----------|---------|----------------------|
| Linux | `$XDG_STATE_HOME/spanreed/agent` (else `~/.local/state/spanreed/agent`) | `…/grok-bridge` |
| macOS | `~/Library/Application Support/spanreed/agent` | `…/grok-bridge` |
| Windows | `%LOCALAPPDATA%\spanreed\agent` | `…\grok-bridge` |

Overrides, in order: `FABRIALS_AGENT_STATE_DIR`, `GROK_BRIDGE_STATE_DIR`,
`GROK_LIGHT_STATE_DIR`. With an override no migration happens.

`migrate_legacy_state(new_dir, legacy_dir)` runs when the new directory has
no `origin.json` and the legacy one has a valid one. It copies `origin.json`,
`workspaces.json` and `journal.json` (owner-only; files already present are
kept), so the install id, port and bookmarks survive. The lock and control
socket stay behind. A second call returns `AlreadyInitialised`. The CLI and
`start` call it before touching state.

Files: `origin.json`, `host.lock`, `journal.json`, `workspaces.json`,
`control.sock` (Unix) or `control.pipe` (Windows named-pipe marker).

## Agent

`HostConfig::agent_program` (CLI: `FABRIALS_AGENT_PROGRAM`, then
`GROK_BRIDGE_AGENT`, then `grok`). The agent inherits the user's environment,
including `GROK_HOME`. When it exits, the host relaunches it with backoff
(1 s doubling to 30 s, reset after 60 s up; `HostConfig::relaunch`), runs the
ACP handshake again and emits `hostStatus` events: `agent_restarting`, then
`agent_ready`. Shutdown stops the relaunching.

## Directory picker

Linux uses the xdg-desktop-portal FileChooser, macOS runs `osascript`
(`choose folder`), other platforms report the picker as unavailable and rely
on `workspace add`.

## Embedded SPA

`build.rs` embeds the SPA when `FABRIALS_AGENT_HOST_WEB_DIST` names its
`dist` directory (absolute, or relative to this crate), e.g.

```sh
FABRIALS_AGENT_HOST_WEB_DIST=/path/to/grok-desktop-portable/apps/web/dist cargo build --release
```

Unset, the host serves a placeholder page on loopback and the hosted UI at
`https://desktop.grok.me` still works. A set but missing directory fails the
build.

## Tests

```sh
cargo test -p fabrials-agent-host
```

`examples/fake_agent.rs` is a scripted ACP agent used by the tests
(`FAKE_AGENT_INIT_LOG`, `FAKE_AGENT_EXIT_ONCE`). `tests/real_cli_contract.rs`
skips unless a real `grok` is installed. `examples/dev_host.rs` starts a bare
host for manual browser checks.
