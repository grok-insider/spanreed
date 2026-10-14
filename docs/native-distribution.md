# Native distribution qualification

The supported release archive targets are `x86_64-unknown-linux-musl` without tray and `x86_64-pc-windows-msvc` with tray. macOS is experimental and excluded from the stable release asset set. This is a candidate pipeline; its current Linux/Windows workflow changes have not yet produced a published release.

`scripts/native-notices.py generate` resolves the selected package, target and features through Cargo metadata, retaining normal and build dependencies. It checks original cached `.crate` archives against Cargo.lock, copies complete original sources and preserves original license/notice bytes, including nested native notices. Packages without bundled text may use a repository at their original Cargo VCS commit; old crates without that metadata require reviewed, checksum-bound source pins. Unresolved licenses fail the build. The verifier rederives the dependency closure and checks every source, notice section and file inventory.

The Rust notices are bound to the exact rustc identity. The generator either fetches the original version/commit documents or accepts `--rust-notices <qualified-wasm-notice-directory>` after checking its recorded rustc identity and original section hashes. A missing upstream version is an error, not permission to substitute the latest text. A third option, `--rust-source-archive`, reads the original source tarball, checks its exact version and commit against rustc, and retains the root license texts, `LICENSES/` and all original standard-library/vendor notices. It is the distribution default: the old versioned copyright-document URL returns 404, including for the stable toolchain. `scripts/prepare-native-notices.py` downloads the official versioned source tarball and checks its published SHA256 before generation. Nix uses its fixed source derivation with no build-time network. Static musl notices use the original v1.2.5 COPYRIGHT; its bytes were also compared with v1.2.3 and v1.2.4 and are identical (SHA256 `f9bc4423732350eb0b3f7ed7e91d530298476f8fec0c6c427a1c04ade22655af`). The exact libc linked by each release toolchain still needs its provenance recorded during native qualification.

The CLI build sets `SPANREED_NATIVE_NOTICES` to the generated `THIRD-PARTY-NOTICES.txt` and `SPANREED_NATIVE_NOTICES_REQUIRED=1`. `build.rs` embeds those original bytes; `spanreed licenses` prints them and the application's AGPL text without constructing application services. Builds without the variable carry an explicit unqualified-local-build message. The release packaging script rejects them. This keeps attribution attached when shell/PowerShell installers or the durable updater replace only the executable; original registry source archives remain in the downloaded distribution archive and release assets.

`scripts/package-cli.py build` verifies the native inventory, exact toolchain identity, selected platform/features, application license, embedded notice bytes and complete archive hashes. The archive contains `spanreed` or `spanreed.exe`, `LICENSE`, `NATIVE-ARTIFACT.json` and `notices/`, including the original source archives. Archive verification rejects traversal, symlinks, duplicates, oversized members, missing files and digest mismatches. CI additionally checks the executable's architecture/static-link contract and startup before packaging. The release producer's provenance and SHA256 sidecar bind that archive to the release commit.

Earlier candidate validation (before `ce2692d`):

- The complete Python suite passes 65 tests, including original-notice/closure verification, CLI/desktop packaging, release orchestration, offline/source-runtime verification and safe concurrent source downloads.
- After the canonical libraries sync to `20c6b44`, actual native notice generation and verification passed for Linux GNU (387 packages, 811 original notice files, 370 original registry sources), Linux musl (387, 812, 370) and Windows/tray (395, 826, 377) against the final candidate lockfile. These platform inventories use the local Rust 1.98.1 identity, not a claim about another release runner toolchain.
- The GNU CLI compiled with the final vendor and required notice bundle. With an empty runtime environment, `spanreed licenses` emitted exactly `LICENSE` plus two LF bytes plus the original notice bundle: 4,341,527 bytes, SHA256 `282e527c558f16589a93dd0e7d7dbd86363c0360535cd531d76758b1d13a7052`. The CLI integration test also checks exact bytes and refusal of unexpected arguments.
- With the final vendor and embedded notices: Linux updater 31/31 tests, including real subprocess termination at all durable phases and the replacement gap; CLI integration 23/23; clippy on adapters/app/CLI with all targets and denied warnings, and workspace formatting, pass.

Outstanding gates:

- Qualify the actual Linux musl/Windows archive workflow. The GNU debug build does not qualify either distributed architecture/link target.
- Qualify full Nix and Tauri installer artifacts. The exact Linux Tauri executable and its native notice IPC were checked separately below; neither a full Nix product build nor a Tauri installer execution has been performed in this integration pass.
- Exercise Windows DACL/Job/version fixtures and update/recover/rollback/installer lifecycle in the existing Windows QA guest. Windows apply remains disabled.
- `block@0.1.6` and `malloc_buf@0.0.6` declare MIT but their exact historical sources do not supply standalone license text. The reviewed source pins match the released Rust files, but do not fill that legal gap. macOS stays excluded until it is resolved and native execution is qualified.
- No OS power-loss test has been performed. Process termination coverage is not evidence of storage-controller or Windows crash durability.

## Desktop and Nix integration

