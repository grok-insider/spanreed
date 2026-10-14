# Windows transport and ACP lifetime fixtures

The focused native tests passed in the existing Windows 11 QA VM with a limited
user token and isolated application-state directories. The
[receipt](evaluation/windows-native-fixtures-2026-10-14.json) binds the compiled
executables, test sources, archived checkout and cleanup results.

The relay fixture now traverses the production embedded listener, including its
FIN and bounded-drain shutdown. The previous fixture called the connection
handler directly and could receive Windows connection-reset error 10054 when
reading through EOF. Both forwarding modes still require EOF, the complete
chunked response, exact JSON, upstream authentication and usage accounting.
Four selected tests passed. Production relay code is unchanged.

The ACP lifetime test passed with its retained handle: explicit shutdown ends
the isolated Windows Job, while a Shared descendant survives host shutdown.
The test then removes its own Shared descendant. A second shutdown remains
idempotent. The temporary scheduled tasks were removed, and no synthetic
fixture process remained.

These are focused native fixtures. Full Windows CI, installer qualification,
desktop UI, physical hardware and the Windows updater release gate remain
separate checks.
