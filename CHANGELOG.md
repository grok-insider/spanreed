# Changelog

All notable, user-facing changes to Spanreed are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.0] - 2026-09-25

- fix: macOS test qualification - hermetic keyring test, full ps command, bounded cross job
- fix: build the macOS LaunchAgent plist test and path only on Unix
- fix: edition 2024 unsafe extern blocks and env writes in Windows code
- refactor: phase 3 - agent host entry in the desktop tray and AgentStatus contract
- refactor: phase 3 - separate spanreed-adapters from spanreed-app
- refactor: phase 2 - migrate to the reshaped fabrials-libs (8606b3a)
- chore: record fabrials-libs 8606b3a46265
- Squashed 'vendor/fabrials-libs/' changes from 4601449..8606b3a
- refactor: phase 3 - agent host tests run without building the fake agent
- refactor: phase 3 - document the workspace layout and lift the lifecycle fixture
- refactor: phase 3 - desktop and tray start the agent host
- refactor: phase 3 - Cargo workspace with domain, app, tray and CLI crates
- refactor: phase 3 - split large modules and functions
- refactor: phase 3 - CLI split into src/cli with one file per subcommand
- refactor: phase 3 - application facade shared by CLI, tray, desktop and API
- refactor: phase 3 - AppContext owns process state instead of statics
- refactor: phase 3 - break module cycles with ports
- Squashed 'vendor/fabrials-libs/' changes from 4ec9840..4601449
- chore: record fabrials-libs 46014491de2c
- feat: phase 5 - Spanreed absorbs grok-bridge as `spanreed agent`
- Squashed 'vendor/fabrials-libs/' changes from 656684f..4ec9840
- chore: record fabrials-libs 4ec9840531cc
- ci: phase 6 - build and test Spanreed on macOS
- refactor: phase 3 - move Spanreed to edition 2024 and deflake the proxy restart test
- refactor: phase 3 - use the shared Grok CLI token helpers
- refactor: phase 2 - build on fabrials-types and the shared pricing, accounts and share code
- Squashed 'vendor/fabrials-libs/' changes from 9838c51..656684f
- chore: record fabrials-libs 656684fa8677
- docs: phase 1 - describe the in-tree fabrials-libs workspace
- fix: phase 1 - read model contracts from fabrials-libs and sync from worktrees
- Squashed 'vendor/fabrials-libs/' changes from 2719c27..9838c51
- chore: record fabrials-libs 9838c516bd06
- build: phase 1 - build against in-tree fabrials-libs and relicense to AGPL-3.0
- Squashed 'vendor/fabrials-libs/' changes from 84fda68..2719c27
- chore: record fabrials-libs 2719c272df7d
- Squashed 'vendor/fabrials-libs/' content from commit 84fda68
- chore: record fabrials-libs 84fda68e625f
- chore: phase 1 - drop the per-crate vendor copies before adding fabrials-libs
- fix: phase 0 - price Grok hops with the user's pricing layers

## [0.6.2] - 2026-09-25

- chore: sync vendored fabrials crates
- chore: refresh the desktop UI vendor
- fix: keep local hops compatible with Grok Build goal checks
- feat: sync per-model usage and price OpenCode Go from models.dev
- feat: price grok-4.7 at the grok-4.6 list rate
- feat: show output tokens per second on captured requests

## [0.6.1] - 2026-09-25

