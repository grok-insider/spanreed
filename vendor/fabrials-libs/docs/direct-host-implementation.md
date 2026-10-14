# Direct host transport and publication implementation

The direct host serves `/remote` behind an exact HTTPS origin configured with
`spanreed remote private-serve https://host.example`. TLS termination must preserve
Host. The browser origin defaults to exactly `https://desktop.grok.me`. The
qualification host opts into exactly `https://grok.fabrials.com` using
`--web-origin`; that configuration replaces the production origin rather than
adding another accepted origin. Other web origins are refused. A private network
such as Tailscale is still the recommended network boundary. The host binds
loopback, not a public interface.

## Local identity and lifecycle

`private-pair <origin>` obtains a single-use, 90-second nonce over the owner-only
control socket and generates a fragment-only pairing URL. Noise binds the browser
key, installation ID, pinned host key and a fresh server-generated channel ID.
No account, cloud relay, discovery request or ticket is needed. Browser pairing
revocation terminates that device's encrypted connection.

The direct identity in `direct.json` is stable across restarts. Changing its
installation ID, endpoint or web origin fails explicitly; there is no automatic
identity or origin migration. Legacy identities without a web origin retain the
production origin. `private-stop` requests shutdown while retaining pairings.
Service installation remains a separate explicit CLI action.

WebSockets use `fabrials.remote.v1`, at most 16 simultaneous connections, bounded
90-KiB messages, bounded queues, handshake and send deadlines, and heartbeat
expiry. Noise fragments logical messages into 60,000-byte encrypted pieces and
reassembles at most 2 MiB + 16 KiB. A text publication preview remains within that
logical bound. Public image bytes go host-to-publication-service, not inside a
large preview reply.

Socket-path and HTTPS-origin validation happen before taking the instance lock or
creating an installation identity. Windows pipe setup fails when an owner-only
DACL cannot be created. Windows filesystem identity uses volume and file index,
with reparse points rejected. These changes have not been qualified on Windows;
project capabilities remain disabled there.

## Optional direct notifications

Each host has its own VAPID key and owner-only subscription file. Browser
registrations have random independent `/push/<uuid>/` scopes, so computers with
different VAPID keys do not replace each other's subscriptions. Enabling requires
a user gesture and permission; configured status checks both host and browser.
Forgetting the profile removes its dedicated browser registration.

The host sends an encrypted generic `remoteActivity` payload, without session or
conversation details. Providers are explicitly allowlisted, DNS results must be
public, redirects and environment proxies are disabled, and transport retries
are disabled. There are four concurrent sends, 60 attempts per minute per host
and 10 per paired device. Subscriptions expire after at most 30 days and are
removed on revocation or provider 404/410. Invalid private notification state
disables this optional capability while leaving the direct host usable.

No real provider delivery or S24 notification run has been performed for this
implementation. Unit tests verify the RFC8291 payload by decrypting it with a
synthetic browser key; that is not evidence of provider delivery.

## Publications v2

Direct mode owns one shared publisher for all sockets and one bounded background
sync loop. It never loads or sends legacy publication grants. API paths are
restricted to `https://fabrials.com/api/spanreed/publications`; TLS, no redirects,
no environment proxy and no automatic HTTP retries apply.

The authorization URL keeps its binding in the fragment. The verifier stays in
owner-only host state; the exchange is bound to the browser principal, nonce,
origin and host key. Only a scoped 30-day publication grant reaches the host.
Neither grant nor verifier is returned to the PWA. A separate durable receipt
ledger records mutations; repeating an idempotency key does not repeat an
exchange or creation, and conflicting payloads are rejected. Unknown outcomes
require review instead of automatic replay.

`previewPublication` returns the exact text candidate, available normalized image
descriptors, explicit selection, totals and a digest. `createPublication` requires
that digest, an explicit `publishFutureText: true`, the unchanged selected image
hashes, an active controller epoch and an idempotency key. The host revalidates
the candidate before creating anything remotely. Images start unselected. Only
normalized PNG/JPEG from the same session can be selected, at most eight, with
the complete upload envelope bounded by 10 MiB. New images never join an existing
publication automatically. Missing or evicted selected media pauses publication.

Text history is verified as append-only. The host saves a SHA256 and length of
its complete JSONL prefix before upload. Rewriting, deleting, truncating or
reordering that prefix pauses synchronization with `publication_source_changed`;
resuming requires a new publication. Existing exported entries retain their
ordinal, including appended chunks of the same message. Only appended entries
receive new ordinals. The publication service owns canonical content deduplication
and the seven-day expiry decision; reads and unchanged content do not renew it.

The host avoids unchanged uploads, spaces revisions by at least one minute,
limits local active publications to five, and pauses on quota or transport
errors. It never automatically resumes a quota pause. Stopping first durably
prevents future uploads, then requests remote deletion. Only a server response
confirming both access revocation and key destruction is reported as deleted;
physical storage cleanup can remain pending. Failed deletion uses bounded retries
from one minute up to one hour, with at most eight publications considered in
one sync tick. An expired authorization leaves deletion pending until renewed.
The user can also remove the publication through the first-party account page.

Production API readiness gates remain closed. These client and host changes do
not activate public publication routes or establish operational deletion evidence.

## Verification

From this libs worktree:

```sh
cargo test -p fabrials-remote-host
cargo clippy -p fabrials-remote-host --all-targets -- -D warnings
```

The suite covers direct Noise pairing and revocation over a real socket, corrupt
optional Push state, per-device Push binding and encryption, PKCE and receipt
replay, real HTTP publication requests with synthetic credentials, ordinal
stability, append-only validation, quota pause, stopped uploads, restart ambiguity,
explicit normalized image selection and fragmented previews over 64 KiB.

The Spanreed CLI tests use a temporary source copy linked to this libs worktree;
no real consumer vendor tree was edited. Native Windows execution and cross-build
are unverified: the VM guest agent disappeared during a read-only query and Syl
marked transport tainted; the Nix daemon disconnected while downloading the
cross compiler. No snapshot was reverted and no host configuration was changed.
