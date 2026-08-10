//! Persist `GROK_CLI_CHAT_PROXY_BASE_URL` so Grok Build hits the capture proxy.

use super::state::{GrokWireState, SetupState};
use super::GROK_CAPTURE_BASE_URL;

pub const ENV_KEY: &str = "GROK_CLI_CHAT_PROXY_BASE_URL";

#[cfg_attr(windows, allow(dead_code))]
const BLOCK_BEGIN: &str = "# >>> spanreed capture >>>";
#[cfg_attr(windows, allow(dead_code))]
const BLOCK_END: &str = "# <<< spanreed capture <<<";

pub fn status_line(state: &SetupState) -> String {
    // Prefer process env, then persisted user/shell config.
    let live = read_live_value().or_else(read_persistent_value);
    match live {
        Some(v) if v == GROK_CAPTURE_BASE_URL => format!("wired → {v}"),
        Some(v) => format!("env={v}"),
        None => {
            if state.wired.grok.is_some() {
                "state says wired (restart shell?)".into()
            } else {
                "not set".into()
            }
        }
    }
}

/// True when Grok is configured to hit the local capture proxy.
pub fn is_wired_to_capture(state: &SetupState) -> bool {
    let live = read_live_value().or_else(read_persistent_value);
    match live {
        Some(v) => v == GROK_CAPTURE_BASE_URL || v.contains("127.0.0.1:18736"),
        None => state
            .wired
            .grok
            .as_ref()
            .map(|g| g.current == GROK_CAPTURE_BASE_URL)
            .unwrap_or(false),
    }
}

pub fn wire(dry_run: bool, state: &mut SetupState) -> Result<String, String> {
    let previous = read_persistent_value();
    if previous.as_deref() == Some(GROK_CAPTURE_BASE_URL) {
        state.wired.grok = Some(GrokWireState {
            previous: state
                .wired
                .grok
                .as_ref()
                .and_then(|g| g.previous.clone())
                .or(previous),
            current: GROK_CAPTURE_BASE_URL.into(),
        });
        return Ok(format!("already wired ({ENV_KEY})"));
    }

    if dry_run {
        return Ok(format!(
            "would set {ENV_KEY}={GROK_CAPTURE_BASE_URL}"
        ));
    }

    platform::set_env(GROK_CAPTURE_BASE_URL)?;
    state.wired.grok = Some(GrokWireState {
        previous,
        current: GROK_CAPTURE_BASE_URL.into(),
    });
    Ok(format!("set {ENV_KEY}={GROK_CAPTURE_BASE_URL}"))
}

pub fn unwire(dry_run: bool, state: &mut SetupState) -> Result<String, String> {
    let Some(wire) = state.wired.grok.clone() else {
        // Still try to clear if live value is ours.
        let live = read_persistent_value();
        if live.as_deref() != Some(GROK_CAPTURE_BASE_URL) {
            return Ok("nothing to unwire".into());
        }
        if dry_run {
            return Ok(format!("would clear {ENV_KEY}"));
        }
        platform::clear_env(None)?;
        return Ok(format!("cleared {ENV_KEY}"));
    };

    if dry_run {
        return Ok(match &wire.previous {
            Some(p) => format!("would restore {ENV_KEY}={p}"),
            None => format!("would clear {ENV_KEY}"),
        });
    }

    platform::clear_env(wire.previous.as_deref())?;
    state.wired.grok = None;
    Ok(match wire.previous {
        Some(p) => format!("restored {ENV_KEY}={p}"),
        None => format!("cleared {ENV_KEY}"),
    })
}

fn read_live_value() -> Option<String> {
    std::env::var(ENV_KEY).ok().filter(|s| !s.is_empty())
}

fn read_persistent_value() -> Option<String> {
    platform::get_env().or_else(read_live_value)
}

/// Idempotent managed shell block for Unix profiles.
#[cfg_attr(windows, allow(dead_code))]
pub fn managed_shell_block() -> String {
    format!("{BLOCK_BEGIN}\nexport {ENV_KEY}=\"{GROK_CAPTURE_BASE_URL}\"\n{BLOCK_END}\n")
}

#[cfg_attr(windows, allow(dead_code))]
pub fn upsert_shell_block(content: &str) -> String {
    let stripped = remove_shell_block(content);
    let mut out = stripped.trim_end().to_string();
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&managed_shell_block());
    out
}

