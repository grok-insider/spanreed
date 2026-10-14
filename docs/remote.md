# Spanreed Remote

The current PWA is `https://desktop.grok.me` and connects directly to the user’s HTTPS/WSS endpoint with a pinned Noise host identity. Private use needs no Fabrials account. Publication authorization is a separate, explicit flow; publishing is still closed pending its release gates.

Remote is part of the normal `spanreed` executable. The CLI, native host, Noise protocol and embedded Portable PWA update together through the existing release archive, installer, `spanreed setup`, `self-update` or Nix package. There is no adjacent companion to install or update. `spanreed-remote-relay` is a separate server executable; users do not need it on their computer.

## Historical managed relay adapter

This adapter remains for compatibility testing. It is not the public product entry and is never an automatic fallback for a direct connection. The current product uses the direct connection described below.

The Fabrials account control plane, relay and `remote.fabrials.com` must be configured by the operator. On the user's computer:

```sh
spanreed remote doctor
spanreed share login
spanreed remote enable "My laptop"
spanreed remote serve
```

`doctor` verifies that this executable includes the PWA shell and Noise WASM. `spanreed share login` authorizes a Fabrials installation against `https://fabrials.com/api/spanreed`; it does not upload metrics or publish conversations. Remote uses the user's Grok executable, credentials, home and shared leader sessions. Only one agent host can use the same state directory, so stop an existing local/desktop agent first.

`serve` remains in the foreground. In a second terminal:

```sh
spanreed remote pair
```

Open its one-use QR/link on the intended browser, signed into the same Fabrials account. Pairing expires after 90 seconds. Account access and host pairing are separate requirements. On iOS, install the PWA before pairing because the installed application can use separate storage.

`spanreed remote status` distinguishes `agentRunning` from the relay's `runtime.connection`. Treat the latter as current only when `runtimeFresh` is true. Connection and publication failures are bounded diagnostic categories, without credentials or server response bodies. A running local agent does not imply a connected relay.

## Private HTTPS with Tailscale

Private mode serves the direct `/remote` endpoint and keeps the embedded PWA as a local fallback. The primary application remains `desktop.grok.me`. Use an exact HTTPS origin with a hostname and no trailing slash, credentials, path, query or fragment. Tailscale is recommended; a user-managed HTTPS/WSS reverse proxy is also supported by the configuration. TLS trust and the network policy must be qualified on each intended device. The proxy must preserve the external Host header and WebSocket upgrade. The browser Origin allowlist remains exactly `https://desktop.grok.me` for the direct endpoint.

Connect the computer and intended device to the tailnet, enable MagicDNS and HTTPS certificates in its DNS settings, and allow the device to reach the computer on TCP 443 in the access policy. A restrictive policy does not grant a newly added phone access automatically. The phone does not need access to the raw loopback port or SSH. Verify the existing configuration with `tailscale serve status` before assigning its HTTPS root to Remote.

```sh
spanreed remote private-serve https://station.example-tailnet.ts.net
```

This remains in the foreground and prints its actual loopback port. In another terminal, configure Tailscale Serve to proxy HTTPS to that address, for example `tailscale serve --bg --https=443 http://127.0.0.1:PORT`, replacing `PORT` with the printed value. Preserve the external Host header. Use Serve inside the tailnet; Funnel exposes a public endpoint and is not this private mode.

Run `spanreed remote private-pair https://station.example-tailnet.ts.net`. Its one-use 90-second pairing link opens `desktop.grok.me` with the endpoint, installation and host identity in the fragment; the PWA removes that fragment and stores the paired profile locally. `spanreed remote pair` remains the historical managed relay command. The device must have tailnet access and the computer must remain running. Already paired private sessions keep working when Fabrials services are unavailable. `spanreed remote private-stop` stops the host while retaining its identity and pairings. Apple platforms require the experimental opt-in until actual-device qualification.

## Autostart

Stop the foreground host before installing its current-user service. Preview or install managed mode:

```sh
spanreed remote service print
spanreed remote service install
```

For private mode:

```sh
spanreed remote service print --private-origin https://station.example-tailnet.ts.net
spanreed remote service install --private-origin https://station.example-tailnet.ts.net
```

