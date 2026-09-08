# Local and hosted workspaces

Spanreed opens the local workspace without requiring a Fabrials account. The
workspace selector switches to hosted ai-relay. Settings → Connect to Fabrials
starts a device authorization, displays its code, opens the approval page, and
checks for approval in the native host. Signing in to the website alone does
not approve an installation. Pending authorization survives window restarts.

Linking neither uploads provider credentials nor enables metrics publication
or history synchronization. Each is a separate choice. Hosted provider login
explicitly creates credentials in ai-relay, whereas local provider login keeps
them on the machine.

Disconnect this installation revokes its server-side installation grant and
refresh tokens, then clears local state. Browser sessions and other devices
remain connected. If revocation cannot reach Fabrials, Spanreed reports an
error instead of claiming the installation was revoked.

Native requests use the versioned `/ui/native/v1/` bearer API. The token stays
in Rust; the renderer selects from a generated operation enum, not arbitrary
URLs or headers. The relay checks the live installation session on each
request and applies the same owner checks as its browser application. Proxy
keys and browser cookies cannot substitute for an installation token. Operator
pool administration is excluded from the native API.

The initial hosted origin is `https://ai.fabrials.com`. Supporting arbitrary
self-hosted origins needs an explicit trust and identity-binding flow; changing
the renderer cannot redirect the native credential transport.

## Linux panel and notifications

`spanreed profile install eww-panel --output ~/.config/eww-spanreed` installs a
standalone Eww window. Run `sh ~/.config/eww-spanreed/spanreed-panel toggle`;
use `open` or `close` for an explicit action. `SPANREED_PANEL_HEIGHT` defaults
to 720 pixels and `SPANREED_PANEL_MONITOR` to monitor index 0; a connector name
such as `DP-1` is also accepted. Eww and the
Spanreed CLI must be on PATH. `spanreed widget panel` reads only
`127.0.0.1:6736/usage`, so opening the panel does not log in, probe providers,
or present hosted data as local data. The launcher suspends polling when closed.
Home Manager can install the launcher with `programs.spanreed.eww.enable = true`
and `programs.spanreed.serve.enable = true`; bind Waybar's click action to
`spanreed-panel toggle`.
Bindings can use the absolute launcher path from `programs.spanreed.eww.package`.

Opt-in reset alerts also run after the daemon's provider refresh, independent
of an open GUI. They share the durable delivery store with GUI checks. Linux
requires `notify-send` and a desktop notification service. Windows uses the
installed application's notification identity. GUI delivery and background delivery
with process-restart deduplication were verified in the Windows 11 QA VM.
macOS/ARM qualification and Windows signing remain deferred.

## Private synchronization and publication

Settings requires a linked account, explicit private-sync consent and selected
sources before upload. Selections and last-run status are bound to the Fabrials
identity; switching accounts requires selecting sources again. Compatible
sources supply the quotas and request metadata they actually report. Unknown
quota or unavailable expiry details are not inferred from another account.

A durable local change feed scans retained history incrementally, including late
records. Outbox acknowledgements survive restart and retries are deduplicated on
the server. Downloaded history has its own inbox and cursor; it is never added
to local billable totals or uploaded again. Private history is paginated in both
workspaces and the ai-relay website. Retained local change-feed/inbox/outbox data
uses disk space proportional to retained observations; there is no silent
history truncation or automatic deletion of unacknowledged data.

Public metrics publication is independent. Settings can publish manually and
manage the daily schedule after publication consent and account linking. The
schedule resolves an installed CLI on PATH rather than running the GUI as a
background publisher. Disabling consent stops future transfers; it does not
remove data already delivered to the hosted account.

Refresh tokens and pending device proofs remain in private native files. A
rotation journal recovers a replacement durably received before interruption;
a lost response before that point requires reconnecting. Account on
fabrials.com lists installation grants, including ones that can no longer be
reached from the original machine, and can revoke them individually.

## Current implementation status

These changes are under implementation and have not been released. Successful
unit tests or builds do not establish live authorization, complete remote
workflow parity, or Linux/Windows qualification. The execution ledger is
`../docs/connected-workspaces-implementation.md` in the Fabrials workspace.
