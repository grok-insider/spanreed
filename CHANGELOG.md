# Changelog

All notable, user-facing changes to spanreed are documented here, newest
first. Release sections are generated from the commit history by an LLM (see
`scripts/gen-changelog.sh`) and finalized in the release pull request — edit the
notes there rather than by hand.

## [0.1.1](https://github.com/grok-insider/spanreed/compare/v0.1.0...v0.1.1) - 2026-07-09

### Added

- *(waybar)* show last-used provider instead of worst utilization
- *(release)* ship macOS + Windows binaries; clarify per-OS paths ([#9](https://github.com/grok-insider/spanreed/pull/9))
- *(platform)* add cross-platform ProcessList seam ([#10](https://github.com/grok-insider/spanreed/pull/10))
- *(platform)* add cross-platform SecretStore seam ([#7](https://github.com/grok-insider/spanreed/pull/7))

### Fixed

- *(release)* make release-plz open Release PRs for this app crate
- *(grok)* use weekly SuperGrok usage pool via billing?format=credits

### Other

- cross-platform framing, prebuilt install, accurate Waybar text ([#11](https://github.com/grok-insider/spanreed/pull/11))

## 0.1.0

- Added support for 17 AI coding subscription providers including Claude, Codex, Cursor, Grok, and more.
- Added a background daemon with a local HTTP API for continuous usage tracking.
- Added Waybar integration with custom module output.
- Added CLI commands: `list`, `json`, `waybar`, `serve`, and `update-pricing`.
- Added automatic pricing updates from LiteLLM, ensuring new models are correctly costed without a binary update.
- Added cost estimation for Claude and Codex, showing "Last 30 Days" spend and "Usage Trend" sparkline.
- Added window reset countdowns to usage displays in both CLI and Waybar tooltip.
- Added a Nix flake and Home Manager module for easy installation on NixOS.
- Added prebuilt binaries via Cachix for faster installation.
- Added documentation files (LICENSE, AGENTS.md, CONTRIBUTING.md) and expanded the README.
- Changed the project maintainer from 0xfell to grok-insider.
- Improved Waybar bar text: now only shows paid Claude, Codex, and Grok sessions, anchored on a 5‑hour window.
- Improved daemon reliability: retries on network startup failures and retains last‑good results across temporary failures.
- Improved cross‑platform support: code now compiles and tests on macOS and Windows (from source).
- Fixed SIGPIPE crash when piping CLI output (e.g., `spanreed json | head`).
- Fixed Waybar tooltip showing "null" for the Usage Trend sparkline.
