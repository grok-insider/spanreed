# fabrials-libs

Shared Rust crates for Spanreed (local) and ai-relay (hosted). One Cargo
workspace, one version line, never published to crates.io (`publish = false`).

| Crate | Purpose |
|-------|---------|
| `fabrials-core` | Portable contracts: provider capabilities, observations, migration |
| `fabrials-model` | DTOs: metric lines, usage records, share snapshots |
| `fabrials-metrics` | Price table, list-price cost, JSONL ledger, SSE usage |
| `fabrials-share` | Community snapshot economics |
| `fabrials-accounts` | Account registry and autosteer scoring |
| `fabrials-oauth-grok` | Grok CLI auth blobs and capture headers |
| `fabrials-providers` | Provider protocols, OAuth flows, and local session readers |
| `fabrials-runtime` | Proxy engine: listener, forwarding, upstream adapters, stores |

## Consumers

Spanreed and ai-relay carry this workspace in-tree at `vendor/fabrials-libs`
(`git subtree`) and depend on the crates by `path`. Change code here first,
then pull it into each consumer with its `scripts/sync-fabrials-libs.sh`; the
consumer's CI rejects an in-tree copy that differs from the recorded revision.

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## License

AGPL-3.0-or-later (`LICENSE`). Session readers adapted from Tokscale keep
their MIT notice in `crates/fabrials-providers/src/usage/formats/TOKSCALE-LICENSE`.
