# Native Remote notifications experiment

Disabled by default. Enable only the candidate with `SPANREED_REMOTE_EXPERIMENT_PUSH=1`, together with the separate backend and Portable experiment switches. The production account API remains fixed. `Notifications::with_loopback_api` is an acceptance constructor restricted to loopback HTTP; a browser cannot choose an account endpoint.

## Authority and protocol

Managed Noise reports `pushNotifications` in `getRemoteCapabilities`. The private local transport does not gain this capability. Managed operations are:

| Operation | Result |
| --- | --- |
| `getNotificationStatus` | `{outcome:"notificationStatus",enabled,registrations:[{id,expiresAt,generation}]}` for the authenticated pairing |
| `configureNotifications {id,bindingToken,generation}` | `{outcome:"notificationsConfigured",id,active:true,expiresAt,generation}` |
| `disableNotifications` | `{outcome:"acknowledged"}`; removes only this pairing's native bindings |

Mutations require an idempotency key. They affect only the requesting pairing and do not require the control lease. These operations are intercepted before the agent journal: the binding token never enters journal, logs, durable state or error strings. Status does not disclose another pairing's registration identifiers. A repeat of a locally active matching ID and generation returns the existing binding without resubmitting its token; this does not assert provider delivery or repair a registration independently removed at the backend.

Binding currently holds the notification-state mutex across credential refresh and the bind HTTP request, bounded to eight seconds in total. During that window, other pairings' notification status and disable requests wait; pairing revocation itself uses the independent owner control plane. The PWA must reconcile native status with its account subscription list because a locally cached binding can outlive a backend deletion until a later 403/410 response.

The native host derives `hostId` from its local registration and `deviceId` from the authenticated `RemoteAccess`, never from browser fields. It posts to `/api/spanreed/remote/push/subscriptions/{id}/bind` with `{bindingToken,hostId,deviceId,generation}` using its installation credential. The backend validates the generation and account/installation relationship. The returned ID, generation, active state and bounded expiry must match before the host accepts the binding.

`remote-notifications-{hostId}.json` is atomically replaced with owner-only permissions. It stores registration ID, pairing principal, generation, expiry and revocation bookkeeping. It contains no subscription endpoint, encryption key, installation bearer or binding token. At most 64 records are accepted, and the reader rejects non-files, oversized state and broad Unix permissions. A restart loads those bindings, rechecks pairing, and starts at the current journal cursor without replaying old notifications.

## Delivery and revocation

The worker lives with the host, independently of the PWA or a Noise channel. A new pending permission or observed `running → idle` transition requests a generic notification. Repeated pending-permission snapshots are deduplicated; reconnecting/interrupted session status does not masquerade as a completed turn. Events observed in the same worker pass are coalesced into one generic attention event per subscription, with a fresh random 128-bit event ID and no conversation, title, command or file content.

The worker allows four concurrent HTTP requests, a five-second credential-refresh deadline, a three-second HTTP deadline, sixty send attempts per host per minute and thirty per local subscription per minute. The backend applies its own stricter account/family limits. Slow or unavailable services do not create an outgoing durable queue. An uncertain send, a rate-limit rejection or a missed journal window is not replayed automatically. Provider acceptance is not proof of device delivery.

The worker checks that the local binding remains enabled and the pairing remains valid after credential refresh and before sending. HTTP 403 or 410 retires the local record. Actual pairing revocation marks bindings inactive before any further sends and calls `/push/devices/revoke`; failed revocations are retried with a five-second floor. This creates the backend's durable pairing tombstone. Disabling notifications is separate: it removes the native binding and the browser removes its account registration; it does not tombstone the still-valid pairing. An already-started provider request cannot be withdrawn. Backend and service-worker revalidation cover their own authority boundaries.

## Acceptance fixture

The ignored `network_fixture::managed_network_fixture` test accepts its owner-only directory through `FABRIALS_REMOTE_HOST_QA`. Its existing `host-input.json` may add `pushApi` (loopback HTTP only), `installationBearer`, and `timeoutSeconds` between 30 and 7200. It runs the real managed Noise channel and notification worker against the local backend and relay. `host-info.json` does not expose the installation bearer. Optional `seedSyntheticSession:true` requires `GROK_HOME=<fixture-directory>/synthetic-grok`, creates two synthetic directories inside the fixture, authorizes only that root, enrolls one workspace and opens a clearly named synthetic session. Its isolated `summary.json` and `updates.jsonl` contain two explicitly synthetic messages for listing, snapshot, draft, speech and share-preview tests. It also enables the projects, device-panel and control-transfer experiments for this fixture. There is no Grok child or fake ACP adapter; prompts are not answered as if a model had run. The live synthetic session can be selected and its snapshot replayed. Reopening it after closing requires a real agent and is outside this fixture.

Write `notification-command.json` atomically with `{id,kind:"permission"}` or `{id,kind:"turn"}` to emit a synthetic native event. `{id,kind:"revoke",deviceId}` revokes a synthetic pairing. The fixture consumes the file and writes `notification-command-receipt.json` with `{id,emitted:true}`; this only confirms native emission. `{id,kind:"pair"}` generates a fresh owner nonce and writes `{id,hostId,hostPublicKey,nonce}` to the same private receipt instead. The receipt is owner-only and its nonce must not enter logs. Create `stop` to stop and join the fixture workers. Keep the backend and relay processes alive throughout a device-delivery test.

Publication is unavailable in this managed fixture: its test-only preview mode rejects `startShare` with `publication_unavailable_in_synthetic_fixture` and disables all publication synchronization, including loaded grants. `host-info.json` declares `publicationAvailable:false`. Preview still reads the synthetic history locally; no share content can be sent to the production endpoint. Real publication is qualified separately.

Unit and Noise/HTTP contract tests cover identity derivation, exact bind fields, minimal payload, closed-channel behavior, dedupe, explicit default-off capability, persistence, own-pairing disabling, uncertain outcomes, revocation during credential refresh and bounded attempts. These tests use a synthetic HTTP service. They cannot qualify provider delivery, foreground/background Android behavior, service-worker cookie access, battery restrictions or human utility. Those gates require the separate real backend and device acceptance.
