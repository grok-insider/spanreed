# Release automation

Feature PRs land in dev. Prepare release creates or updates one release branch and PR into master: patch by default, minor/major via Manual Version Bump. It reads the latest stable GitHub release, requires its tag in dev, and keeps CLI/desktop versions and their local lock entries synchronized. It does not update third-party dependencies or require an AI changelog service. Human review/merge remains required; the bot does not approve its own PRs.

## Permissions and checks

Use only GITHUB_TOKEN. Repository workflow permissions stay read-only by default; Settings → Actions → General must allow GitHub Actions to create pull requests. Preparation/synchronization jobs request contents, pull-requests and actions write. Publication requests contents write, actions read and pull-requests read. No personal token or signing secret is required.

Preparation explicitly dispatches CI, Nix, Desktop builds and (for master) guard master on the exact release head. Each dispatch validates PR ownership/base/head SHA; existing branch protection must require all release checks. A changed head needs fresh results. Immediately after pushing a bot branch, dispatch waits up to ten seconds for the PR API to report that exact SHA; a different head after that bounded window still fails without dispatching checks. Closed or external PRs fail immediately. CLI and desktop builds used for publication are reusable workflows. The stable CLI matrix covers Linux x86_64 musl and Windows x64. CLI macOS archives are excluded until original license texts and native qualification are complete. Desktop macOS qualification stays opt-in and is excluded from release publication; Windows is explicitly unsigned.

GitHub also creates approval-required `pull_request` runs for PRs opened or updated with `GITHUB_TOKEN`. Explicit dispatch results do not clear that approval gate. After reviewing the exact PR head, a maintainer must select **Approve workflows to run** in the PR, or approve each pending run with `gh api --method POST repos/grok-insider/spanreed/actions/runs/<run-id>/approve`, then wait for the required checks before merging normally. Apply this to release and master-to-dev synchronization PRs. Do not bypass branch protection or let the bot approve its own runs. This is part of the review step; publication itself requires no personal token. See [GitHub's token event rules](https://docs.github.com/en/actions/concepts/security/github_token).

## Publication and recovery

A master push validates that its full SHA is the merge of a release PR, versions agree, and a nonempty changelog section exists. CLI Linux musl, CLI Windows with tray, Ubuntu deb and Windows NSIS build from that SHA. A separate source job packages the exact Spanreed tree, its existing vendored libraries and the Portable revision behind the embedded PWA. Each producer records provenance and SHA256 sidecars. Publication verifies five manifests and ten files, including the source archive's Git identities and complete file hashes, creates a draft, uploads without clobbering and downloads to verify before marking latest/public. No install scripts are release assets; canonical install URLs use fabrials.com. Source contents, preparation and rebuild instructions: [Corresponding source](source-distribution.md).

After publication a master-to-dev synchronization PR preserves development work and receives explicit checks. Merge it after green checks before preparing another release. Conflicts or non-bot edits to an existing release branch are reported, not overwritten.

Re-run failed jobs for transient upload/API errors; completed builder artifacts remain associated with the same run. To recover with another completed Release run whose four Linux/Windows native builders and source producer passed:

```sh
gh workflow run release.yml --ref <ref-at-release-sha> -f release_sha=<40-character-sha> -f artifacts_run_id=<run-id>
```

The supplied run must have the same SHA and be a Release push/dispatch in this repository. A draft with differing assets, a conflicting tag, or an incomplete public release fails without overwrites. Re-running a complete public release verifies its downloads and performs no publication mutations or rebuilds. Missing/expired build artifacts require a new build run; do not delete public assets to force a recovery.

The first deployment of new dispatch workflows may require an administrator to start checks with gh until those workflows are registered on master. This bootstrap is separate from the acceptance release, which must publish with GITHUB_TOKEN inside Actions.

## Validation

Run `python3 -m unittest discover -s scripts/tests -v`, actionlint, and normal Rust gates before PRs. The 0.3.1 acceptance cycle must cover bot PR creation and exact-SHA checks, approved merge, all assets published by Actions, public SHA256 verification, no-op recovery, and resolved master-to-dev synchronization. Preserve the already published 0.3.0 release and the unrelated local Codex credential-selection patch.

## Remote and embedded Portable

The released CLI includes Remote directly. It ships as the same single `spanreed` file in each existing archive, so installers, setup and self-update cannot leave a companion on an incompatible version. The standalone relay is a server package and is not required on user machines.

`assets/remote-web` is embedded in native builds and Nix. Its source manifest records every file digest plus the actual Portable/Noise source-input hashes, HEADs and dirty state. Refresh it with `scripts/remote-assets.py build` as described in [Remote](remote.md). PR/Nix checks verify content; release builders additionally require committed source identities. The source producer requires those inputs to match the included Portable tree and Spanreed vendor revision exactly. A bundle built from uncommitted work is valid for local qualification but cannot pass the release gate. The native release smoke check runs `spanreed remote doctor`, which rejects an executable missing the PWA or Noise module. No network fetch or placeholder bundle is used during Rust/Nix builds.


## Native redistribution status

The CLI pipeline generates the exact target/feature dependency inventory before compiling and requires its original notice text through `SPANREED_NATIVE_NOTICES`. `spanreed licenses` embeds the source license and the entire notice payload, so installers and self-update preserve it as part of the executable. Packaging refuses a binary missing those exact bytes and verifies the archive after writing it; each CLI archive contains the inventory and the original checksum-matched registry sources, including MPL components. See [Native distribution](native-distribution.md) for checks and qualification gaps. Local/Nix builds without the generated bundle identify themselves as unqualified for redistribution.

Desktop dependency/frontend inventories and Nix artifact notice integration are not complete. This change does not qualify those packages or authorize a public native release. The Windows updater gate also remains closed until its native fixture passes on Windows. The qualified private PWA deployment and the future native public release are separate artifacts.