The desktop renderer generates `FRONTEND-NOTICES.txt` and a hash/range inventory from its installed production dependencies, required peers, nested font notices, the vendored Lobe notice and the Vite runtime. The complete production dependency closure is retained even when bundling removes code. Missing text or an unreviewed license fails the build. Embla 8.6.0 needs its exact upstream license at the npm gitHead. `react-remove-scroll-bar` omits the text in both npm 2.3.7 and 2.3.8: their complete `dist/` files are byte-identical, and the source blobs at npm 2.3.7's gitHead match the upstream commit supplying its original license. The immutable URL, source blobs, original checksum and locked registry integrity are recorded in `desktop/notices/source-pins.json`.

Tauri embeds the native and frontend originals in the executable and exposes them in Settings → General → Licenses. Its distribution build refuses missing notices. `scripts/package-desktop.py` verifies both inventories, stages all original crate sources under the installer's `licenses/native/`, and checks exact license bytes in the executable after building. The original AGPL text is also supplied as the installer license. macOS signing remains blocked by the required notice gate until its native closure is qualified.

The Nix CLI and desktop hooks use the same generator with checksum-addressed `.crate` downloads, checksum-pinned supplementary notices and the exact Nix rustc source. They embed the notices before compilation and install original sources/inventories below `$out/share/spanreed/licenses` or `$out/share/spanreed-desktop/licenses`. No network is used by the generator in the sandbox. `nix/notices.nix` derives the input list from Cargo.lock; a version bump does not require an opaque extra notice-bundle hash. A newly unlicensed dependency still fails closed and requires an original source pin before distribution.

The original registry payload counts exclude separately retained supplementary license files. Earlier generator diagnostics combined those categories; the payload and source checksums were unaffected.

The standalone `spanreed-remote-relay` Nix package is excluded from this distribution qualification. The public connection contract is direct and the managed relay remains disabled; these CLI/desktop notices do not qualify the relay artifact.

Integration checks before the native runtime trial: desktop typecheck and production renderer build; 27 existing desktop tests plus two original-notice tests; 65 Python tests; real desktop native notice generation with 465 packages, 932 original notice texts and 451 registry archives; real desktop redistribution staging; fully offline CLI generation from original sources (387 packages, 815 original notice texts, 370 registry archives); Nix derivation evaluation for CLI and desktop. No full Nix build was run on station.

## Exact native runtime trial, 2026-10-14

The clean source `ce2692d2c7f7ca7db4d15f25c213a2a050baabba` includes canonical
libraries `6ae91bd`, the multidevice PWA and the Unix updater lock fix. The GNU
CLI built with required original notices; its license output is 4,417,799 bytes,
SHA256 `54e86dfbf0bdafc7216896404157f5df925c25f846a5e79345528deb86083e75`.
Eighteen lifecycle checks passed. The binary SHA256 is
`e4fa51029b1d601f577808ec8e9dee2f8588542804d75e61081d0e8d467aca07`.
It was activated in station's independent QA Nix profile and service, preserving
identity and pairings. That small wrapper copies the verified binary and its
runtime/source/notice closure; it is not a full native Nix product build.

The Linux GNU debug Tauri executable built with custom protocol and no installer,
with required original notices. In Syl's hidden Sway, Settings → View licenses
returned exactly 5,068,946 bytes after the textarea's normal newline conversion,
SHA256 `3c24e36b9653115979a7facf85fb1d26d410591697e8b20d31164295fcdf1c00`.
`package-desktop.py verify` independently found the complete 5,069,785 original
bytes in the binary. The frozen source was clean; its executable SHA256 is
`3bd8e49d002ba2ea536e999925c09b98782eb9949fbd8684633720a2d1657233`.
The [IPC receipt](evaluation/native-notices-ipc-2026-10-14.json) records the
native extraction method and limits. The owned application, launcher and hidden
compositor were closed afterwards. The filesystem profile was separate, but the
host network was shared; this was not a security sandbox.

Unix updater tests now pass 32 cases, including an inherited-lock regression
that fails when explicit unlock is removed. Windows updater/security evidence
is separate in [windows-updater-qualification.md](windows-updater-qualification.md).
Actual `.deb`/NSIS installation, published musl/Windows archives, Windows apply,
power-loss durability and full platform release qualification remain pending.

## Shared engine lifetime follow-up

Canonical libraries `2f5eba7f96a0ef4afc218bb9939d51631a7efa93` separate a Shared
ACP client's Linux systemd scope from the host service. A newly started leader
can therefore survive a host restart. An existing leader is never moved by this
code. Failure to create the required scope fails before agent execution instead
of falling back to an unsafe service-owned process. Outside a systemd service,
the direct launch path is retained.

Both ACP APIs share process ownership and bounded stderr draining. Isolated
Unix cleanup covers the inherited process group; it does not contain a child
that deliberately creates a new group. On Windows, explicit shutdown closes the
isolated Job even while another handle reference remains alive; Shared children
remain independent.

The [receipt](evaluation/acp-lifetime-2026-10-14.json) binds all changed files to
the synced canonical source. It records 538 host tests, strict Clippy, Linux
service stop/restart and missing-manager trials, three caught Linux mutations,
and the native Windows retained-reference trial with a caught mutation. The
Windows trial ran without an active administrator role and left no synthetic
agent process or temporary task. These source-level trials are distinct from
the earlier installed `ce2692d` artifact and do not qualify a new installer.