The private origin is validated before writing files or invoking a service manager. Tailscale Serve is configured separately. Use `spanreed remote service uninstall` before switching between managed and private services; both use the same host state and service identity.

- Linux: user unit `spanreed-remote.service` under `$XDG_CONFIG_HOME/systemd/user` or `~/.config/systemd/user`, running `spanreed remote serve` or `private-serve` with the selected origin; restart on failure, umask 0077.
- macOS: `~/Library/LaunchAgents/com.fabrials.spanreed.remote.plist` in `gui/<uid>`; restart on unsuccessful exit. Uninstall a loaded older launch agent before reinstalling.
- Windows: task `Spanreed Remote <current-user-SID>`, limited interactive current-user principal, logon trigger, retry on failure and no default 72-hour execution limit.

These commands do not create system services or enable a login session. The computer must remain on. Configure `FABRIALS_AGENT_PROGRAM` in the service environment if Grok is not on its PATH; shell aliases do not apply. On Unix, an atomic binary replacement leaves running hosts on their current executable until restarted; restart after active work finishes to use the new version.

On Windows, stop Remote before replacing an installation: wait for active work to finish, stop the foreground host, and run `spanreed remote service uninstall` if its task is installed. Reinstall the same service mode and private origin afterward (or restart the foreground command). Replacing an executable held by an active Remote process is outside the qualified installer lifecycle. Keep the registration and pairing state; `remote disable` is not needed for a binary update. Windows self-update is explicitly disabled; active host replacement and the installer lifecycle have not been qualified.

### Self-update safety

On supported unmanaged Unix installations, self-update takes the real agent-host instance lock before downloading or applying a release. An active agent or Remote host causes a refusal without stopping its sessions. Finish active work, stop the host explicitly, apply the update, then restart the same host configuration. The lock remains held through replacement so a host cannot start in the middle of it.

Only explicitly published, stable releases with a canonical three-part version are accepted. The downloaded archive must match the single SHA256 sidecar entry naming that exact asset. The candidate is written and flushed in an owner-only directory on the destination filesystem. A bounded `--version` check must match before replacement and again at the installed path. The installation lock, SHA-pinned previous/candidate files and atomically persisted phase journal survive updater termination. Symlinks, writable shared Unix destinations, corrupted recovery metadata and an unexpected installed checksum are refused. Capture processes and user services are not stopped or restarted implicitly.

`spanreed self-update --status` inspects the local transaction without a network request; `--recover` reconciles an interrupted transaction; `--rollback` explicitly restores the retained previous executable. The successful installed-version check commits the transaction while retaining the previous version for rollback. Failed validation restores the previous executable; failed recovery leaves verified artifacts for inspection. The tests terminate real updater subprocesses at each persisted transition and immediately after replacement, then recover them without applying another update.

Windows update and dry-run paths remain disabled before lookup, download or mutation; `--check` and local `--status` remain available. The implementation now inspects the executable owner's DACL using an actual file handle, rejects foreign modifying permissions/reparse points, and starts version probes suspended before assigning their process to a Job. The handoff helper binds parent PID to process creation time and reacquires update/host locks. These paths and their native fixtures still need a real Windows build and qualification; their existence does not open the gate. Failures after helper journal acquisition are recorded in `last_error`. A failure before journal acquisition can leave a prepared transaction without that extra diagnostic, so inspect status and recover explicitly.

The current Linux validation is 31 updater tests plus clippy for adapters, app and CLI. Process-crash recovery is tested; OS crash/power-loss behavior is not. Directory fsync is used on Unix; native Windows replacement/power-loss guarantees remain unqualified. Version checks do not prove every runtime feature. Distribution and embedded-license work is documented in [Native distribution](native-distribution.md). Older candidate evidence in `evaluation/direct-update-2026-10-13.json` describes its recorded binary only.

Nix users install the regular Spanreed package. Home Manager exposes `programs.spanreed.remote.enable`, optional `privateOrigin`, and `agentProgram` for an absolute Grok executable. With `privateOrigin = null`, first complete the account login and `remote enable` interactively. Home Manager manages the unit; do not overwrite it with `remote service install`.

