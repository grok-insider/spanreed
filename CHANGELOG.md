# Changelog

All notable, user-facing changes to Spanreed are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.2] - 2026-09-07

- fix: consume release commit history under pipefail
- fix: synchronize desktop versions in release preparation
- fix: preserve generated contract line endings on Windows
- ci: defer macOS and ARM qualification and release artifacts
- feat: add shared desktop console and provider runtime
- Show Codex 5h/Weekly pools, team plans, and extra windows.
- feat: route capture through ai-relay and remove in-process proxy (#4)
- docs: add Cursor Cloud setup notes for cloud agents (#3)
- feat: rename product to Spanreed
- chore: release v0.0.6
- ci: ship CLIs with one merge to master as one release
- docs: pin Nix flake install URLs to release tags
- feat: detect early weekly resets without inflating at 100% week
- feat: record per-provider weekly pool % at first probe
- fix: do not scale mid-week observation as if the pool started at 0%
- feat: link and send community share from the tray
- docs: install and update from github:grok-insider/spanreed
- fix: wrap Ayatana/GTK on LD_LIBRARY_PATH for tray dlopen
- feat: enable Linux SNI tray in Nix and fix tray feedback
- ci: re-trigger release PR checks
- chore: release v0.0.5
- fix: tray menu feedback and safe Windows binary replace
- ci: re-trigger release PR checks
- chore: release v0.0.4
- ci: install GTK/AppIndicator deps for Linux tray feature check
- docs: note Linux GTK deps for tray CI
- fix: cfg-gate tray_autostart Command and RUN_VALUE
- ci: build and test with --features tray (musl release stays static)
- fix: silence tray_format dead_code without tray feature
- docs: how to enable tray feature in CI and release builds
- feat: self-update and Behelit system tray
- ci: re-trigger release PR checks
- chore: release v0.0.3
- feat(share): require X-linked login for community pool
- feat(share): due-gate and login/missed-run catch-up
- test(share): expect schema v2 when economics present
- fix(share): clippy clean share_economics
- feat(share): multi-model economics, client_id, daily Madrid schedule
- fix(windows): run capture watchdog without a console window
- chore: release v0.0.2
- fix(share): map ProgressFormat::Count to kind count not percent
- style: rustfmt share module
- feat: opt-in share of aggregated quotas to api.grokinsider.net
- fix(install): resolve versioned musl/windows release assets
- docs: install via grokinsider.net; GH Releases are binaries only
- fix: scope Windows PATH string to windows cfg only
- ci: restore Model A workflows, release-plz, and AI changelog
- fix: quiet bare TCP probes and ASCII capture log lines
- feat: capture watchdog and persistent logs
- feat: add Windows setup/install and capture ensure diagnostics
- feat(grok): estimate cost with public API list prices
- fix(grok): treat missing creditUsagePercent as weekly 0%
- feat: drop Claude, hide Grok plan noise, strip Codex models/cache, add forecasts
- style: rustfmt after MetricKind/probe flag wiring
- docs: document compact probe default and detail flags
- feat(probe): default to quotas only; add --cost/--models/--cache/--trend/--plan/--all
- feat(model): add MetricKind semantic tags on all metric lines
- docs: use a real spanreed probe snapshot in the README
- docs: describe the product without anti-framework pitch
- docs: simplify README and drop remote/CI-centric copy
- chore: remove GitHub Actions and release automation
- ci: update grok-insider Cachix public signing key
- feat: precise Grok capture costs and honest estimate labels
- feat: record rate-limit history with force-reset epochs
- feat: show local tokens since weekly rate-limit epoch
- feat: show Models and Cache breakdown from local usage logs
- style(grok): rustfmt assert in billing unit test
- fix(grok): fall back to monthly billing when credits omit usage
- ci: fix release automation branch checkout and gate binary uploads
- ci: fix duplicated if expression on nix build job
- ci: release policy — AI changelog action, admin X/Y, patch-only auto, full guard
- ci: use release-changelog-action@v1 for AI release notes
- chore: reset public version line to 0.0.1
- ci: run master workflows only from PRs, not bare pushes
- ci: adopt Model A (dev integration + guard-master)
- style: rustfmt + clippy for copilot opt-in auth
- feat(copilot): opt-in auth instead of auto-detect from gh
- fix(nix): derive package version from Cargo.toml
- docs: update changelog
- chore: version bump
- style: rustfmt claude plan-period test
- feat: show paid plan renew / last period dates
- feat(capture): dual-listen system capture + HM capture.enable
- docs: mention grok-proxy in main module header
- feat(grok): product usage breakdown + accurate token capture path
- docs: update changelog
- chore: version bump
- fix(release): make release automation open Release PRs for this app crate
- feat(waybar): show last-used provider instead of worst utilization
- fix(grok): use weekly SuperGrok usage pool via billing?format=credits
- docs: cross-platform framing, prebuilt install, accurate Waybar text
- feat(release): ship macOS + Windows binaries; clarify per-OS paths
- feat(platform): add cross-platform ProcessList seam
- feat(platform): add cross-platform SecretStore seam
- docs: update changelog
- fix(ci): open Release PR on first release (release automation + AI changelog)
- ci: skip release automation cleanly when RELEASE_PLZ_TOKEN is unset
- ci: automated releases with release automation + LLM-written changelog
- feat(platform): cross-platform port Phase 0 — compile on macOS & Windows
- chore(gitignore): add defensive OS/editor/log/env ignores
- ci: add required fmt+clippy+test gate and release workflow
- chore: migrate project identity to grok-insider
- feat(waybar): filter paid providers and anchor on Session window
- feat(pricing): auto-refresh from upstream and dynamic Claude windows
- ci: bump actions/checkout to v6, cachix-action to v17 (Node 24)
- fix: serve daemon retries on network-down and retains last-good
- feat: window reset countdowns and Waybar usage trend tooltip
- feat: local-log cost estimation, SIGPIPE fix, and test suite
- docs: add LICENSE, AGENTS.md, CONTRIBUTING.md; expand README
- feat: add twelve more providers for full parity
- ci: add Nix flake and Cachix for prebuilt installs
- feat: initial Linux-native spanreed daemon, CLI, and Waybar

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
