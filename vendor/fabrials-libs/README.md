# fabrials-libs

Shared Rust crates for Spanreed (local) and ai-relay (hosted). One Cargo
workspace, one version line, never published to crates.io (`publish = false`).

| Crate | Purpose |
|-------|---------|
| `fabrials-types` | Data types and portable contracts: hop and consumption records, metric lines, share snapshots, observations, provider capabilities, migration |
| `fabrials-metrics` | Price table, list-price cost, JSONL ledger, SSE usage |
| `fabrials-share` | Community snapshot economics |
| `fabrials-accounts` | Account registry and autosteer scoring |
| `fabrials-providers` | Provider protocols: OAuth and device flows, Grok CLI auth, usage parsers, local session readers |
| `fabrials-runtime` | Proxy engine: listener, routing, forwarding, hop and history stores; defines the `Provider` port |
| `fabrials-upstreams` | Provider adapters that implement the engine's `Provider` port |

Dependencies point one way: `types` at the bottom; `metrics`, `accounts`
and `providers` above it; the engine (`runtime`) knows no provider;
`upstreams` depends on the engine and on `providers`.

## Consumers

Spanreed and ai-relay carry this workspace in-tree at `vendor/fabrials-libs`
(`git subtree`) and depend on the crates by `path`. Change code here first,
then pull it into each consumer with its `scripts/sync-fabrials-libs.sh`; the
consumer's CI rejects an in-tree copy that differs from the recorded revision.

Crate manifests spell out edition, license and path dependencies instead of
inheriting `workspace.*` fields: consumers load them as plain path
dependencies from inside their own workspace, where that inheritance does not
resolve.

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## License

AGPL-3.0-or-later (`LICENSE`). Session readers adapted from Tokscale keep
their MIT notice in `crates/fabrials-providers/src/usage/formats/TOKSCALE-LICENSE`.
