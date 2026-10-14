# Windows NSIS lifecycle qualification

On 2026-10-14, the unsigned desktop artifact built from
`e6d9cc8cb076d7896edec98ee7338d4d4d8f1f7a` passed 31 checks in the existing
Windows 11 QA VM. The [receipt](evaluation/windows-nsis-e6d9cc8-2026-10-14.json)
records the exact installer, installed executable, license bytes and evidence
hashes. Desktop workflow 38504260236 passed on that exact source revision.
Its separate standard-user tests also passed, including Credential Manager,
and removed their temporary account and private test directory.

The test ran as a newly created standard user, with the administrator role
inactive. It used the installer's current-user mode and the HKCU uninstall
registration. The first cycle installed the candidate 0.7.1 in a new profile,
checked its bytes and registration, and uninstalled it. With the installation
directory and registration absent, the second cycle installed published
0.7.0, upgraded to the candidate, repaired it and uninstalled it. That second
cycle used the same previously used profile. Installations used
`/S /D=<temporary-user-install-directory>`; uninstallation used `/S`.

The installed executable matched the one extracted from the CI artifact.
Upgrade replaced the old executable and left exactly one registration with the
candidate version. Repair preserved both the executable and synthetic user
state. Normal uninstallation removed the binary, installation directory and
registration while retaining that state. The inventory contains hashes of
420 installed files, including the original application license, native notices
and crate source archives. Independent verification covered 409 native packages,
841 original notice sections and 395 registry sources. The builder's lockfiles
and application license use Git's Windows CRLF representation; their hashes
were checked against the exact frozen commit after that sole conversion.

The earlier [historical artifact receipt](evaluation/windows-nsis-2026-10-14.json)
covers 25 checks on `9eaabe27b684ef468bd67a809a0cafd92bc7da5a`. Two initial harness
failures were corrected before that successful run: PowerShell 5 needs an array
wrapper when counting a single registry entry, and NSIS can exit before its
child removes the registration. Both successful runs used the corrected
waiting condition. The exact e6 run additionally covers direct installation
of the candidate in a new profile.

Cleanup removed the temporary task, batch-logon right, account, profile,
profile directory and registry hives. Windows User Profile Service retained
the hives after the task finished, so cleanup needed a clean reboot of the QA
VM. Before reboot, the account was disabled and no application unrelated to
the Windows shell was running. The reboot did not force application closure.

The existing user's 351 inventoried files and Spanreed registration, the
machine-wide Spanreed registration and Defender service status/start modes
were preserved. This run captured full service inventories. Comparison ignores
only the generated suffix of services whose registry template has the Windows
per-user service flag; service counts, start modes, executable paths and
identities remain part of the comparison. Four mutation checks confirmed that
this ignores a generated suffix but detects a changed mode, an ordinary service
rename and a missing service.

The first cleanup attempt failed because Windows retained the profile. After
the clean reboot, the cleanup removed all its resources but the baseline guard
returned exit 1: Windows had temporarily changed `IsolationSession` from
Manual to Auto. Both failures are retained. A later read-only comparison,
after 128 seconds of boot, found all normalized service definitions and the
other baselines equal. Service Control Manager recorded four start-mode
transitions for `IsolationSession`, ending back at Manual, and no new service
installation. The receipt distinguishes that final successful comparison from
the original guard failure. The historical 9ea run retained only a service
fingerprint, so its more limited conclusion remains unchanged.

This lifecycle run does not exercise interactive installer UI, UAC or
SmartScreen, the first graphical application start, machine-wide MSI
installation, an upgrade while the application is active, actual CLI updater
replacement or rollback, power-loss recovery or physical Windows hardware.
Stable release signing and release artifact qualification remain separate
gates. The interactive path is covered separately below.

## Interactive installer and first graphical start

Later on 2026-10-14, the same unsigned installer
(`d0220e5fb1ab462b94649d78e3dd7e31de7d78557eaf4a7d4b2eea0ef2af71bd`) was run
interactively by another new standard user, in a second console session of the
same VM, while the existing user's session stayed signed in. The
[receipt](evaluation/windows-gui-e6d9cc8-2026-10-14.json) records the evidence
hashes and the scripts used.

The installer showed its welcome, license, install location under the user's
LocalAppData and completion pages, and exited with code 0. No UAC prompt
appeared, which matches current-user mode. The installed executable matched the
CI extraction (`70def19b…`) and the HKCU registration reported 0.7.1. The first
graphical start ran one application process in the temporary user's session,
with its WebView2 processes, and showed the overview, Settings and the rendered
AGPL license text. The tray menu offered Open dashboard, Settings, Start agent
host and Quit Spanreed. The Windows Application log recorded no Spanreed or
WebView2 error during the run.

The graphical uninstaller exited with code 0. It removed the installation
directory, registration and desktop shortcut, left no application process, and
kept the four user data files, as its default option states.

Cleanup removed the temporary account, profile, profile directory and
ProfileList key, its registry hives and its access entries on the test
directory, without a reboot. The account never had extra logon rights and
owned no scheduled task. While it ran, Service Control Manager switched
`IsolationSession` from Manual to Auto, so the cleanup guard reported a service
difference. The service returned to Manual 34 seconds later without
intervention. BITS had shown the same kind of toggle before the run started.
A later read-only comparison found the existing user's files and registration,
the machine registration, Defender, the security policy, and both the raw and
normalized service inventories equal to the state before the run. The receipt
keeps both results and the Service Control Manager events. It does not
attribute these toggles to Spanreed, but it does not prove their cause either.

The run was driven through Syl by Codex agents until the graphical uninstall.
Cleanup and the final comparison were finished afterwards through the QEMU
guest agent as LocalSystem.

The interactive run still does not cover SmartScreen, because the locally
copied installer had no Mark of the Web. It also does not cover a signed
release artifact, machine-wide MSI installation, upgrade while the application
is running, CLI updater replacement or rollback, power loss, physical Windows
hardware, or Windows accessibility tools.
