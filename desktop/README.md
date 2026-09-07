# Spanreed desktop

Tauri 2 hosts the React interface. The renderer receives usage, account metadata
and explicit sharing preferences through native commands; provider credentials
stay in Rust. IBM Plex Sans and the React components come from `@fabrials/ui`.

```sh
bun install --frozen-lockfile
bun run tauri dev
bun run tauri build
```

On NixOS, enter `nix develop` from the Spanreed checkout first. Linux needs
WebKitGTK 4.1, GTK3 and AppIndicator. The development shell provides Bun, native
build dependencies and AppIndicator's runtime library path (the tray loads it
dynamically). Run the commands above from `desktop/` inside that shell.

For a direct native build with the embedded frontend:

```sh
bun run build
cargo build --locked --manifest-path src-tauri/Cargo.toml --features tauri/custom-protocol
./src-tauri/target/debug/spanreed-desktop
```

Keep the development shell active when running that unwrapped debug binary.
The flake provides a separate desktop package alongside the default CLI/tray:

```sh
# From the Spanreed checkout
nix build .#spanreed-desktop
nix run .#desktop
# Install both so `spanreed gui` can find the desktop executable:
nix profile install .#spanreed .#spanreed-desktop
```

With the Spanreed Home Manager module imported, set
`programs.spanreed.enable = true` and `programs.spanreed.desktop.enable = true`.
This installs both packages; background services retain their separate options.
The desktop package embeds the frontend and wraps GTK/WebKit resources and the
AppIndicator runtime dependency. It does not need the development shell.

The Bun dependency derivation installs from the frozen lockfile with scripts
disabled and verifies a recursive SHA-256 hash. Dependency changes may require
updating `nodeModules.outputHash` in `flake.nix`. The local UI package is linked
to the current source before building, so cached dependencies cannot retain an
older version of its components. The Linux dependency
closure includes both CPU architectures; actual ARM desktop execution still
requires platform verification.

The desktop workflow builds the native application and a platform installer:
Debian `.deb`, macOS `.dmg`, or Windows NSIS setup executable. It collects
installer SHA-256 files in artifacts explicitly named `unsigned`; these are
review artifacts, not signed releases. Signing, notarization and publication
remain separate release requirements. A workflow definition is not evidence
that those platforms have been tested.

For a signed macOS review artifact, manually dispatch Desktop builds with
`sign_macos=true`. After the unsigned platform jobs pass, the additional job
builds a universal Intel/Apple Silicon DMG, signs and notarizes its application,
then verifies codesign, Gatekeeper, the stapled ticket and both architectures
before uploading the DMG and checksum. It does not publish a release.
Configure the `desktop-signing` GitHub environment with `APPLE_CERTIFICATE`
(base64 P12), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`
(`Developer ID Application: …`), `APPLE_ID`, `APPLE_PASSWORD` (an app-specific
password) and `APPLE_TEAM_ID`. These credentials are available only to the
signing step. Missing configuration fails rather than producing an unsigned
artifact under a signed name. Configure environment branch/reviewer rules for
the refs permitted to sign. This path has not yet run on a macOS runner.
The variables follow the official
[Tauri signing and notarization guide](https://v2.tauri.app/distribute/sign/macos/).
Windows signing and release publication still require their own configured
distribution credentials and workflow.

To build an installer locally, run `bun run tauri build --bundles deb` on Linux,
`--bundles dmg` on macOS, or `--bundles nsis` on Windows from this directory.
Bundler tools are cached under `target/.tauri` so Windows SYSTEM builds do not
rely on its restricted profile cache.
The Ubuntu 24.04 Debian build requires glibc 2.39 or newer. The package declares
that minimum, `libsecret-tools` for Secret Service credential
discovery and `xdg-utils` for opening browser authorization links, in addition
to the libraries detected by Tauri.
Outputs are under `src-tauri/target/release/bundle` unless Cargo's target directory
is overridden. Windows uses Tauri's current-user NSIS default and downloads the
WebView2 bootstrapper when the runtime is absent. See the official
[Windows installer guide](https://v2.tauri.app/distribute/windows-installer/)
and [macOS bundle guide](https://v2.tauri.app/distribute/macos-application-bundle/).

The screens include overview, accounts, provider detection, autosteer, history,
requests, models, client connections, migration and settings. Accounts support
API-key management and native Grok/Nous device authorization. The GUI can start
its own local proxy and review OpenCode JSON/JSONC or Grok Build TOML edits.
Metrics publication and history synchronization are independent, default-off
choices. Choosing hosted mode opens ai-relay in a browser; transferring selected
API-key accounts requires a separate paired migration and hosted approval.
OAuth accounts require a new authorization in the destination environment.

Spanreed respects absolute `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and
`XDG_CACHE_HOME` overrides on every platform, including Windows. Explicit
`HOME`/`USERPROFILE` overrides also apply to CLI credential-file discovery.
Without overrides, the OS-native directories remain the default. Windows
capture logs and CLI installation use local app data unless `XDG_DATA_HOME`
is set. These overrides are useful for isolated QA or portable data directories;
they do not redirect the operating system's credential store.

OpenCode files follow OpenCode's own XDG conventions on all platforms:
`~/.config/opencode/opencode.json` (or `.jsonc`) and
`~/.local/share/opencode/` for authentication and history, unless the respective
`XDG_CONFIG_HOME` or `XDG_DATA_HOME` is set. They do not use Spanreed's AppData
or macOS Library defaults. Existing files in an inactive directory are not
selected or automatically moved.

The application binary must be on PATH as `spanreed-desktop` for `spanreed gui`.
Status-bar profiles are available through `spanreed profile list`, `show NAME`
and `install NAME --output DIR`. Installation writes new files only; include
the generated fragment from your existing bar configuration. Eww uses its
[documented expression syntax](https://elkowar.github.io/eww/expression_language.html).

Source vendors permit standalone builds while the new shared crates are
unpublished. In the Fabrials workspace, edit `libs/fabrials-*` and run
`python3 tools/sync-shared.py`; `--check` reports drift without writing.

### macOS / ARM contributor needed

Pull requests build Linux and Windows only. A contributor can explicitly enable
the preparatory macOS job with `build_macos=true` on the Desktop builds manual
workflow; `sign_macos=true` also enables it. Neither option changes the deferred
support status without the native evidence below.

As of 2026-09-07, macOS/ARM qualification is deferred by product decision until a contributor with macOS hardware can help. The existing build/signing configuration is preparatory, not a verified support claim. Contributors should record their OS and architecture, source revision, build output and checksums, and native onboarding, account authorization, proxy, preferences, notifications and migration results. Universal Intel/Apple Silicon packaging, Developer ID signing, notarization and Gatekeeper verification also require real evidence before distribution is qualified. Keep credentials and signing material in their secret stores.
