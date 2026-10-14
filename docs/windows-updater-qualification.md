# Windows updater qualification — 2026-10-14

The focused updater suite passed in `win11-qa`, Windows 11 build 26200, using
Rust/Cargo 1.95.0 and MSVC. The source was the retained release candidate
`8b12b86b67a0d6aceadfde3aeb4a88768335cdd0`; its archive hash, the executable hashes
and selected source hashes are recorded in the
[report](evaluation/windows-updater-2026-10-14.json).

The compiled tests run as `WIN11-QA\friend` through a temporary Scheduled Task
with `RunLevel Limited`. The process reports no active administrator role. Syl
uses QEMU Guest Agent, with no viewer or host foreground input. The temporary
task was removed afterward.

The `self_update::` filter passes 26 tests. Its one ignored native Job test is
then explicitly executed and passes separately, leaving no unexecuted test in
that selected group. Coverage includes durable recovery and replacement failure,
rollback, active-host exclusion, ACL read/write distinctions and the closed
Windows release gate. The native fixture covers ten rapid exits, deadlines,
parent/descendant cleanup, excess output, wrong version, assignment rejection
before candidate execution and absence of descendant escape markers.

Reproduce by compiling the retained source and native fixture:

```powershell
cargo test --locked -p spanreed-adapters --lib --no-run --message-format=json
cargo build --locked -p spanreed-adapters --example update_probe_fixture
```

Run `scripts/qualify-windows-updater.ps1` under the ordinary test user, passing
`-Source`, `-TestExecutable` (the adapters test executable reported by Cargo),
`-ProbeFixture` (the built `update_probe_fixture.exe`) and a new
`-EvidenceDirectory`. The harness records the source and binary hashes and fails
on unexpected ignored tests, process failures or test timeouts.

This does not qualify replacement and rollback of a real installed application,
power failure, an installation lifecycle or physical Windows hardware. There is
no qualified Windows distribution notice bundle for the VM's 1.95.0 toolchain
in this run. No Windows package was distributed and the Windows self-update
release gate remains closed. The successful synthetic fixtures do not replace
those remaining release checks.
