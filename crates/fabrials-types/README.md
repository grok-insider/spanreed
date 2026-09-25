# fabrials-types

Shared DTO types for [Spanreed](https://github.com/grok-insider/spanreed) and
ai-relay. Part of the `fabrials-libs` workspace; consumers vendor the workspace in-tree.

## What’s in here

- **metric** — `MetricLine`, `ProviderOutput`, `ProbeView`
- **usage** — `UsageRecord` (one completed API call)
- **share** — community snapshot (`ShareSnapshot` schema v1/v2), economics, reset events

No I/O, no HTTP, no Tokio. Pricing lives in `fabrials-pricing`; the JSONL ledger in the host store.

## License

AGPL-3.0-or-later (see the workspace `LICENSE`).
