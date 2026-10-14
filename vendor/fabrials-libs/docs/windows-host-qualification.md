# Windows host qualification — 2026-10-14

This qualification uses the existing `win11-qa` VM (Windows 11 build 26200,
x86_64 MSVC, Rust/Cargo 1.95.0). Syl drives QEMU Guest Agent. A temporary
Scheduled Task runs the tests as the logged-in `WIN11-QA\friend` account with
`RunLevel Limited`; the report independently records that its administrator
role is inactive. No viewer or host foreground input is needed.

## Security changes

An isolated ACP child is created suspended and assigned to a kill-on-close Job
before its initial thread is resumed. Assignment and thread-discovery failures
terminate the suspended child and return an error. Thread lookup requires one
thread belonging to the live child process handle and a suspend count of one.
The process and thread handles remain open during these checks. Shared ACP
clients retain their existing spawning behavior: an existing terminal leader
must not be placed in a Job whose lifetime belongs to one remote client.

Private state requires the actual owner SID to match the process user SID, a
protected DACL, explicit access for that user, and no grants to principals other
than that user and SYSTEM. A caller with administrator access cannot adopt a
foreign-owned state directory. Security changes operate on an opened handle,
reject a reparse-point leaf and preserve the existing owner. The instance lock
also opens its leaf without following reparse points and applies its DACL to
that same handle.

This does not make all filesystem operations in the host immune to path races.
Directory enrollment has its separate handle identity and final-path containment
checks; the report qualifies the specific cases listed below.

## Executed evidence

The checked-in [report](evaluation/windows-host-2026-10-14.json) binds selected
source and runner files, the native test executable and the captured logs to
SHA-256 hashes. Windows `cargo clippy --locked -p fabrials-agent-host --all-targets
-- -D warnings` also passes. Clippy 1.95 required three equivalent lint cleanups
and a Unix-only fixture variable rename that Linux Clippy 1.98 did not require.

The limited-user run passes all 16 selected tests, with zero failures and zero
ignored tests. The normally ignored foreign-owner test is explicitly enabled
with a prepared fixture owned by SYSTEM that grants full control to the test
user. It checks both the ACL and owner before and after direct protection and
`ensure_private_directory`, and requires those fixture inspections to succeed.

| Filter | Passed | Evidence |
| --- | ---: | --- |
| `win_acl::tests` | 4 | Files/directories, foreign grants, inheritance, real junction, foreign owner unchanged |
| `win_job::tests` | 3 | No execution before assignment, preflight failure kills before execution, live parent and descendant die on Job close |
| `win_fs::tests` | 1 | Opened directory containment and sibling prefix collision |
| `remote_projects::windows_tests` | 1 | Real junction excluded, replaced child refused, principal separation |
| `control::windows::windows_tests` | 3 | Named-pipe creation, derived identity, mint/status/stop |
| `instance::tests` | 4 | Private state, exclusive ownership, release and retry |

The first SYSTEM run used an older snapshot and its PowerShell tree-wait harness
hung after three successful tests. The corrected snapshot's SYSTEM run then
rejected fresh test paths whose Windows default owner was Administrators; its
foreign-owner and junction negative tests passed. Neither attempt qualifies
positive behavior as SYSTEM. The final harness uses a bounded process wait and
captures both streams separately. The successful qualification runs as a normal
user, which is the supported mode. Existing state with another owner needs an
explicit operator migration; the application will not take ownership for it.

On Linux, the same Rust source passes all 458 host unit tests and 77 integration
tests, plus `cargo clippy --locked -p fabrials-agent-host --all-targets -- -D
warnings`. These deterministic ACP fixtures use no inference provider.

## Reproduction

Prepare a fresh checkout and a new evidence directory. In a privileged fixture
setup context, create a disposable directory owned by SYSTEM, disable inherited
permissions, and grant full control to both SYSTEM and the chosen ordinary test
user. Use separate `icacls /grant` and `icacls /setowner` calls. Do not use a real
application state directory as the fixture.

Run the following under that ordinary user, with the configured MSVC environment
and at most two Cargo build jobs:

```powershell
.\scripts\qualify-windows-host.ps1 `
  -Source C:\qa\libs `
  -EvidenceDirectory C:\qa\evidence-new `
  -ForeignOwnerFixture C:\qa\foreign-owner-fixture
```

Alternatively, pass `-TestExecutable` with the compiled host unit-test executable
to reuse the same binary under a different user. The report records its SHA-256,
selected source/runner hashes, account, active administrator role, test counts,
exit codes and unqualified capabilities. The prepared foreign-owner fixture is
mandatory and ignored tests fail qualification. Remove the temporary Scheduled
Task after it finishes; retain evidence and remove disposable fixtures when no
longer needed.

## Remaining release gates

This evidence does not qualify Windows self-update, actual per-user service
installation/restart, shared terminal leader continuity with real Grok,
production inference, power failure, or physical hardware. The Windows remote
capability remains disabled until the coordinating release completes its native
end-to-end gates. Passing this focused suite is not sufficient to enable it.
