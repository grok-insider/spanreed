# Windows NSIS lifecycle qualification

On 2026-10-14, the unsigned desktop artifact built from
`9eaabe27b684ef468bd67a809a0cafd92bc7da5a` passed 25 checks in the existing
Windows 11 QA VM. The [receipt](evaluation/windows-nsis-2026-10-14.json) records
the exact installer, installed executable, license bytes and evidence hashes.
This result applies to that artifact; it does not qualify a later source or
embedded Portable bundle.

The test ran as a newly created standard user, with the administrator role
inactive. It used the installer's current-user mode and the HKCU uninstall
registration. The sequence was a clean installation of published 0.7.0, an
upgrade to the candidate 0.7.1, repair with the same candidate and normal
uninstallation. Installations used `/S /D=<temporary-user-install-directory>`;
uninstallation used `/S`.

The installed executable matched the one extracted from the CI artifact.
Upgrade replaced the old executable and left exactly one registration with the
candidate version. Repair preserved both the executable and synthetic user
state. Normal uninstallation removed the binary, installation directory and
registration while retaining that state. The inventory contains hashes of
420 installed files, including the original application license, native notices
and crate source archives.

Two initial harness failures were corrected before the complete successful
run: PowerShell 5 requires an array wrapper when counting a single registry
entry, and the NSIS uninstaller can exit before its child removes the
registration. The final harness waits for both the directory and registration
to disappear. The successful run began with neither present.

Cleanup removed the temporary task, batch-logon right, account, profile,
profile directory and registry hives. Windows User Profile Service retained
the hives after the task finished, so cleanup needed a clean reboot of the QA
VM. Before reboot, the account was disabled and no application unrelated to
the Windows shell was running. The reboot did not force application closure.

The existing user's 351 inventoried files and Spanreed registration, the
machine-wide Spanreed registration and Defender service status/start modes
were preserved. The aggregate fingerprint of all Windows service definitions
changed across reboot. Windows recreated its per-user services, but the prior
full service inventory was not retained, so the exact difference cannot be
established. The original cleanup guard's failing result is retained; the
final receipt reports resource removal and the narrower successful baseline
checks separately. It does not assert equality of every service definition.

This does not exercise interactive installer UI, UAC or SmartScreen, the first
graphical application start, machine-wide MSI installation, an upgrade while
the application is active, actual CLI updater replacement or rollback,
power-loss recovery or physical Windows hardware. Stable release signing and
release artifact qualification remain separate gates.
