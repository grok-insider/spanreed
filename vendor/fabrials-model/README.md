# fabrials-model

Shared DTO types for [Spanreed](https://github.com/grok-insider/spanreed) and
ai-relay. Published on crates.io (GitHub remote is private):

```toml
fabrials-model = "0.1.0"
```

## What’s in here

- **metric** — `MetricLine`, `ProviderOutput`, `ProbeView`
- **usage** — `UsageRecord` (one completed API call)
- **share** — community snapshot (`ShareSnapshot` schema v1/v2), economics, reset events

No I/O, no HTTP, no Tokio. Pricing and ledger files live in `fabrials-metrics`.

## License

MIT
