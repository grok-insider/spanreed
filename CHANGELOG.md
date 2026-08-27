# Changelog

All notable, user-facing changes to Spanreed are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
this project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
