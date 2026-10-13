# Shared host execution and reconnect contract

The shared Grok host accepts simultaneous writers through command protocol 4.
Pairing, command responses and event envelopes retain protocol version 2. The
HTTP event query and Noise subscribe body advertise `protocolVersion: 4`.
The shared host has no control lease or takeover. Older browsers see it as
read-only; isolated hosts retain their existing lease protocol.

## Execution and recovery

Each conversation has one durable FIFO and at most one active host prompt.
Different conversations may execute concurrently. The host limits open sessions
to 8, queued prompts to 16 per conversation, command concurrency to 16 and queued
text plus distinct normalized image bytes to 100 MiB. Remote connection limits
and per-transport message limits remain independently enforced.

Scheduling requests are coalesced per conversation into at most eight drainers.
A dirty flag preserves a wakeup arriving while a drainer is finishing. Saturation
sets one rescan flag rather than allocating another waiting task per command.
The global command concurrency limit remains 16.

Receipts bind an idempotency key to its device principal, session and complete
operation payload. A repeated request returns its existing receipt. A reused key
with another payload or operation class is rejected. The enqueue response means
the queue entry was persisted, not that inference completed. Clients query
`getCommandStatus` after execution invalidations and reconnects.

Stopping names the exact active turn. It pauses the FIFO by default and preserves
pending entries. Interrupting also compares the queue revision before atomically
inserting the replacement. The transport uses Grok's `_meta.promptId`,
`rewindIfNoOutput: true` and `cancelSubagents: false`. A stop received before the
prompt write is deferred until the write is confirmed. Repeating a stop cannot
cancel a successor or leave a cancellation fence behind.

Cancellation is advertised only after the running binary reports the qualified
Grok Build version 1.0.52. The executable qualification is maintained in the
portable repository's `docs/evaluation/acp-runtime-2026-10-14.json`. This host
does not cancel externally started terminal turns: they have no host-owned turn
identifier. Shared remote shell prompts are explicitly refused.

Interrupted or otherwise uncertain work is never replayed automatically.
Restart preserves queued entries, pauses their queues and marks active receipts
`needsReview`. `reviewUncertainTurn` reloads the upstream conversation, flushes
the event projection, requires confirmed idle state and marks the receipt
`reviewed` without resending anything. The queue stays paused. Revoking a device
holds its pending entries while leaving its already active turn untouched.

## Event consistency

The journal holds a bounded materialized snapshot. Reconnecting a shared client
receives its own snapshot without mutating another client's stream, invoking
upstream `session/load`, or advancing the event sequence. Historical replay during
an initial load is staged until the load response and event barrier complete;
then one atomic snapshot is published. Live deltas received during that interval
are included. Image replay during restoration waits for bounded decoding before
the event barrier, including when the live session has not been inserted yet.
Live image decoding retains the originating message even when it
finishes after a subsequent text reply.

Every event envelope includes `observationEpoch`. Durable hosts increment and
persist it at startup before serving requests; memory-only test hosts use 0.
Unknown journal versions, invalid state and an exhausted JavaScript-safe counter
refuse startup. Event sequence numbers restart within each epoch. Clients compare
`(observationEpoch, eventSequence)` lexicographically for stored observations.
This requires no clock ordering and no filesystem write for each text delta.

Private snapshots begin with `replayReset`; intermediate frames have sequence 0.
Their final `workspacesChanged` contains the captured watermark, which may also
be 0. All frames carry the same epoch. A cumulative acknowledgement never removes
events needed by another connected writer; the bounded journal controls retention.

## Windows filesystem boundary

Windows directory enrollment resolves each opened handle with
`GetFinalPathNameByHandleW`, verifies component containment under the opened root,
and retains volume/file identity. Reparse points and root-prefix collisions are
rejected. Windows-only tests exercise a real junction, a replaced child, root
prefix collisions and principal ownership of directory handles.

These Windows paths have not been compiled or executed in this Linux run; no
Windows Rust standard library is installed here. The Windows remote-project
capability remains disabled pending qualification. Linux passing tests do not
qualify the Windows implementation.

## Verification

The deterministic ACP fixtures do not contact an inference provider. Tests cover
two writers, stale turn and revision rejection, enqueue/close serialization,
permission races, revoked origins, receipt persistence, prompt-write cancellation
ordering, ambiguous recovery, private snapshots and initial replay/live ordering.
The HTTP/socket integration executes 50 alternating enqueue/remove pairs with
both writers connected and a two-second p95 ceiling. This is a local regression
gate, not a WAN performance claim.

`cargo clippy -p fabrials-agent-host -p fabrials-remote-host --all-targets --locked
-- -D warnings` passed after all changes. `cargo test -p fabrials-agent-host -p
fabrials-remote-host --locked` passed 458 agent-host unit tests, 77 agent-host
integration tests and 73 remote-host tests. Three explicitly opt-in remote-host
experiments remain ignored. Cargo used two build jobs throughout.

The direct browser fixture seeds the same synthetic transcript into its journal
that it writes to its synthetic Grok home. `DIRECT_BROWSER_QA_ACP=1` additionally
launches the neighboring `fake_multiwriter_agent` executable; it does not run
real Grok inference. The fixture advertises `synthetic/custom` and
`synthetic/alternate`, with synthetic low/high effort values. Exact-turn
cancellation remains disabled because the fixture is not the qualified CLI.

Controls live inside `DIRECT_BROWSER_QA_DIRECTORY`, which must remain private:

- Write the session ID to `release-turn` to complete its fake active prompt.
- Write it to `disable-models` to publish an empty selectable catalogue.
- Create `pair-next` to write a fresh pairing object into `pair-next.json`.
  Remove the previous JSON before generating another one. Pairing material must
  not be copied into logs or evidence.
- Create `stop` to terminate the fixture and its synthetic agent.

The executable smoke check verified model/effort changes, prompt completion,
empty catalogue, reconciled direct startup and a distinct second nonce pinned to
the same host key. Browser qualification is recorded by the coordinating release
report.
