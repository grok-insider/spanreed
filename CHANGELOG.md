# Changelog

All notable, user-facing changes to spanreed are documented here, newest
first. Release sections are generated from the commit history by an LLM (see
`scripts/gen-changelog.sh`) and finalized in the release pull request — edit the
notes there rather than by hand.

## [0.1.2](https://github.com/grok-insider/spanreed/compare/v0.1.1...v0.1.2) - 2026-07-10

### Added

- show paid plan renew / last period dates
- *(capture)* dual-listen system capture + HM capture.enable
- *(grok)* product usage breakdown + accurate token capture path

### Other

- rustfmt claude plan-period test
- mention grok-proxy in main module header

## 0.1.1

- Added cross-platform support for macOS and Windows, including OS-specific credential storage (Keychain, Credential Manager) and process discovery
- Added Waybar module that shows the last-used paid provider (Claude, Codex, or Grok) based on recent activity, falling back to highest utilization when no recent signal is found
- Added weekly SuperGrok usage pool detection via `billing?format=credits` for accurate reset countdowns
- Added prebuilt binary install instructions for Linux, macOS, and Windows with SHA-256 checksums
- Added `probe --force` and `update-pricing` subcommands to the Usage module
- Changed Waybar text semantics to show the highest window of paid Claude/Codex/Grok usage, session-anchored, escalating to Weekly at 80% or higher
- Changed documentation to reflect cross-platform support (Linux, macOS, Windows) with per-OS credential paths
- Changed credential storage to use OS-native keychains (secret-tool on Linux, security CLI on macOS, cmdkey/PowerShell on Windows)
- Changed process discovery to use platform-specific backends (ps/lsof on macOS, empty stubs on Windows)
- Fixed Grok billing endpoint to use the weekly SuperGrok usage pool instead of legacy monthly allotment

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
