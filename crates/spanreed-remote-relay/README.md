# Spanreed Remote Relay

This is a standalone, unpublished Rust service. It routes encrypted browser-to-host frames; it has no Noise keys, session storage, command queue, inference credentials or access to host files. The account control plane is the Fabrials web service.

```sh
cargo test --locked --manifest-path crates/spanreed-remote-relay/Cargo.toml
cargo clippy --locked --manifest-path crates/spanreed-remote-relay/Cargo.toml --all-targets -- -D warnings
cargo build --release --locked --manifest-path crates/spanreed-remote-relay/Cargo.toml
```

Configuration:

| Variable | Meaning |
| --- | --- |
| `SPANREED_REMOTE_RELAY_SECRET` | Required, at least 32 bytes, only in this service and web. Authenticates consumption/introspection; never give it to a host or browser. |
| `SPANREED_REMOTE_CONTROL_URL` | Default `https://fabrials.com`. HTTPS is required, redirects disabled, no embedded credentials, query or fragment. |
| `SPANREED_REMOTE_RELAY_BIND` | Default `127.0.0.1:8791`. Put TLS ingress at `remote-relay.fabrials.com` in front of this private listener. |
| `SPANREED_REMOTE_CONNECTS_PER_MINUTE` | Default 30 per source IP. The relay deliberately ignores forwarded IP headers. A reverse proxy aggregates peers: set a suitable limit and apply per-client admission at the trusted ingress. |

`GET /health` is process liveness only; it does not certify control-plane availability. `GET /ready` returns 200 only after a successful authenticated control-plane readiness probe less than ten seconds old, and returns 503 during shutdown. Both outcomes disable caching. Probes run serially every five seconds with a three-second request deadline; failed probes invalidate the previous success. The web probe uses a separate one-connection pool with two-second connection and SQL deadlines, checks the exact migration ledger and rejects a read-only database. It never creates or migrates tables. Apply migrations before readiness-gated ingress; an uninitialized schema is intentionally unready. `GET /connect` upgrades WebSocket connections. Browser Origin must be exactly `https://remote.fabrials.com`; native hosts omit Origin. Query parameters are rejected so credentials cannot accidentally enter URL logs. Reverse proxy logs must not record WebSocket bodies or Authorization.

The first text frame is `{"type":"authenticate","ticket":"..."}` within five seconds. The ticket is consumed once by web; admission also introspects its current account/host/generation. A host receives `{"type":"ready"}`. A browser receives `{"type":"ready","channelId":"..."}` and its host receives `{"type":"open","channelId":"..."}`.

Subsequent frames are `{"type":"data","channelId":"...","payload":"..."}`, where payload is standard base64 of one opaque Noise frame. A sender may close its channel using `{"type":"closed","channelId":"..."}`. The host receives browser disappearance as `{"type":"closed","channelId":"...","reason":"disconnected"}`. Channel IDs are random and checked against account, host, generation and exact socket identity. A browser cannot select another browser's channel. A host reconnect replaces its previous socket and closes all old browser channels; the old socket's teardown cannot remove the replacement.

Limits: 90 KiB JSON/frame; 65,535 decoded bytes; four outbound frames queued per socket; 64 browser channels per host; 1,024 total sockets including authentication; bounded admission map. Data sends wait up to five seconds for bounded queue capacity outside the routing mutex; slow receivers then close on backpressure. A stalled browser closes only its own channel. Send timeout is five seconds. Heartbeat ping is every 15 seconds with 45 seconds without a received frame as expiry. Every connection rechecks authorization every ten seconds; control-plane denial, timeout, malformed response or outage closes it. HTTP connect timeout is three seconds, whole request five seconds and response body at most 16 KiB. Nothing is replayed after reconnection.

The server currently runs as one replica. Host sockets are process-local, so load-balanced replicas without sticky host/browser routing are unsupported. Horizontal distribution, persistent telemetry, ingress and Coolify deployment are not included here. SIGINT and SIGTERM drain connections; container orchestration should allow ten seconds for shutdown. TLS/proxy, actual Postgres/S3, device browsers and integration with the native Noise connector must be qualified before enabling the public beta.

Offline tests cover account/host/channel isolation, identity replacement, generation checks, revocation teardown, queue pressure, frame schema and ciphertext bounds, channel/connection admission limits and shutdown. No live network or inference was used for those checks.

Authenticated host frames for an already closed channel are discarded, and host close notifications are idempotent. This covers ciphertext already queued when a browser disconnects; closing one browser must not tear down the host and its other channels. Optional inbound close reasons are accepted but never trusted or echoed.

The fragmented-message test sends 36 full-size ciphertext frames (over 2 MiB) to a deliberately delayed receiver and verifies ordered delivery while its queue stays at four.

Each socket has an independent bounded writer task. Waiting for the destination queue does not block flushing the source socket, preventing two directions of a fragmented exchange from deadlocking on each other’s queues.

## Local integrated acceptance

The canonical fabrials-libs runner `scripts/qualify-managed.ts` connects Portable's actual managed connector and Noise WASM to this relay and the native host over loopback sockets. It covers pairing, observer authorization, reconnect, native restart, device binding, account isolation, ticket replay/expiry and authenticated revocation. Its account login/ticket store and IndexedDB persistence are simulated; it does not qualify deployed HTTPS or OAuth. See `fabrials-libs/docs/managed-acceptance.md` for the command and exact boundaries.