## Explicit qualification web origin

`desktop.grok.me` remains the default hosted UI. A separate qualification installation can select `https://grok.fabrials.com` explicitly:

```sh
spanreed remote private-serve https://station.example.ts.net --web-origin https://grok.fabrials.com
spanreed remote private-pair https://station.example.ts.net --web-origin https://grok.fabrials.com
```

Use the same dedicated `FABRIALS_AGENT_STATE_DIR` for these foreground commands. With that override or either legacy override (`GROK_BRIDGE_STATE_DIR`, `GROK_LIGHT_STATE_DIR`) present, every service action (`print`, `install`, `uninstall`) refuses before filesystem writes or invoking a service manager: generated services do not preserve a custom state directory, and uninstall could otherwise stop the usual installation. Do not unset the override to operate on an isolated installation; use the foreground host for qualification.

For installations using the normal state directory, service print/install accept `--private-origin https://station.example.ts.net --web-origin https://grok.fabrials.com`; their rendered commands preserve the selected web origin on Linux, macOS and Windows. Only `https://desktop.grok.me` and `https://grok.fabrials.com` are accepted, exactly as written: no alternate ports, trailing slash, credentials, path, query, fragment, aliases or wildcard subdomains. `--web-origin` requires private mode and does not change managed enrollment or public-publication origins.

`direct.json` stores `webOrigin` alongside the endpoint and pinned host identity. Existing identities without the field retain `desktop.grok.me`; no implicit migration rewrites their identity. Starting, pairing or installing a service with another endpoint or web origin fails before host startup or service writes. Use a separate qualification installation and pair it explicitly; changing an existing installation requires an explicit migration procedure. `remote status` exposes the configured `webOrigin`. The private host accepts its selected hosted web origin and its own HTTPS endpoint; the unselected hosted origin remains denied.

Build the matching QA PWA with `FABRIALS_REMOTE_ORIGIN=https://grok.fabrials.com`; the production default remains unchanged. The lifecycle harness accepts `--web-origin https://grok.fabrials.com` and includes refusal of pairing or startup under another hosted origin. Those socket checks must run in an environment that permits binding local sockets; the restricted 2026-10-13 follow-up could run pure validation, persistence, CLI preflight and service rendering but could not repeat a live connection.

## Revocation and recovery

`spanreed remote devices` lists paired browsers and `spanreed remote revoke <device>` revokes one. `service uninstall` only stops/removes autostart; it does not revoke account access. `remote disable` disables local managed mode, requests cloud host revocation and stops the host. Cloud host revocation also withdraws its public conversation links. Failed cloud revocation must be retried.

To replace a managed registration:

1. Use the owner account and run `spanreed remote disable`. If the old installation login has expired or is unavailable, run `spanreed share login` into the **same owner account**, then retry. Host deletion is account-scoped; another account cannot revoke it.
2. Stop the foreground host and uninstall its user service if installed, so it cannot restart.
3. Run `spanreed remote reset`. It requires disabled state, obtains the instance lock, rechecks cloud revocation and only then removes the local Remote identity. Keep state files intact if a check fails.
4. Run `spanreed share logout`, then `spanreed share login` to authorize a fresh installation. Logout only removes the local account session and does not replace cloud revocation.
5. Run `spanreed remote enable "My laptop"`, start it and pair the browsers again. Reinstall autostart after stopping the foreground instance if desired.

For direct mode, revoke a paired browser with `spanreed remote revoke <device>` or forget its profile in the PWA. Stop the host using `private-stop`; do not use the historical managed reset sequence to rotate direct identity. A changed endpoint or installation identity requires explicit migration/re-pairing; silently replacing the pinned identity is refused.

To withdraw an individual public conversation while its computer is offline, use `https://fabrials.com/account/publications` in a first-party account session. The host records a pending deletion until the service confirms both access withdrawal and destruction of its keys. Withdrawal cannot recall copies already downloaded by readers.

## Reproducible source distribution

