//! Cross-platform OS keyring access (the "SecretStore" seam).
//!
//! Several providers persist their OAuth blob in the OS keyring as a fallback
//! to a plaintext credentials file. The native keyring differs per OS:
//!
//! - **Linux**: the Secret Service via the `secret-tool` CLI (libsecret).
//! - **macOS**: the login Keychain via the `security` CLI.
//! - **Windows**: the Windows Credential Manager via the `cmdkey` CLI for
//!   detection and a small PowerShell snippet (CredRead) to read the blob.
//!
//! All three back ends shell out to a platform tool rather than linking a
//! keyring crate, matching the rest of the codebase's zero-extra-dep approach.
//! Each function keys off a single logical `service` string (the same value
//! used as the Secret Service `service` attribute on Linux); the per-OS back
//! ends map it to their native account/target name.
//!
//! Username-aware variants (`lookup_user` / `store_user` / `delete_user`) match
//! the go-keyring / `gh` layout: Linux attributes `service` + `username`,
//! macOS `-s` service + `-a` account, Windows target `spanreed:{service}:{user}`.
//!
//! Callers should treat the keyring as best-effort: every function returns
//! `Option`/`bool` and never panics, so a missing tool or absent item simply
//! means "no credential here".

/// Read a secret for `service` from the OS keyring.
///
/// Returns `None` if the platform tool is missing, the item does not exist, or
/// the stored value is empty.
pub fn lookup(service: &str) -> Option<String> {
    platform::lookup(service)
}

/// Read a secret for `(service, username)` — the multi-account keyring layout
/// used by `gh` (`service=gh:github.com`, `username=<login>`).
pub fn lookup_user(service: &str, username: &str) -> Option<String> {
    platform::lookup_user(service, username)
}

/// Store `secret` under `service` (with a human-readable `label` where the
/// platform supports one). Best-effort: returns `false` on any failure.
pub fn store(service: &str, label: &str, secret: &str) -> bool {
    platform::store(service, label, secret)
}

/// Store `secret` under `(service, username)`.
#[allow(dead_code)] // multi-account write path; used by auth importers
pub fn store_user(service: &str, username: &str, label: &str, secret: &str) -> bool {
    platform::store_user(service, username, label, secret)
}

/// Is there a stored secret for `service`? Used by `detect()`.
pub fn exists(service: &str) -> bool {
    platform::exists(service)
}

/// Delete a secret for `service`. Best-effort.
pub fn delete(service: &str) -> bool {
    platform::delete(service)
}

/// Delete a secret for `(service, username)`. Best-effort.
#[allow(dead_code)] // multi-account clear path; used by auth logout variants
pub fn delete_user(service: &str, username: &str) -> bool {
    platform::delete_user(service, username)
}

#[cfg(target_os = "linux")]
mod platform {
    use std::io::Write;
    use std::process::{Command, Stdio};

    pub fn lookup(service: &str) -> Option<String> {
        let out = Command::new("secret-tool")
            .args(["lookup", "service", service])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    pub fn lookup_user(service: &str, username: &str) -> Option<String> {
        let out = Command::new("secret-tool")
            .args(["lookup", "service", service, "username", username])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    pub fn store(service: &str, label: &str, secret: &str) -> bool {
        let child = Command::new("secret-tool")
            .args(["store", "--label", label, "service", service])
            .stdin(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(_) => return false,
        };
        if let Some(stdin) = child.stdin.as_mut() {
            if stdin.write_all(secret.as_bytes()).is_err() {
                return false;
            }
        }
        child.wait().map(|s| s.success()).unwrap_or(false)
    }

    pub fn store_user(service: &str, username: &str, label: &str, secret: &str) -> bool {
        let child = Command::new("secret-tool")
            .args([
                "store", "--label", label, "service", service, "username", username,
            ])
            .stdin(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(_) => return false,
        };
        if let Some(stdin) = child.stdin.as_mut() {
            if stdin.write_all(secret.as_bytes()).is_err() {
                return false;
            }
        }
        child.wait().map(|s| s.success()).unwrap_or(false)
    }

    pub fn exists(service: &str) -> bool {
        lookup(service).is_some()
    }

    pub fn delete(service: &str) -> bool {
        Command::new("secret-tool")
            .args(["clear", "service", service])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn delete_user(service: &str, username: &str) -> bool {
        Command::new("secret-tool")
            .args(["clear", "service", service, "username", username])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::process::Command;

    pub fn lookup(service: &str) -> Option<String> {
        // `-w` prints only the password; `-s` matches the generic-password
        // service. Exit status is non-zero when no matching item exists.
        let out = Command::new("security")
            .args(["find-generic-password", "-s", service, "-w"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    pub fn lookup_user(service: &str, username: &str) -> Option<String> {
        let out = Command::new("security")
            .args(["find-generic-password", "-s", service, "-a", username, "-w"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    pub fn store(service: &str, label: &str, secret: &str) -> bool {
        // `-U` updates the item if it already exists; `-l` sets the display
        // label, `-a` the account (reuse the service so lookups by `-s` match).
        Command::new("security")
            .args([
                "add-generic-password",
                "-U",
                "-s",
                service,
                "-a",
                service,
                "-l",
                label,
                "-w",
                secret,
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn store_user(service: &str, username: &str, label: &str, secret: &str) -> bool {
        Command::new("security")
            .args([
                "add-generic-password",
                "-U",
                "-s",
                service,
                "-a",
                username,
                "-l",
                label,
                "-w",
                secret,
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn exists(service: &str) -> bool {
        lookup(service).is_some()
    }

    pub fn delete(service: &str) -> bool {
        Command::new("security")
            .args(["delete-generic-password", "-s", service, "-a", service])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn delete_user(service: &str, username: &str) -> bool {
        Command::new("security")
            .args(["delete-generic-password", "-s", service, "-a", username])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(target_os = "windows")]
#[path = "secret_windows.rs"]
mod platform;

// Fallback for any other OS: no keyring integration.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    pub fn lookup(_service: &str) -> Option<String> {
        None
    }
    pub fn lookup_user(_service: &str, _username: &str) -> Option<String> {
        None
    }
    pub fn store(_service: &str, _label: &str, _secret: &str) -> bool {
        false
    }
    pub fn store_user(_service: &str, _username: &str, _label: &str, _secret: &str) -> bool {
        false
    }
    pub fn exists(_service: &str) -> bool {
        false
    }
    pub fn delete(_service: &str) -> bool {
        false
    }
    pub fn delete_user(_service: &str, _username: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A service name no real keyring item will use. Exercising the public API
    // here means the per-OS back end (whichever this test runs on, including the
    // macOS/Windows CI `cross` job) is invoked end-to-end and must not panic and
    // must report "absent" cleanly.
    const ABSENT: &str = "spanreed-nonexistent-service-d4e8f1a2";

    #[test]
    fn lookup_absent_is_none_and_does_not_panic() {
        assert!(lookup(ABSENT).is_none());
    }

    #[test]
    fn lookup_user_absent_is_none() {
        assert!(lookup_user(ABSENT, "nobody").is_none());
    }

    #[test]
    fn exists_absent_is_false() {
        assert!(!exists(ABSENT));
    }
}
