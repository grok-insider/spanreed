# Changelog

All notable, user-facing changes to spanreed are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
