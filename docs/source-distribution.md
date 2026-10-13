# Corresponding source distribution

Every new Spanreed release must include `spanreed-<version>-source.tar.gz` and
its SHA256 sidecar alongside its binaries. The source job produces these files;
publication and recovery independently verify their contents before any public
release mutation. The release remains a draft until all sixteen assets verify.

## Contents and correspondence

The archive contains `spanreed/` at the exact release merge commit, including
the complete existing `vendor/fabrials-libs` subtree, and `portable/` at the
revision recorded by the embedded PWA. It includes the original licenses,
vendored notices, lockfiles, build scripts and workflow recipes. `NOTICE.txt`
indexes license files; `source-bundle.json` records each byte hash and file mode.
Its original Git commit objects allow the verifier to reconstruct both Git
trees and check that the complete exported sources match the claimed commits.
Changing a file and recalculating only the archive's SHA256 is insufficient.

The Noise build revision must equal `vendor/fabrials-libs.source` and every
recorded Portable/Noise input must match the included sources. This avoids
publishing another private library checkout or asking recipients for a private
token. Do not export only the Noise crate with the original workspace lock:
removing the other workspace members changes Cargo's locked resolution.

Release packaging rejects dirty checkouts, dirty embedded inputs, vendor drift,
missing inputs, mismatched versions, links, submodules and oversized archives.
It exports committed files only. No runtime state, ignored credentials,
`node_modules`, `target` or `.git` metadata is collected. A tracked credential
would still be tracked source: normal secret scanning and review remain required.

## Release preparation

1. Finish and commit the reviewed canonical library changes and synchronize the
   complete subtree into Spanreed using its normal sync script. Do not hand-edit
   the vendor copy.
2. Finish and commit Portable. Rebuild `assets/remote-web` using this Portable
   checkout and the same clean library revision as the Spanreed pin, then commit
   the bundle and its source manifest in Spanreed.
3. Prepare and merge the normal release PR. The source job checks out Portable
   by its recorded commit. It needs no private library credentials. Seven native
   builders and the source producer must all succeed before publication.

For a local check from clean release inputs, use an output outside the checkout:

```sh
python3 scripts/source-bundle.py build --portable /path/to/portable \
  --version X.Y.Z --sha FULL_RELEASE_COMMIT \
  --output /tmp/spanreed-X.Y.Z-source.tar.gz
python3 scripts/source-bundle.py verify \
  --archive /tmp/spanreed-X.Y.Z-source.tar.gz \
  --version X.Y.Z --sha FULL_RELEASE_COMMIT
```

The archive is deterministic for the same committed inputs: sorted files,
preserved executable bits, zero timestamps and owners, and no gzip filename.
Existing outputs are never overwritten. These commands do not publish.

## Building from the extracted archive

Install the public toolchains used by the included release workflows and the
embedded manifest's `build` fields. Python 3.12 or newer reads the manifests;
the PWA requires Node 22 or newer and the `packageManager` version from
`portable/package.json` (Corepack selects it). Rust needs the
`wasm32-unknown-unknown` target for rebuilding Noise. Native system libraries,
cross-compilation tools and desktop requirements are listed in
`spanreed/.github/workflows/build-cli.yml`, `desktop.yml` and `spanreed/flake.nix`.
No private Git repository is required; the lockfiles resolve public registries.

From the archive root, rebuild the encrypted PWA with the included exact inputs:

```sh
corepack pnpm --dir portable install --frozen-lockfile
mkdir -p build/crypto
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' cargo build --locked --release \
  --manifest-path spanreed/vendor/fabrials-libs/crates/fabrials-remote-noise/Cargo.toml \
  --target wasm32-unknown-unknown --target-dir build/noise
cargo run --locked \
  --manifest-path spanreed/vendor/fabrials-libs/tools/remote-bindgen/Cargo.toml -- \
  "$PWD/build/noise/wasm32-unknown-unknown/release/fabrials_remote_noise.wasm" \
  "$PWD/build/crypto"
(cd portable && FABRIALS_REMOTE_CRYPTO_DIST="$OLDPWD/build/crypto" \
  node scripts/build-remote.mjs)
```

The archive already includes the PWA that was embedded in the native release.
`(cd spanreed && cargo build --locked --release)` builds the default CLI using
those verified assets and the included `.cargo/config.toml`. For the shipped target/features and
desktop packages, follow the included platform workflows. To modify the PWA,
replace `spanreed/assets/remote-web` with `portable/apps/web/dist` after building;
that produces a modified local build, not the original release. Recreating the
release provenance requires Git checkouts and the release preparation above.

This gate proves source correspondence and distributes existing license files.
It does not claim bit-for-bit reproducibility across different toolchains or
replace the dependency-license audit. Registry dependency sources and their
licenses remain available through the included exact lockfiles; public release
qualification must check their redistribution obligations and required notices.
## Standalone PWA correspondence

Portable’s deployment artifact includes a Source and license dialog and a local
copy of this archive only when its entire PWA shell and original source identities
match the embedded native assets. From the clean reviewed release checkout:

```sh
python3 scripts/source-bundle.py pwa-proof \
  --archive /tmp/spanreed-X.Y.Z-source.tar.gz \
  --pwa-dist /absolute/portable/apps/web/dist
```

This command trusts the clean checkout’s package version and HEAD, verifies the
archive’s original Git trees and embedded source manifest, and compares each
shell byte against the supplied directory. Its JSON proof supplies input digests,
full shell identity, archive checksum, original own license, emitted third-party
notices and hashes of the original Portable installers and deployment configuration.
Portable compares those distribution files too; a newer installer cannot be paired
with an older source archive by editing deployment metadata.

The proof covers both `deploy/remote/build-output.json` and
`deploy/remote/qualification-build-output.json`. Qualification at
`https://grok.fabrials.com` uses an explicitly selected application origin and its
own hosting configuration. Its shell must match the native embedded application
byte for byte. A production-origin archive cannot qualify a test-origin shell.
Set `FABRIALS_REMOTE_ORIGIN=https://grok.fabrials.com` when running
`scripts/remote-assets.py build` for the qualification candidate; the build
inherits this setting when it invokes Portable. Keep the production default
when preparing the ordinary public release.

Review builds may omit source; public deployment verification fails closed
without this exact proof. Older cached HTML cannot use a newer shell’s source
metadata. Hosting must retain matching source access for every supported running
version; Portable’s current artifact contains only its own archive. The local
synthetic tests do not qualify a published clean archive or source-retention hosting.