#[cfg_attr(windows, allow(dead_code))]
pub fn remove_shell_block(content: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in content.lines() {
        if line.trim() == BLOCK_BEGIN {
            skipping = true;
            continue;
        }
        if line.trim() == BLOCK_END {
            skipping = false;
            continue;
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::process::Command;

    pub fn get_env() -> Option<String> {
        let script = format!(
            "[Environment]::GetEnvironmentVariable('{ENV_KEY}','User')"
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    pub fn set_env(value: &str) -> Result<(), String> {
        let escaped = value.replace('\'', "''");
        let script = format!(
            "[Environment]::SetEnvironmentVariable('{ENV_KEY}','{escaped}','User')"
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .map_err(|e| format!("powershell set env: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "set {ENV_KEY}: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    }

    pub fn clear_env(restore: Option<&str>) -> Result<(), String> {
        match restore {
            Some(v) => set_env(v),
            None => {
                let script = format!(
                    "[Environment]::SetEnvironmentVariable('{ENV_KEY}',$null,'User')"
                );
                let out = Command::new("powershell")
                    .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                    .output()
                    .map_err(|e| format!("powershell clear env: {e}"))?;
                if !out.status.success() {
                    return Err(format!(
                        "clear {ENV_KEY}: {}",
                        String::from_utf8_lossy(&out.stderr)
                    ));
                }
                Ok(())
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    use std::path::PathBuf;

    pub fn get_env() -> Option<String> {
        // environment.d
        if let Some(v) = read_environment_d() {
            return Some(v);
        }
        // shell profiles
        for p in profile_paths() {
            if let Ok(text) = std::fs::read_to_string(&p) {
                if text.contains(BLOCK_BEGIN) {
                    return Some(GROK_CAPTURE_BASE_URL.to_string());
                }
            }
        }
        None
    }

    pub fn set_env(value: &str) -> Result<(), String> {
        // Prefer systemd user environment.d when the directory exists or we can create it.
        write_environment_d(value)?;
        // Also patch common interactive shells so CLI sessions pick it up.
        for p in profile_paths() {
            patch_profile(&p, true)?;
        }
        Ok(())
    }

    pub fn clear_env(restore: Option<&str>) -> Result<(), String> {
        match restore {
            Some(v) if !v.is_empty() => {
                write_environment_d(v)?;
            }
            _ => {
                remove_environment_d()?;
            }
        }
        for p in profile_paths() {
            if p.exists() {
                patch_profile(&p, false)?;
            }
        }
        Ok(())
    }

    fn environment_d_path() -> PathBuf {
        crate::creds::expand("~/.config/environment.d/90-spanreed-grok.conf")
    }

    fn read_environment_d() -> Option<String> {
        let text = std::fs::read_to_string(environment_d_path()).ok()?;
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix(&format!("{ENV_KEY}=")) {
                let v = rest.trim().trim_matches('"').to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
        None
    }

    fn write_environment_d(value: &str) -> Result<(), String> {
        let path = environment_d_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir environment.d: {e}"))?;
        }
        let body = format!("{ENV_KEY}={value}\n");
        std::fs::write(&path, body).map_err(|e| format!("write environment.d: {e}"))
    }

    fn remove_environment_d() -> Result<(), String> {
        let path = environment_d_path();
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("remove environment.d: {e}"))?;
        }
        Ok(())
    }

    fn profile_paths() -> Vec<PathBuf> {
        let home = dirs::home_dir().unwrap_or_else(|| crate::creds::expand("~"));
        vec![
            home.join(".bashrc"),
            home.join(".zshrc"),
            home.join(".zprofile"),
            home.join(".bash_profile"),
            home.join(".profile"),
        ]
    }

    fn patch_profile(path: &PathBuf, enable: bool) -> Result<(), String> {
        let existing = if path.exists() {
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?
        } else if enable {
            String::new()
        } else {
            return Ok(());
        };

        let new_content = if enable {
            upsert_shell_block(&existing)
        } else {
            remove_shell_block(&existing)
        };

        if new_content == existing {
            return Ok(());
        }
        // Only create profile files that already existed, except .profile/.bashrc when enabling.
        if !path.exists() && enable {
            // Prefer not to create every shell file; only touch if parent home exists and
            // file is .bashrc or .profile (most common login paths).
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !matches!(name, ".bashrc" | ".profile" | ".zshrc") {
                return Ok(());
            }
        }
        std::fs::write(path, new_content).map_err(|e| format!("write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_block_idempotent() {
        let once = upsert_shell_block("");
        let twice = upsert_shell_block(&once);
        assert_eq!(once.matches(BLOCK_BEGIN).count(), 1);
        assert_eq!(twice.matches(BLOCK_BEGIN).count(), 1);
        assert!(twice.contains(GROK_CAPTURE_BASE_URL));
    }

    #[test]
    fn shell_block_remove() {
        let with = upsert_shell_block("export FOO=1\n");
        let gone = remove_shell_block(&with);
        assert!(!gone.contains(BLOCK_BEGIN));
        assert!(gone.contains("export FOO=1"));
    }
}
