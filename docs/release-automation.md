# Release automation

Feature PRs land in dev. Prepare release creates or updates one release branch and PR into master: patch by default, minor/major via Manual Version Bump. It reads the latest stable GitHub release, requires its tag in dev, and keeps CLI/desktop versions and their local lock entries synchronized. It does not update third-party dependencies or require an AI changelog service. Human review/merge remains required; the bot does not approve its own PRs.

## Permissions and checks

Use only GITHUB_TOKEN. Repository workflow permissions stay read-only by default; Settings → Actions → General must allow GitHub Actions to create pull requests. Preparation/synchronization jobs request contents, pull-requests and actions write. Publication requests contents write, actions read and pull-requests read. No personal token or signing secret is required.

Bot-created PRs do not trigger pull_request workflows. Preparation explicitly dispatches CI, Nix, Desktop builds and (for master) guard master on the exact release head. Each dispatch validates PR ownership/base/head SHA; existing branch protection must require all release checks. A changed head needs fresh results. CLI and desktop builds used for publication are reusable workflows. macOS/ARM jobs stay opt-in, excluded from release publication; Windows is explicitly unsigned.

## Publication and recovery

A master push validates that its full SHA is the merge of a release PR, versions agree, and a nonempty changelog section exists. CLI Linux musl, CLI Windows with tray, Ubuntu deb and Windows NSIS build from that SHA. Each artifact contains its provenance and SHA256 sidecars. Publication verifies four build manifests and eight files, creates a draft, uploads without clobbering and downloads to verify before marking latest/public. No install scripts are release assets; canonical install URLs use fabrials.com.

After publication a master-to-dev synchronization PR preserves development work and receives explicit checks. Merge it after green checks before preparing another release. Conflicts or non-bot edits to an existing release branch are reported, not overwritten.

Re-run failed jobs for transient upload/API errors; completed builder artifacts remain associated with the same run. To recover with another completed Release run whose four builders passed:

```sh
gh workflow run release.yml --ref <ref-at-release-sha> -f release_sha=<40-character-sha> -f artifacts_run_id=<run-id>
```

The supplied run must have the same SHA and be a Release push/dispatch in this repository. A draft with differing assets, a conflicting tag, or an incomplete public release fails without overwrites. Re-running a complete public release verifies its downloads and performs no publication mutations or rebuilds. Missing/expired build artifacts require a new build run; do not delete public assets to force a recovery.

The first deployment of new dispatch workflows may require an administrator to start checks with gh until those workflows are registered on master. This bootstrap is separate from the acceptance release, which must publish with GITHUB_TOKEN inside Actions.

## Validation

Run `python3 -m unittest discover -s scripts/tests -v`, actionlint, and normal Rust gates before PRs. The 0.3.1 acceptance cycle must cover bot PR creation and exact-SHA checks, approved merge, all assets published by Actions, public SHA256 verification, no-op recovery, and resolved master-to-dev synchronization. Preserve the already published 0.3.0 release and the unrelated local Codex credential-selection patch.
