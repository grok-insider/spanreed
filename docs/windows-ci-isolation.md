# Windows CI test isolation

GitHub's elevated Windows runner can create files owned by Administrators.
That is intentionally rejected by the agent host, whose private state requires
the actual user's SID. CI runs its Windows Cargo suites through
`scripts/run-windows-tests.ps1` with a disposable non-administrator account;
production owner and DACL checks remain unchanged.

The wrapper creates a fresh local account in the built-in Users group and a
read-only snapshot of committed `HEAD`, made with `git archive`. The checkout
and existing build output receive no write grant. The account has its own
writable target, Cargo registry, profile, temporary files and logs. Only Cargo's
registry cache and index are copied; credentials and configuration are excluded.
The installed toolchain receives a temporary read/execute grant, removed during
cleanup. The child receives an explicit Windows/compiler environment and none
of the parent's other environment variables. MSVC is discovered with `vswhere`;
`VsDevCmd` initializes an x64 compiler environment, from which only explicit
compiler variables are copied. The MSVC linker directory precedes Git in PATH,
and Cargo receives the absolute native linker path. The full developer
environment is never printed or passed to the test user.

The child verifies its SID, inactive administrator role, actual ownership of a
new directory, rejection of writes to the source snapshot and absence of a
parent-only environment canary. A native helper creates it suspended, assigns a
kill-on-close Windows Job, then resumes it. The default lifetime limit is 40
minutes. Completion, failures and timeouts close that Job and remove the account,
loaded profile after unload, toolchain grant and private working directory.
Cleanup failures fail the job. Cleanup waits for an empty Job and an unloaded
profile with separate five-second bounds. Removing the private directory retries
for up to ten seconds for transient Windows WebCache locks, then fails if the
directory remains. Evidence is retained separately for
one day on non-PR jobs; PR jobs print their reports without uploading artifacts.

The `cross` suite runs the existing default and tray checks/tests and the
vendored shared workspace tests. The `desktop` suite runs the root workspace's
contracts tests and the explicit ignored Credential Manager roundtrip. They run
actual Cargo commands, including normal doctests, under the limited account;
no test filter is added to the normal suites.

```powershell
./scripts/run-windows-tests.ps1 -Suite cross
./scripts/run-windows-tests.ps1 -Suite desktop
./scripts/run-windows-tests.ps1 -Suite cargo-probe
```

The wrapper deliberately tests committed `HEAD`, not uncommitted edits. Its
setup requires an elevated Windows CI process. It does not alter any existing
account or its memberships. It is a correctness fixture for trusted CI source,
not a sandbox for hostile Cargo build scripts.

The PowerShell/C# lifetime wrapper was tested in Windows 11 `win11-qa`: a
successful probe and an expected-failure two-second timeout.
Both used newly created non-administrator accounts, proved file ownership and
source/environment isolation, terminated a sleeping descendant and removed
their accounts and private roots without cleanup errors. The temporary task
used to launch the elevated fixture was removed. The recorded source hashes and
evidence hash are in the [report](evaluation/windows-ci-isolation-2026-10-14.json).
Both workflow files pass `actionlint`.

The first hosted Windows run reached Cargo but failed to find its manifest:
PowerShell's `Set-Location` does not change the native working directory used by
`.NET ProcessStartInfo`. The launcher now sets `WorkingDirectory` explicitly.
The `cargo-probe` suite uses the same command runner to execute real Cargo
`metadata --locked --offline --no-deps --format-version 1` against a synthetic
read-only package, checking its reported manifest and workspace paths. This
passes under a new limited account with Cargo 1.95.0; removing the working
directory assignment reproduces the original failure. It copies no registry cache.

The second hosted run reached compilation but selected Git's `link.exe`, because
the new account had no Visual Studio compiler setup. The wrapper now initializes
that setup explicitly. `cargo-probe` also compiles and executes a dependency-free
native binary via `cargo run --locked --offline --quiet`, checking its actual
working directory. Both commands passed with Rust 1.95.0 and MSVC 14.44.35207
under a fresh non-administrator account; account, profile and private directory
cleanup passed. The first compiled probe exposed the transient WebCache cleanup
lock; its Rust commands passed, but the wrapper correctly failed cleanup. The
remaining directory was removed and the complete probe passed with bounded
cleanup retries. No mutation substituting Git's linker was run in this guest,
which has no Git installation.

The hosted desktop Cargo suite passed at `34e0af0`: contracts and the explicitly
executed Credential Manager roundtrip, under the dedicated non-administrator
account. That job subsequently failed outside the wrapper when the native notice
generator decoded Cargo metadata as cp1252; strict UTF-8 fixes that separate issue.

The hosted cross suite passed its all-target check but failed in the agent host
unit tests. Its precise assertion was obscured because the combined stdout/stderr
tail contained only compilation messages. The wrapper now retains and prints
separate tails for both streams and reports resolved tool paths without dumping
the environment. A native failure fixture proved that its stdout diagnostic stays
visible alongside 150 stderr lines, with cleanup succeeding; the normal compiled
probe also passed.

The complete host library at `2f5eba7` was exercised separately in the Windows VM:
442 passed, two POSIX shell tests failed because that guest lacks `sh`/Git, and
one test requiring an explicit foreign-owner fixture remained ignored. This does
not establish the cause of the original hosted failure. The complete cross suite
and subsequent installer build still require successful hosted runs. None of
these checks qualifies a Windows release or enables its closed self-update gate.
