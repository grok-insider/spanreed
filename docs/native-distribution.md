# Native distribution qualification

The supported release archive targets are `x86_64-unknown-linux-musl` without tray and `x86_64-pc-windows-msvc` with tray. macOS is experimental and excluded from the stable release asset set. This is a candidate pipeline; its current Linux/Windows workflow changes have not yet produced a published release.

`scripts/native-notices.py generate` resolves the selected package, target and features through Cargo metadata, retaining normal and build dependencies. It checks original cached `.crate` archives against Cargo.lock, copies complete original sources and preserves original license/notice bytes, including nested native notices. Packages without bundled text may use a repository at their original Cargo VCS commit; old crates without that metadata require reviewed, checksum-bound source pins. Unresolved licenses fail the build. The verifier rederives the dependency closure and checks every source, notice section and file inventory.

The Rust notices are bound to the exact rustc identity. The generator either fetches the original version/commit documents or accepts `--rust-notices <qualified-wasm-notice-directory>` after checking its recorded rustc identity and original section hashes. A missing upstream version is an error, not permission to substitute the latest text. The local Rust 1.98.1 documents require that qualified bundle in this environment. Static musl notices use the original v1.2.5 COPYRIGHT; its bytes were also compared with v1.2.3 and v1.2.4 and are identical (SHA256 `f9bc4423732350eb0b3f7ed7e91d530298476f8fec0c6c427a1c04ade22655af`). The exact libc linked by each release toolchain still needs its provenance recorded during native qualification.

The CLI build sets `SPANREED_NATIVE_NOTICES` to the generated `THIRD-PARTY-NOTICES.txt` and `SPANREED_NATIVE_NOTICES_REQUIRED=1`. `build.rs` embeds those original bytes; `spanreed licenses` prints them and the application's AGPL text without constructing application services. Builds without the variable carry an explicit unqualified-local-build message. The release packaging script rejects them. This keeps attribution attached when shell/PowerShell installers or the durable updater replace only the executable; original registry source archives remain in the downloaded distribution archive and release assets.

`scripts/package-cli.py build` verifies the native inventory, exact toolchain identity, selected platform/features, application license, embedded notice bytes and complete archive hashes. The archive contains `spanreed` or `spanreed.exe`, `LICENSE`, `NATIVE-ARTIFACT.json` and `notices/`, including the original source archives. Archive verification rejects traversal, symlinks, duplicates, oversized members, missing files and digest mismatches. CI additionally checks the executable's architecture/static-link contract and startup before packaging. The release producer's provenance and SHA256 sidecar bind that archive to the release commit.

Validated so far:

- Four original-notice/closure tests, five packaging tests and fifteen release-orchestration tests pass locally; the complete Python suite passes 58 tests.
- After the canonical libraries sync to `20c6b44`, actual native notice generation and verification passed for Linux GNU (387 packages, 811 original notice files, 377 original registry sources), Linux musl (387, 812, 377) and Windows/tray (395, 826, 387) against the final candidate lockfile. These platform inventories use the local Rust 1.98.1 identity, not a claim about another release runner toolchain.
- The GNU CLI compiled with the final vendor and required notice bundle. With an empty runtime environment, `spanreed licenses` emitted exactly `LICENSE` plus two LF bytes plus the original notice bundle: 4,341,527 bytes, SHA256 `282e527c558f16589a93dd0e7d7dbd86363c0360535cd531d76758b1d13a7052`. The CLI integration test also checks exact bytes and refusal of unexpected arguments.
- With the final vendor and embedded notices: Linux updater 31/31 tests, including real subprocess termination at all durable phases and the replacement gap; CLI integration 23/23; clippy on adapters/app/CLI with all targets and denied warnings, and workspace formatting, pass.

Outstanding gates:

- Qualify the actual Linux musl/Windows archive workflow. The GNU debug build does not qualify either distributed architecture/link target.
- Complete desktop Rust/frontend notice packaging and Nix notice integration; do not claim those outputs qualified from the CLI checks.
- Exercise Windows DACL/Job/version fixtures and update/recover/rollback/installer lifecycle in the existing Windows QA guest. Windows apply remains disabled.
- `block@0.1.6` and `malloc_buf@0.0.6` declare MIT but their exact historical sources do not supply standalone license text. The reviewed source pins match the released Rust files, but do not fill that legal gap. macOS stays excluded until it is resolved and native execution is qualified.
- No OS power-loss test has been performed. Process termination coverage is not evidence of storage-controller or Windows crash durability.
