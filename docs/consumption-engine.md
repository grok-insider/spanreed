# Consumption engine

Spanreed reads client usage directly; Codex does not need to pass through the proxy.
Quota probes, recorded client consumption and proxy traffic are separate measurements.
The Usage screen and `spanreed usage --client codex --days 31` query the same local store.

## Architecture

- `fabrials-core::usage`: validated records, token subsets, provenance and report contracts.
- `fabrials-providers::usage`: pure format normalization and read-only remote protocols.
- `fabrials-runtime::local_usage`: SQLite transactions, source checkpoints, stable identities and projection revisions.
- `src/usage`: host discovery, credentials, imports, pricing and queries.
- `@fabrials/ui`: shared local and synchronized consumption components.

Input includes cache reads/writes; output includes reasoning. Subsets must not be added twice.
Unknown models or cache rates remain unpriced. Reported costs and list-price estimates are
identified on each record. Subscription fees and remaining quotas are not inferred from tokens.
Session/account summaries and inferred timestamps remain outside daily charts.
Queries select records by their observation/start time; aggregate amounts are never prorated.

## Sources

The catalog and individual MIT reader ports are pinned to Tokscale
`0d621cae343af9b057fb4a38c84d089524ac4378`. There is no Tokscale executable or crate dependency.
Original notices and fixture tests are retained under `formats/TOKSCALE-LICENSE`.
These are reader implementations tested against fixtures, not claims of live account qualification.
Codex additionally has an incremental parser checked against five immutable real metadata samples.

Use Usage → Source settings to add absolute roots when an installation uses a custom layout.
`spanreed usage sources` lists the effective configuration. Each reader accepts its native format;
renaming unrelated JSON files does not make them supported. A source with no recognized usage
can be empty or a newer unsupported schema; inspect source status and the client version.