- fix: keep the Windows alert on the user desktop
- fix: show the Windows alert on the active desktop
- fix: keep the macOS alert dialog off System Events
- fix: show the macOS alert dialog from the background tray
- fix: show the Windows alert with a WinForms dialog
- fix: realize the tray card before attaching its view
- fix: keep a parked desktop named spanreed-desktop
- fix: recognize a desktop process before its command line appears
- fix: apply a tray route when the page hash is already set
- fix: keep a tray card click when the window blurs
- fix: deliver tray alerts before the desktop handshake
- fix: reload the card when a reset is abandoned
- fix: clear a tray status from the Linux panel title
- fix: dispatch tray route changes on the page window
- fix: tell the desktop page when a tray route changes the hash
- fix: deliver desktop reset alerts through the system path
- fix: apply a tray route that arrives before the page listens
- test: drop status text from the card document assertion
- fix: keep a cleared status line out of the card document
- fix: update a tray status line without reloading the card
- fix: start the desktop when its command cannot be read
- fix: restore the capture-down line after a transient
- fix: keep capture-down visible during a refresh
- fix: show a dialog for background alerts too
- fix: keep the capture-down line until capture returns
- fix: clear a tray status line in the open card
- fix: show a Linux dialog when notifications fail
- fix: show a Windows dialog when the toast is unconfirmed
- fix: put gdbus on the tray notification path
- fix: treat only a desktop process as the open app
- style: format the desktop command matcher
- fix: treat a wrapped desktop binary as the running app
- fix: clear a reset line that never reports back
- fix: identify the Windows desktop by its process image
- fix: start the desktop when the lock pid is not the desktop
- fix: reload the open card for every tray status line
- fix: paint a tray status line on the open card
- fix: still deliver a Linux alert when the desktop accepts it
- fix: leave the tray route for the desktop page to read
- fix: show a macOS dialog when the banner is not confirmed
- fix: apply a tray page once instead of repeating it
- fix: apply a tray route even when the page timer is suspended
- fix: show the Windows alert dialog in a visible window
- fix: show a Windows dialog when the toast is not delivered
- fix: deliver a Linux alert when notify-send exits with an error
- fix: bring an open desktop window forward from the tray
- fix: run package tests without FHS binaries
- fix: keep the desktop route test from launching a host binary
- fix: show a dialog when a tray alert cannot be delivered
- fix: do not launch the CLI when opening the desktop
- fix: send macOS and Windows alerts without PATH
- fix: keep reopening the desktop after a notification panic
- fix: show a hidden desktop window when a tray route is waiting
- fix: look for the desktop app in Windows install folders
- fix: start the packaged Spanreed desktop binary
- fix: still deliver macOS and Windows alerts directly
- fix: fall back when the desktop cannot show an alert
- fix: hand tray alerts to the running desktop app
- fix: keep a Windows toast when the balloon cannot be shown
- fix: hide the desktop window instead of destroying it
- fix: refresh the tray card when a status line expires
- fix: show a Windows balloon even when the toast is dropped
- fix: send every tray alert through the platform notification
- fix: send quota-band alerts through the tray notification
- fix: open dashboard and settings from the desktop tray
- fix: keep tray alerts visible after they are sent
- fix: reach the Linux notification bus from system Python
- fix: let the tray service open the installed desktop app
- test: deliver a capture alert through the Linux notification bus
- test: cover opening a running and a newly started desktop
- fix: drop cleared status lines from the tray tooltip
- fix: show Windows tray alerts through a registered toast id
- fix: keep the desktop entry formatting
- fix: raise the desktop window from the tray route
- fix: deliver tray alerts when the Windows toast is unavailable
- fix: surface the desktop window and expire tray status
- fix: keep soup and webkit off the default package
- feat: open the desktop app from the tray
- fix: give every registry provider a tray mark
- feat: show one provider at a time in the tray
- feat: use provider marks on the tray card
- feat: place pace under each tray row
- feat: group the tray card like a menu bar
- feat: label the local installation with the machine hostname
- feat: offer Use reset on the tray card
- feat: open a usage card from the tray icon
- feat: cover every CodexBar provider
- fix: use the ruby gem as the app icon
- fix: release the account file lock on drop
- fix: send the xAI compat fixture to the mock upstream
- fix: show hosted resets relatively and fail stuck forwards
- fix: remove the unused Grok review wrapper
- fix(desktop): close the remaining setup and sharing gaps
- ci: retrigger stuck pull request checks
- fix: compare the Nix vendor links with normalized newlines
- fix: keep vendored UI bytes stable and refresh the Nix node hash
- feat(desktop): restyle the console with @fabrials/ui 0.4
- docs(desktop): describe the sections, vendored UI packages and fixtures
- feat(desktop): regroup the app into overview, usage, accounts, routing and connect
- feat(desktop): add browser fixtures for layout review
- fix: regenerate model contract bindings for hosted tools
- feat: replace GTK titlebar with in-app Spanreed chrome
- fix: match hosted OpenCode Go and Grok Build capture hops (#31)
- feat: route local capture through OpenCode Go (#29)

## [0.6.0] - 2026-09-09

- feat: track local consumption across clients and modernize Linux views (#25)

## [0.5.0] - 2026-09-09

- feat: connect local Codex usage and reviewed hosted flows (#21)

## [0.4.0] - 2026-09-08

- fix: wait for pushed release heads before dispatching checks
- refactor: restrict unbound remote calls to dashboard discovery
- fix: explain authorization conflicts and pending session checks
- feat: expose the Eww launcher for monitor-aware desktop bindings
- fix: report Windows notification delivery failures
- feat: connect native workspaces and synchronize selected private history

## [0.3.1] - 2026-09-07

- fix: resume release drafts before their tag exists
- fix: automate complete releases with scoped GitHub tokens

## [0.3.0] - 2026-09-07

- Add a local desktop console with the same design system and components as ai-relay, including accounts, model discovery, routing, usage history and request inspection.
- Embed the shared provider runtime directly in Spanreed, with durable credential rotation and SQLite usage storage.
- Add Nous OAuth and API-key accounts, SuperGrok device authorization, and read-only Codex/SuperGrok reset inventories with opt-in notifications.
- Add reviewed client configuration and bidirectional account migration with explicit approvals and fresh OAuth grants.
- Add built-in Waybar/Eww profiles, independent privacy controls, and Windows Credential Manager integration.
- Build Linux x86_64 and Windows x64 desktop review installers. Windows signing remains pending; macOS/ARM qualification and distribution are deferred until a hardware contributor provides native evidence.
- Keep CLI and desktop release versions synchronized; preserve generated contract line endings on Windows.

## [0.2.1] - 2026-08-28

- feat: Grok autosteer picks the soonest pool reset, then smaller plans (Premium+ before Heavy), without waiting for 100%
- feat: switch the active SuperGrok account as soon as a better live hop exists

## [0.2.0] - 2026-08-27

- feat: host-owned Grok accounts, fabric capture via ai-relay, and plan-aware ls
- feat: `spanreed sync` — push/pull SuperGrok plan, pool %, and capture hops to ai.fabrials.com (Fabrials X JWT; no Grok secrets)
- feat: point install and share at fabrials.com
- chore: depend on fabrials-* crates from crates.io

## [0.1.0] - 2026-08-14

First public Spanreed release (product rename from the tester-only 0.0.x line).

- feat: rename product, binary, paths, env, flake, and tray to Spanreed
- feat: detect early weekly resets without inflating at 100% week
- feat: record per-provider weekly pool % at first probe
- feat: Linux SNI tray and community share from the tray
- feat: self-update
- ci: one merge to master is one release
