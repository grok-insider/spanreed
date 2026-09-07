# Spanreed addons

Addons extend spanreed without a Rust ABI or a scripting engine.

The host speaks a small JSON protocol over stdin/stdout to:

- extra **providers** (merged into `list` / `probe` / `json` / waybar / `/usage`)
- extra **CLI prefixes** (`spanreed grok …`)
- optional **listeners** the addon process binds on loopback

The same operations exist as an in-process Rust trait so the first addon
(`grok-bridge`) can ship inside the `spanreed` binary and as
`spanreed-addon-grok-bridge`.

## Discovery

1. Compiled-in addons registered in `src/addons/mod.rs`.
2. Manifests `~/.config/spanreed/addons/<id>.toml`:

```toml
id = "grok-bridge"
command = "/usr/local/bin/spanreed-addon-grok-bridge"
enabled = true
```

3. Executables named `spanreed-addon-<id>` on `PATH`.

`spanreed addon list` shows all three sources.

## Protocol (v1)

The host sets `SPANREED_ADDON=1` and writes one JSON object (a single line),
then reads one JSON object.

```json
{"v":1,"op":"hello"}
```

```json
{
  "v": 1,
  "ok": true,
  "hello": {
    "id": "grok-bridge",
    "name": "Grok account bridge",
    "caps": {
      "providers": ["grok-bridge"],
      "commands": ["grok"],
      "listeners": true
    }
  }
}
```

| `op` | Request extra | Response |
|---|---|---|
| `hello` | — | `hello` |
| `detect` | `provider` id | `{ "detected": true }` |
| `probe` | `provider` id | `output` = `ProviderOutput` |
| `command` | `argv` (without `spanreed`) | `stdout` / `stderr` / `code` |
| `shutdown` | — | `{ "ok": true }` |

Rules:

- Never panic; probe failures are `ProviderOutput` error badges.
- Do not log tokens.
- Listeners bind loopback only.
- The host times out slow addons so Waybar stays snappy (cached last-good probe).

## First-party driver: Grok

Identities live on the **host**, not in the plugin:

```
spanreed account add grok --name heavy     # device-code inside spanreed
spanreed account import grok --name work   # migrate ~/.grok/auth.json
spanreed account ls
spanreed account use grok/heavy            # default fabric route
spanreed account login grok/heavy          # re-auth
```

The capture service **is** the fabric (`127.0.0.1:18736`). Path selects
upstream and account; each captured `usage` is stamped with `account_id`:

- `/v1/…` — Grok Build + **active** account
- `/acct/<alias>/v1/…` — that SuperGrok account
- `/xai/v1/…` — OpenCode → api.x.ai

`GROK_CLI_CHAT_PROXY_BASE_URL=http://127.0.0.1:18736/acct/heavy/v1`

`spanreed grok …` still forwards, then prints a deprecation note.

Canonical ids are numbered (`heavy-1`, `premium-plus-1`). `--name work` is a
nickname, not the id. `spanreed account alias grok/heavy-1 work` adds more.
`/acct/work/v1` and `/acct/heavy-1/v1` are the same secret; ledger stamps
`grok/heavy-1`. X Premium and X Premium+ are different slugs.

`spanreed account autosteer` (default on with ≥2 Grok accounts) switches the
**default `/v1`** route when the active pool is at 100%. `/acct/<alias>/…`
never moves. Pick order: reset <12h first, then better plan, then higher
used % (finish the week).
