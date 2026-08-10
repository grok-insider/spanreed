# Contributing to spanreed

Thanks for your interest. spanreed is a small, focused Rust project; the most
common contribution is **adding a new provider**. This guide covers local setup
and the add-a-provider walkthrough.

For the high-level architecture and module map, read [`AGENTS.md`](AGENTS.md)
first.

## Dev setup

You need a Rust toolchain (stable). On NixOS, `nix develop` drops you into a
shell with `cargo`, `rustc`, `rustfmt`, `clippy`, `rust-analyzer`, and
`libsecret`.

```sh
cargo build                 # debug
cargo build --release       # binary: target/release/spanreed
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
SPANREED_OFFLINE=1 cargo test
./target/debug/spanreed list
./target/debug/spanreed probe <id>
```

Run fmt, clippy, and tests before sharing a change.

## Git workflow

This repo uses **Model A**:

1. Branch from **`dev`**, open a PR into **`dev`**.
2. When a batch is ready to ship, open one PR **`dev` → `master`** (guard allows
   only `dev` or release-bot heads into `master`).
3. After merge to `master`, automation may open a **patch** Release PR
   (`release-plz-v*`). Deliberate **minor/major** bumps use the
   **Manual Version Bump** workflow (repo admins).

Never push directly to `master`. Do not hand-edit `CHANGELOG.md` outside a
Release PR.

## Adding a provider

A provider is one file implementing the `Provider` trait. Use an existing one as
a template:

- **Simplest** (env/file API key): `src/providers/zai.rs`
- **OAuth file + refresh**: `src/providers/codex.rs`
- **SQLite-backed token**: `src/providers/cursor.rs`
- **Local process discovery**: `src/providers/antigravity.rs`

### 1. Create `src/providers/<id>.rs`

```rust
use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;

const ID: &str = "example";
const NAME: &str = "Example";

pub struct Example;

impl Provider for Example {
    fn id(&self) -> &'static str { ID }
    fn name(&self) -> &'static str { NAME }

    fn detect(&self) -> bool {
        // Cheap, side-effect-free check: a file, env var, or running process.
        creds::env("EXAMPLE_API_KEY").is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let key = match creds::env("EXAMPLE_API_KEY") {
            Some(k) => k,
            None => return ProviderOutput::error(ID, NAME, "No EXAMPLE_API_KEY set."),
        };

        let resp = match Request::get("https://api.example.com/usage")
            .bearer(&key)
            .header("Accept", "application/json")
            .send()
        {
            Ok(r) => r,
            Err(e) => return ProviderOutput::error(ID, NAME, e),
        };
        if resp.is_auth_error() {
            return ProviderOutput::error(ID, NAME, "API key rejected.");
        }
        if !(200..300).contains(&resp.status) {
            return ProviderOutput::error(ID, NAME, format!("HTTP {}", resp.status));
        }
        let data = match resp.json() {
            Some(d) => d,
            None => return ProviderOutput::error(ID, NAME, "invalid response"),
        };

        let pct = data.get("used_percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let lines = vec![MetricLine::percent("Session", pct, None)];
        ProviderOutput::new(ID, NAME, lines)
    }
}
```

### 2. Register it in `src/providers/mod.rs`

```rust
pub mod example;
// ...
// inside all():
Box::new(example::Example),
```

### 3. Build and test

```sh
cargo build
spanreed probe example     # forces the provider even if undetected
```

### Rules

- **Never panic in `probe()`.** Return `ProviderOutput::error(ID, NAME, msg)`
  for every failure path. Messages should be actionable
  (e.g. "Run `tool login` again.").
- **Use `creds::` helpers** for all credential/path/SQLite/keyring/`/proc`
  access instead of hand-rolling IO.
- **Refresh and persist** OAuth tokens back to the source you read them from.
- **Match the API exactly.** These endpoints are undocumented; copy field names
  and units (cents vs dollars, seconds vs ms) precisely.
- **Never log raw tokens** or write secrets to disk outside their original
  credential file.

## Sharing changes

- Keep changes focused. One provider or one fix per change.
- For provider work, include before/after `spanreed probe <id>` output (tokens
  redacted).
- Run fmt, clippy, and `SPANREED_OFFLINE=1 cargo test`.
- Clear, human Conventional Commit subjects — no version numbers, no PR ids.

## Releases

No automated release pipeline in-tree right now (local-first clean slate).
Bump `version` in `Cargo.toml` deliberately when you cut a line; keep
`CHANGELOG.md` in sync. Not published to crates.io.

Install bootstrap scripts live in `scripts/install.sh` and
`scripts/install.ps1`. They expect GitHub Release assets named
`spanreed-<target>.tar.gz` / `spanreed-x86_64-pc-windows-msvc.zip`, or
accept `--from-path` / `-FromPath` for local dogfood. After placing the binary
they run `spanreed setup`.

## Reporting issues

Open an issue with:

- the provider id and `spanreed probe <id>` output (redact tokens),
- what you expected vs. what you saw,
- your distro and how the provider's CLI/app stores its credentials.

## License

By contributing you agree that your contributions are licensed under the
project's [MIT License](LICENSE).

## Branch policy

Open feature/fix PRs against **`dev`**, not `master`. When a batch is ready, open a single **`dev` → `master`** integration PR.