Change native libraries in canonical `fabrials-libs`; import the reviewed, committed revision with `scripts/sync-fabrials-libs.sh <repository> <commit>`. The subtree workflow requires a clean consumer tree and creates commits. Never edit `vendor/fabrials-libs` manually. A disposable consumer snapshot can qualify uncommitted library work, but does not make the repository releasable.

The checked-in `assets/remote-web` bundle allows offline native/Nix builds without Node. `.cargo/config.toml` supplies its path; Nix sets the same bundle explicitly. `assets/remote-web.source.json` records SHA256 for every emitted file and every selected Portable/Noise build input, each source HEAD and whether those inputs were uncommitted. It never represents uncommitted content as a clean HEAD.

To refresh it, install Portable dependencies from its frozen lockfile and use a Rust toolchain with the `wasm32-unknown-unknown` standard library, then run:

```sh
python3 scripts/remote-assets.py build \
  --portable /absolute/path/to/grok-desktop-portable \
  --libs /absolute/path/to/fabrials-libs
python3 scripts/remote-assets.py check
```

The builder compiles Noise and the matching bindgen helper with locked dependencies, builds Portable with the resulting WASM, and refuses changed source inputs during the build. It requires Node 22+, Corepack/pnpm and the source toolchains. Required shell files, one matching versioned JS/WASM pair, same-origin manifest scope, symlink rejection and all hashes are validated. The checked-in bundle preserves byte-for-byte line endings on Windows.

Normal development builds use `cargo build --locked`. `scripts/build-remote.sh` also builds the standalone relay and copies native release binaries to `target/remote-dist` (override `SPANREED_REMOTE_DIST`), using explicit target directories independent of Cargo environment overrides. It runs vendor integrity and bundle checks and invokes the resulting `spanreed remote doctor`; it does not install or contact a relay. Windows release builds use the normal Cargo/Actions path.

CI validates bundle integrity, the normal workspace including Remote, the standalone relay and cross-platform builds. Release builders additionally run `remote-assets.py check --release`, which rejects uncommitted source identities, and execute `remote doctor` on runnable native artifacts. Rebuild the bundle from committed Portable/Noise sources before publishing. Nix exposes `spanreed-remote-relay` separately and checks it alongside the normal CLI and desktop.

## Qualification

The 2026-10-13 direct candidate was compiled in a disposable consumer copy
against the canonical uncommitted libraries, without editing this consumer's
vendor. `scripts/qualify-direct-lifecycle.py --binary <candidate> --output <receipt>`
passed 16 checks on Linux: invalid-origin/socket preflight without state writes,
service preview, real foreground startup/status, private filesystem mode,
fragment pairing and endpoint binding, explicit stop, stable identity on restart,
single-instance rejection and failed endpoint-migration preservation. It used
temporary state and an empty synthetic Grok home; no service was installed and
the child and temporary state were removed. Receipt:
`evaluation/direct-lifecycle-2026-10-13.json`. This does not qualify the actual
service manager, Windows, TLS/Tailscale, model inference or release update/rollback.

The lifecycle receipt was refreshed after embedding the final Source/license shell and rebuilding the candidate. `evaluation/direct-update-2026-10-13.json` binds that executable to the 47-file embedded bundle, its source manifest and the integration-only lockfile; 23 updater tests, two CLI checks and clippy passed again. Portable and Noise inputs remain explicitly uncommitted. The real consumer vendor and lockfile were preserved, so this evidence does not qualify a publishable clean-source release.

`scripts/qualify-direct-vision.ts` also exercised the frozen PWA artifact through real Noise, Spanreed and the official Grok 1.0.52 ACP binary using a synthetic profile in a filesystem sandbox and the local-development provider key. The final run passed 13 checks, including an unpredictable number carried only by an image, no tool calls, and image plus answer restoration after reload. It used hidden Syl Sway at 2560×1440 and no private Grok profile. The receipt records all four paid prompts, the default-renderer image bug found and fixed, an intermediate harness selector error, and exact binary/artifact hashes: `evaluation/direct-vision-2026-10-13.json`. This remains a laboratory TLS test, not Tailscale, a physical mobile-device check, or a publication/push qualification.

### Experimental diagnostics and approved project roots

