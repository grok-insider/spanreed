# fabrials-pricing

Price tables and public list-price cost for `HopRecord`. Hosts supply the
price layers through `PricingSource` (embedded snapshot, remote refresh,
published overlays, user override); published list prices that upstream
lacks live as data in `src/pricing-overlays.json`.

Part of the `fabrials-libs` workspace. Depends on `fabrials-types`.

## OpenAI Sol / Luna list prices

Standard USD per million tokens, verified against the official model pages:

| Model | Input | Cache read | Cache write | Output |
| --- | ---: | ---: | ---: | ---: |
| [gpt-6.1-sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol) | $2 | $0.10 | $2.50 | $10 |
| [gpt-6-sol](https://developers.openai.com/api/docs/models/gpt-6-sol) | $2 | $0.20 | $2.50 | $10 |
| [gpt-6-luna](https://developers.openai.com/api/docs/models/gpt-6-luna) | $0.10 | $0.01 | $0.125 | $0.50 |

For prompts **greater than 272,000 input tokens**, the entire request uses
2× input/cache rates and 1.5× output rates. Exactly 272,000 retains base rates.
`Usage` counts prompt tokens as input + cache read + cache create (disjoint
categories); `HopRecord.input_tokens` already includes cached input.
`HopRecord` has no cache-write counter; cache-write costs are available through
`PricingMap::cost` and `exact_cost` with `Usage.cache_create`.

The optional JSON field `long_context_threshold_tokens` selects this strict,
full-request threshold per model. Higher rates keep the existing
`*_above_200k_tokens` JSON names and `Pricing.*_above_200k` fields for compatibility;
when a threshold is present, those names no longer imply a 200k boundary.
Without this field, legacy behavior remains unchanged: `PricingMap` applies
progressive 200k tiers per category, and `HopRecord` reprices the full request
at input ≥200k. Missing higher rates retain the corresponding base rate.

These verified rates are published overrides, after the remote snapshot and
before user overrides. Hosts can replace overlays through `PricingSource`.
Normalized provider-qualified aliases retain the same rates and threshold;
strict lookup does not guess unknown or dated model IDs. Strict costs still
refuse unpublished cache rates. Regional, Batch, Flex and Fast modifiers are
not applied by these Standard token prices.

## Model resolution

`find` accepts an exact table key or its normalized spelling (lowercase,
provider prefix removed, `:`/`@` replaced with `-`). Explicit alias entries
from any price layer remain recognized; similar prefixes or suffixes are not
aliases and never inherit prices.

The only fallback is a known normalized base ID followed by `-YYYY-MM-DD` or
`-YYYYMMDD`, with a valid Gregorian calendar date. This is format validation,
not confirmation that a provider published that snapshot. Partial dates such
as `-0309`, arbitrary suffixes, and reverse matches from a dated row to an
unpriced base return `None`. `exact`/`exact_cost` still require an explicit
exact or normalized entry, including for dated IDs.
