# Contributing to spanreed

Thanks for your interest. spanreed is a small, focused Rust project; the most
common contribution is **adding a new provider**. This guide covers the dev
setup, the add-a-provider walkthrough, and PR expectations.

For the high-level architecture and module map, read [`AGENTS.md`](AGENTS.md)
first.

## Dev setup

You need a Rust toolchain (stable). On NixOS, `nix develop` drops you into a
shell with `cargo`, `rustc`, `rustfmt`, `clippy`, `rust-analyzer`, and
`libsecret`.

```sh
cargo build                 # debug
cargo build --release       # release binary: target/release/spanreed
cargo fmt                   # format
cargo clippy                # lint
./target/debug/spanreed list
./target/debug/spanreed probe <id>
```

Run `cargo fmt` and `cargo clippy` before opening a PR.

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

## Pull requests

- Keep PRs focused. One provider or one fix per PR.
- For provider changes, include before/after `spanreed probe <id>` output (with
  any tokens/PII redacted) so reviewers can see it works.
- Run `cargo fmt` and `cargo clippy`; CI runs `nix flake check` on both
  `x86_64-linux` and `aarch64-linux`.
- Write clear, human commit messages. No AI-generated commit slop.

## Releases

Releases are automated with [release-plz](https://release-plz.dev) plus an
LLM-written changelog — you never tag or hand-edit `CHANGELOG.md`:

1. Merge feature/fix PRs to `master` as usual (Conventional Commit titles; commit
   messages stay human-written).
2. A standing **Release PR** (`chore: release vX.Y.Z`) is kept up to date
   automatically: release-plz bumps the version and `scripts/gen-changelog.sh`
   writes a user-facing `CHANGELOG.md` section (model `deepseek/deepseek-v4-flash`
   via OpenRouter) into it. Review and edit those notes in the Release PR.
3. **Merge the Release PR** to ship. release-plz creates the `vX.Y.Z` tag + GitHub
   Release; `release.yml` then builds the static musl binaries (x86_64 + aarch64)
   and attaches them, with the release body taken from the `CHANGELOG.md` section.

Nothing is published until the Release PR is merged. Delivery is the GitHub
Release (prebuilt binaries) and the Cachix cache — not crates.io. Do **not** set
`publish = false` in `Cargo.toml` (release-plz would skip the package and open no
Release PR); crates.io is disabled in `release-plz.toml` via `git_only = true`
and `publish = false`.

Regenerate a changelog section locally (e.g. to preview):

```sh
OPENROUTER_API_KEY=... scripts/gen-changelog.sh 0.2.0 v0.1.0..HEAD
```

Maintainer one-time setup: repo secrets `RELEASE_PLZ_TOKEN` (fine-grained PAT with
Contents + Pull requests: read/write) and `OPENROUTER_API_KEY`, plus the "Allow
GitHub Actions to create and approve pull requests" setting enabled.

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