The fixture `tests::managed_relay_qualification_fixture` is compiled only for tests. It requires `FABRIALS_REMOTE_RELAY_QA`, `FABRIALS_REMOTE_RELAY_QA_CONTROL_URL` and `FABRIALS_REMOTE_RELAY_QA_SECRET`, writes `relay-ready.json` under the private fixture directory and ends when that directory contains `stop`. The test-only control-plane client accepts explicit HTTP loopback addresses only. Production HTTPS validation is unchanged.

Host-requested channel close preserves queued ciphertext order and enqueues a terminal frame. A single five-second deadline covers queue admission, pending socket writes and final close; heartbeats cannot reset it. The browser writer sends the terminal frame before stopping when the peer keeps up. Draining channels still count against the host admission limit, and host disconnection, generation revocation and lost authorization abort them immediately.


## Operations evaluation

`qualify.ts` launches the test-only relay executable with a disposable loopback control plane and synthetic identities. Set `RELAY_QA_BINARY` to the built test executable, `RELAY_QA_ROOT` to a new absolute private evidence directory, and run `bun crates/spanreed-remote-relay/qualify.ts`. The default measured duration is 3,600 seconds, after a one-minute warmup; `RELAY_QA_DURATION_SEC` allows a shorter smoke run, whose status explicitly does not qualify the hour-long gate.

The harness records binary and source hashes, ten host sockets and one hundred browser sockets, exact account/host/browser echo identities, per-account and global revocation, a real control-plane TCP outage, recovery, channel admission, a paused raw TCP WebSocket receiver, forwarded-IP spoofing during a connection storm, bounded queues, process RSS and owner-controlled drain. A successful hour requires at least 570 RSS samples in both first and last ten-minute windows and no more than ten percent growth between their means. The exact initial and final RSS, measurements, request counts and cleanup receipt stay in the report; an incomplete interval cannot pass this gate.

These are real relay/WebSocket operations with synthetic control-plane responses and opaque base64 frames. They do not qualify OAuth, public TLS ingress, PostgreSQL outages, Noise encryption, inference, multi-replica routing or real device lifecycle. The forwarded-IP storm verifies spoof resistance and bounded admission for one TCP source. It does not verify fair per-user limits behind a reverse proxy: all clients behind that proxy share its TCP-source allowance unless trusted ingress enforces separate client limits. `/qa/stats` and its socket/queue counts exist only in test builds. Do not run performance comparisons concurrently with this harness.

Evaluation status (2026-10-13): the final 23 Rust tests and strict clippy passed. The complete operations run passed 29 checks with ten hosts and one hundred browsers, including 3,600.651 measured seconds after warmup, 364,150 verified echoes across the run and no duplicates or identity mismatch. Both ten-minute RSS windows contain 600 samples: means were 28,793 KiB and 25,826 KiB, a 10.3% decrease, satisfying the growth ceiling. The paused TCP receiver closed after 5.415 seconds; two rounds each reconnected one hundred browsers simultaneously. Of 1,100 unauthenticated upgrade attempts from the same TCP source with changing forwarded headers, 1,000 upgraded and 100 were limited; none consumed a ticket. Drain took 111 ms, the relay exited successfully and no pending tickets remained. Source and executable hashes stayed unchanged. The central relay receipt passes all nine acceptance checks for this scoped module. Concurrent development work was present, so this run qualifies stability, bounds and RSS; it does not qualify an isolated latency comparison or idle CPU.

The first and third smokes found two product defects. A burst of 54 browser disconnects filled the four-frame host queue with close notices and terminated the host and unaffected channels. After fixing that path, one hundred simultaneous authenticated reconnects exposed the same immediate-enqueue failure for Open notices. Both notifications now use bounded, awaited delivery outside the routing mutex. Open notification delivery completes before the browser reader starts, preserving Open-before-data and Ready-before-host-data order. Regression tests cover both bursts; the final fourth real smoke passed. Failed reports remain under `/tmp/grok-remote-evaluation-ter17my1/relay-smoke-1/` and `relay-smoke-3/`; the successful smoke report is `relay-smoke-4/report.json`. The completed hour is `relay-soak-1/report.json`, with central decisions in `relay-evaluation-result.json` and frozen source/binary identity in `relay-source-manifest.json` under the same private evidence root.

`qualify-rollback.ts` runs a frozen baseline executable, then the candidate, then the identical baseline behind one stable loopback TCP ingress. Set `RELAY_QA_BASELINE_BINARY`, `RELAY_QA_CANDIDATE_BINARY` and a new `RELAY_QA_ROOT`, then run it with Bun. The completed evaluation delivered twenty exact echoes per phase, reconnected after each replacement, and drained in 120/128/131 ms with zero leftover sockets or tickets. Both executable hashes stayed unchanged. This proves local relay binary replacement with synthetic identities and manual client reconnection; it does not prove web-database rollback, public TLS ingress, OAuth, native persisted state or deployment orchestration. Rolling back the browser-family web authentication implementation to its predecessor is independently unsafe for immediate family revocation.
