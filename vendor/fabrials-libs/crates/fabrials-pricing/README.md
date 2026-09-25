# fabrials-pricing

Price tables and public list-price cost for `HopRecord`. Hosts supply the
price layers through `PricingSource` (embedded snapshot, remote refresh,
published overlays, user override); published list prices that upstream
lacks live as data in `src/pricing-overlays.json`.

Part of the `fabrials-libs` workspace. Depends on `fabrials-types`.