| Client | Default relative location | Collection |
| --- | --- | --- |
| OpenCode (`opencode`) | `opencode/storage/message` | Local files / database |
| Claude Code (`claude`) | `projects` | Local files / database |
| Codex CLI (`codex`) | `sessions` | Local files / database |
| Cursor IDE (`cursor`) | `.config/tokscale/cursor-cache` | Authenticated report / cache |
| Gemini CLI (`gemini`) | `tmp` | Local files / database |
| Amp (`amp`) | `amp/threads` | Local files / database |
| Droid (`droid`) | `.factory/sessions` | Local files / database |
| OpenClaw (`openclaw`) | `.openclaw/agents` | Local files / database |
| Pi (`pi`) | `.pi/agent/sessions` | Local files / database |
| Kimi CLI (`kimi`) | `.kimi/sessions` | Local files / database |
| Qwen CLI (`qwen`) | `.qwen/projects` | Local files / database |
| Roo Code (`roocode`) | `.config/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks` | Local files / database |
| Kilo Code (`kilocode`) | `.config/Code/User/globalStorage/kilocode.kilo-code/tasks` | Local files / database |
| Mux (`mux`) | `.mux/sessions` | Local files / database |
| Kilo CLI (`kilo`) | `kilo/kilo.db` | Local files / database |
| Crush (`crush`) | `crush/projects.json` | Local files / database |
| Hermes Agent (`hermes`) | `state.db` | Local files / database |
| Copilot CLI (`copilot`) | `.copilot/otel` | Local files / database |
| Goose (`goose`) | `goose/sessions/sessions.db` | Local files / database |
| Codebuff (`codebuff`) | `projects` | Local files / database |
| Antigravity (`antigravity`) | `antigravity-cache/sessions` | Local language server / cache |
| Zed Agent (`zed`) | `zed/threads/threads.db` | Local files / database |
| Kiro (`kiro`) | `.kiro/sessions/cli` | Local files / database |
| Trae (`trae`) | `trae-cache/sessions` | Authenticated report / cache |
| Warp (`warp`) | `warp-cache` | Authenticated report / cache |
| Cline (`cline`) | `.config/Code/User/globalStorage/saoudrizwan.claude-dev/tasks` | Local files / database |
| Gajae-Code (`gjc`) | `sessions` | Local files / database |
| Grok Build (`grok`) | `sessions` | Local files / database |
| Jcode (`jcode`) | `sessions` | Local files / database |
| Command Code (`commandcode`) | `.commandcode/projects` | Local files / database |
| MiMo Code (`micode`) | `mimocode` | Local files / database |
| Antigravity CLI (`antigravity-cli`) | `antigravity-cli/conversations` | Local files / database |
| Junie (`junie`) | `.junie/sessions` | Local files / database |
| ZCode (`zcode`) | `.zcode/projects` | Local files / database |
| OpenCodeReview (`opencodereview`) | `.opencodereview/sessions` | Local files / database |
| CodeBuddy (`codebuddy`) | `.codebuddy/projects` | Local files / database |
| WorkBuddy (`workbuddy`) | `.workbuddy` | Local files / database |
| Devin CLI (`devin-cli`) | `devin/cli/sessions.db` | Local files / database |
| Devin Desktop (`devin-desktop`) | `Library/Application Support/Devin/User/acp-events` | Local files / database |
| Senpi (OmO Native) (`senpi`) | `sessions` | Local files / database |
| Augment Code (`augment`) | `.augment/sessions` | Local files / database |
| Kimchi (`kimchi`) | `sessions` | Local files / database |
| Reasonix (`reasonix`) | `stats` | Local files / database |
| Prime Agent (`prime-agent`) | `sessions` | Local files / database |
| Freebuff (`freebuff`) | `projects` | Local files / database |
| Cherry Studio (`cherrystudio`) | `CherryStudio/.claude/projects` | Local files / database |
| DeepSeek Harness (`dsh`) | `sessions` | Local files / database |
| MiniMax Code (`mcode`) | `headless/mcode` | Local files / database |
| Fx (`fx`) | `.fx/sessions` | Local files / database |
| Oh My Pi (`omp`) | `.omp/agent/sessions` | Local files / database |
| LM Studio (`lmstudio`) | `server-logs` | Local files / database |
| Unsloth (`unsloth`) | `studio.db` | Local files / database |
| Hindsight (`hindsight`) | `usage` | Local files / database |

## Connections and privacy

Cursor, Trae and Warp use explicit credentials entered in Usage → Connections, stored in a
private local file. CLI automation can send `{client, account, credential}` JSON on standard
input to `spanreed usage connect`; avoid putting credentials in shell arguments or history.
`spanreed usage disconnect CLIENT` removes that connection and its imported remote report.
Antigravity uses its running local language server. Requests are read-only, bounded and directed
to fixed origins. Failed or incomplete collection retains the previous snapshot and reports an error.
These remote protocols are fixture-tested; live account qualification remains pending.

Only normalized usage metadata is persisted. Prompts, responses and provider credentials are not
uploaded by consumption synchronization. Projects and sessions remain local. Sources must be
explicitly selected in private synchronization settings, independently of public metrics consent.
V2 replaces the owner/device/source projection with a monotonic revision, including corrections
to zero. The authenticated server revision is consulted to recover after a local database rebuild.
Old v1 writers cannot overwrite a v2 projection. The hosted UI selects one device at a time to
avoid adding copied histories across machines. Quotas and hosted account authorization remain separate.

## Operational limits

Files above 1 GiB, invalid SQLite headers and inaccessible sources report errors. Complete
Codex lines are checkpointed; a partial final line is retried. Malformed lines retain known records
and mark the source partial. Other readers use replacement imports because earlier records or
sibling files can change. Some legacy formats cannot distinguish an empty history from an
unrecognized schema. Initial import of a large history can take time; subsequent unchanged files
are skipped. Explicit refresh rereads sources.

Linux is the qualification target. macOS/ARM remains deferred until a contributor with real Mac
hardware can validate discovery, credentials, packaging and the native GUI. Windows signing is deferred.
