# fabrials-usage-import

Local usage import for Fabrials hosts: one `SessionReader` per local client
(registered in `READERS`, checked against `catalog.json`), the incremental
Codex parser, remote usage protocols, and normalization to
`ConsumptionRecord`. The Grok Build reader is public in `grok_build`.

Session readers are adapted from Tokscale (MIT, `src/formats/TOKSCALE-LICENSE`).
