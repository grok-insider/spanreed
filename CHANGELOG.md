# Changelog

All notable, user-facing changes to spanreed are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.6] - 2026-08-13

- ci: ship CLIs with one merge to master as one release
- docs: pin Nix flake install URLs to release tags
- feat: detect early weekly resets without inflating at 100% week
- feat: record per-provider weekly pool % at first probe
- fix: do not scale mid-week observation as if the pool started at 0%
- feat: link and send community share from the tray
- docs: install and update from github:grok-insider/spanreed
- fix: wrap Ayatana/GTK on LD_LIBRARY_PATH for tray dlopen
- feat: enable Linux SNI tray in Nix and fix tray feedback

## [0.0.5] - 2026-08-11

- fix: tray menu feedback and safe Windows binary replace

## [0.0.4] - 2026-08-11

- ci: install GTK/AppIndicator deps for Linux tray feature check
- docs: note Linux GTK deps for tray CI
- fix: cfg-gate tray_autostart Command and RUN_VALUE
- ci: build and test with --features tray (musl release stays static)
- fix: silence tray_format dead_code without tray feature
- docs: how to enable tray feature in CI and release builds
- feat: self-update and Behelit system tray

## [0.0.3] - 2026-08-10

- feat(share): require X-linked login for community pool
- feat(share): due-gate and login/missed-run catch-up
- test(share): expect schema v2 when economics present
- fix(share): clippy clean share_economics
- feat(share): multi-model economics, client_id, daily Madrid schedule
- fix(windows): run capture watchdog without a console window

## [0.0.2] - 2026-08-10

- fix(share): map ProgressFormat::Count to kind count not percent
- style: rustfmt share module
- feat: opt-in share of aggregated quotas to api.grokinsider.net
- fix(install): resolve versioned musl/windows release assets
- docs: install via grokinsider.net; GH Releases are binaries only

## [0.0.1] - 2026-08-10

### Added

- Daemon + CLI + status-bar JSON for subscription usage probes
- Multi-provider detection and probing from local credentials
- Optional local cost estimates from CLI session logs
- Grok capture proxy, ledger, and public API list-price cost estimates
- Windows setup/install helpers and capture ensure diagnostics
- Nix flake packaging