`spanreed remote doctor --inspect` performs read-only local checks and prints
JSON. `--private-origin https://computer.example.ts.net` also validates that
origin using the service renderer's rules. It does not create host state,
refresh credentials, start Grok or claim that a present leader socket is alive.
Service environment, embedded terminal continuity and browser routing still
need their separate live checks.

The project browser candidate requires
`SPANREED_REMOTE_EXPERIMENT_PROJECTS=1` on the host and `remote-projects` in
Portable's evaluation build. Authorize roots from a terminal on that computer:

```sh
spanreed remote roots authorize /absolute/path/to/projects
spanreed remote roots list
spanreed remote roots revoke ROOT_ID
```

Relative paths are resolved at the caller. The owner-only local control channel
may show canonical paths; the browser receives short-lived opaque handles and
labels. This candidate is Unix-only until the equivalent Windows filesystem
identity guarantees are qualified. Revoking a browsing root invalidates its
handles; it does not withdraw a workspace already enrolled from that root.

Device management and explicit control transfer have separate host switches,
`SPANREED_REMOTE_EXPERIMENT_DEVICE_PANEL=1` and
`SPANREED_REMOTE_EXPERIMENT_CONTROL_TRANSFER=1`, and matching browser flags
`device-panel` and `control-transfer`. Transfer applies only to Shared agent
mode. A request is tied to one live connection, expires after 30 seconds and
requires approval from the current controller. Browser pairing identity alone
does not confer control. These switches are off by default and are not release
acceptance results.

Service rendering and operation plans are tested without installing a service. Native compile/test results, real service-manager lifecycle, official Grok continuity, Tailscale routing, Android/iOS installation and suspension are distinct checks. CI configuration and generated service files are not evidence that launchd, Task Scheduler or a mobile device have been exercised. Record the actual platform and results before distributing a release.

### Station QA activation, second candidate — 2026-10-14

The Linux QA profile activated the frozen Spanreed source
`7e2a7816c73d7e58b64852509b47ede75527859a` with Portable
`00393e13e45b7dfbbfcc9844b46d758f2a582486`. The GNU CLI SHA256 is
`90d3f5078a3f392220f59d80d09ae96ad3f8bf4b33b28ab6f5f3d1ccbf4c6bf3`;
the corresponding source archive SHA256 is
`377d1352f3a6366472216216755b35fc8fc7cde89e3dd8fde4a6655588ef0197`.
The sanitized [activation receipt](evaluation/station-activation-2026-10-14-r2.json)
records the verified artifact, original notices, service transition, stopped-host
backup, five byte-identical identity files and local/Tailscale HTTPS health.
Only the QA profile and its two owned unit links changed; the ordinary daemon,
shared leader and open terminal retained their process identities. No inference,
browser or mobile actions were performed during this activation. Process survival
does not qualify conversation continuity. Browser reconnect/revocation, S24 and
real terminal continuation remain separate acceptance checks; this is not a
public release or installer/self-update qualification.

### Station QA activation, third candidate — 2026-10-14

The Linux QA profile activated the frozen Spanreed source
`e6d9cc8cb076d7896edec98ee7338d4d4d8f1f7a` with Portable
`abe4f35895aacda102a09c3b50475379febf3b6c`. The GNU CLI SHA256 is
`5cdd5c4d8aa6c9d04cc99d9772b0964a3d2fde67658818f91a9180bdc2ff7e47`;
the corresponding source archive SHA256 is
`d7482730885ea14e67d3df818a6680ea731173bf700c4c4f4bd2555b34a90bfc`.
The [sanitized activation receipt](evaluation/station-activation-2026-10-14-r3.json)
records the independently verified CI, Desktop and Nix workflows for that exact
commit, the matching deployed public shell and source metadata, the private
stopped-host backup, five byte-identical identity files and trusted HTTPS health.
The QA unit, launcher and runtime closure were checked before activation; only
the QA profile and its two owned unit links changed. The ordinary daemon, shared
leader and open terminal retained their process identities. This receipt covers
no model request, browser or mobile action, and no public release. Process
survival does not qualify conversation continuity. The second candidate's
receipt remains historical evidence for its own source and artifact only.
