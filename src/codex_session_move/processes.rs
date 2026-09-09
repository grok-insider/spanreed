fn client(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = lower.trim_end_matches(".exe");
    matches!(
        base,
        "codex" | "codex.js" | "opencode" | "opencode.js" | "spanreed" | "spanreed-desktop"
    ) || base.starts_with("codex-")
}
#[cfg(target_os = "linux")]
pub(super) fn ensure_idle() -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let own = std::fs::metadata("/proc/self")
        .map_err(|_| "Cannot verify running clients")?
        .uid();
    for entry in std::fs::read_dir("/proc").map_err(|_| "Cannot enumerate running clients")? {
        let entry = entry.map_err(|_| "Cannot enumerate running clients")?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("Cannot inspect a running client".into()),
        };
        if metadata.uid() != own {
            continue;
        }
        let args = match std::fs::read(entry.path().join("cmdline")) {
            Ok(a) => a,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                return Err("Cannot inspect running clients; use independent authorization".into())
            }
        };
        if args.split(|b| *b == 0).take(3).any(|arg| {
            std::path::Path::new(&String::from_utf8_lossy(arg).into_owned())
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(client)
        }) {
            return Err("Close Codex, OpenCode and other Spanreed processes before moving this session, or use independent hosted authorization".into());
        }
    }
    Ok(())
}
#[cfg(target_os = "windows")]
pub(super) fn ensure_idle() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let output=std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command","$ErrorActionPreference='Stop'; @(Get-CimInstance Win32_Process | Select-Object ProcessId,Name) | ConvertTo-Json -Compress"]).creation_flags(0x08000000).output().map_err(|_|"Cannot enumerate running clients")?;
    if !output.status.success() {
        return Err("Cannot enumerate running clients; use independent authorization".into());
    }
    let rows: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).map_err(|_| "Cannot inspect running clients")?;
    for row in rows {
        let pid = row["ProcessId"]
            .as_u64()
            .ok_or("Invalid process identity")?;
        let name = row["Name"].as_str().ok_or("Invalid process name")?;
        if pid != u64::from(std::process::id()) && client(name) {
            return Err(
                "Close Codex, OpenCode and other Spanreed processes before moving this session"
                    .into(),
            );
        }
    }
    Ok(())
}
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(super) fn ensure_idle() -> Result<(), String> {
    Err(
        "Session moves require a qualified Linux or Windows host; use independent authorization"
            .into(),
    )
}

#[cfg(target_os = "linux")]
pub(super) fn ensure_no_keyring() -> Result<(), String> {
    let result = std::process::Command::new("secret-tool")
        .args(["lookup", "service", "Codex Auth"])
        .output()
        .map_err(|_| {
            "Cannot inspect the system credential store; use independent hosted authorization"
        })?;
    if result.status.code() == Some(1) && result.stdout.is_empty() && result.stderr.is_empty() {
        return Ok(());
    }
    Err("A system credential exists or its absence cannot be verified; use independent hosted authorization".into())
}
#[cfg(target_os = "windows")]
pub(super) fn ensure_no_keyring() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let result = std::process::Command::new("cmdkey.exe")
        .arg("/list")
        .creation_flags(0x08000000)
        .output()
        .map_err(|_| "Cannot inspect the system credential store")?;
    if !result.status.success()
        || String::from_utf8_lossy(&result.stdout)
            .to_ascii_lowercase()
            .contains("codex")
    {
        return Err("A system credential exists or its absence cannot be verified; use independent hosted authorization".into());
    }
    Ok(())
}
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(super) fn ensure_no_keyring() -> Result<(), String> {
    Err("System credential inspection is not qualified on this platform".into())
}
