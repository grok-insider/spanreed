//! Cross-platform process & listening-port discovery (the "ProcessList" seam).
//!
//! Some providers (e.g. Antigravity) discover a locally-running language server
//! by scanning processes for a marker, reading a CLI flag off its command line,
//! and finding the TCP ports it listens on. The mechanism is OS-specific:
//!
//! - **Linux**: parse `/proc` (cmdlines, `/proc/<pid>/fd` socket inodes,
//!   `/proc/net/tcp{,6}`). No external tools.
//! - **macOS**: shell out to `ps` (cmdlines) and `lsof` (listening ports).
//! - **Windows / other**: empty stubs — local language-server discovery is not
//!   yet supported, so dependent providers degrade gracefully (they simply
//!   report "not detected") rather than failing to build.
//!
//! [`extract_flag`] is pure string parsing and shared across all platforms.

/// A running process discovered from the OS.
pub struct ProcInfo {
    pub pid: i32,
    /// Full command line (argv joined by spaces).
    pub cmdline: String,
}

/// Find processes whose command line contains all of `needles`
/// (case-insensitive). Returns matches with their full command line so callers
/// can extract flags like `--csrf_token`.
pub fn find_processes(needles: &[&str]) -> Vec<ProcInfo> {
    platform::find_processes(needles)
}

/// Find TCP ports `pid` is listening on (localhost). Empty on platforms without
/// process introspection support.
pub fn listening_ports(pid: i32) -> Vec<u16> {
    platform::listening_ports(pid)
}

/// Extract a CLI flag value from a command line. Handles `--flag value` and
/// `--flag=value`. Platform-independent.
pub fn extract_flag(cmdline: &str, flag: &str) -> Option<String> {
    let parts: Vec<&str> = cmdline.split_whitespace().collect();
    let flag_eq = format!("{flag}=");
    for (i, part) in parts.iter().enumerate() {
        if *part == flag {
            if let Some(next) = parts.get(i + 1) {
                return Some(next.to_string());
            }
        } else if let Some(rest) = part.strip_prefix(&flag_eq) {
            return Some(rest.to_string());
        }
    }
    None
}

#[cfg(target_os = "linux")]
mod platform {
    use super::ProcInfo;
    use std::collections::HashSet;

    pub fn find_processes(needles: &[&str]) -> Vec<ProcInfo> {
        let mut out = Vec::new();
        let proc = match std::fs::read_dir("/proc") {
            Ok(p) => p,
            Err(_) => return out,
        };
        for entry in proc.flatten() {
            let name = entry.file_name();
            let pid: i32 = match name.to_str().and_then(|s| s.parse().ok()) {
                Some(p) => p,
                None => continue,
            };
            let cmdline_path = entry.path().join("cmdline");
            let raw = match std::fs::read(&cmdline_path) {
                Ok(r) => r,
                Err(_) => continue,
            };
            // /proc cmdline is NUL-separated.
            let cmdline: String = raw
                .split(|b| *b == 0)
                .map(|seg| String::from_utf8_lossy(seg).into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            let lower = cmdline.to_lowercase();
            if needles.iter().all(|n| lower.contains(&n.to_lowercase())) {
                out.push(ProcInfo { pid, cmdline });
            }
        }
        out
    }

    pub fn listening_ports(pid: i32) -> Vec<u16> {
        // Collect socket inodes owned by the pid.
        let fd_dir = format!("/proc/{pid}/fd");
        let mut inodes: HashSet<u64> = HashSet::new();
        if let Ok(fds) = std::fs::read_dir(&fd_dir) {
            for fd in fds.flatten() {
                if let Ok(target) = std::fs::read_link(fd.path()) {
                    let t = target.to_string_lossy();
                    if let Some(inode) =
                        t.strip_prefix("socket:[").and_then(|s| s.strip_suffix(']'))
                    {
                        if let Ok(n) = inode.parse::<u64>() {
                            inodes.insert(n);
                        }
                    }
                }
            }
        }
        if inodes.is_empty() {
            return Vec::new();
        }

        let mut ports = HashSet::new();
        for tcp_file in ["/proc/net/tcp", "/proc/net/tcp6"] {
            let content = match std::fs::read_to_string(tcp_file) {
                Ok(c) => c,
                Err(_) => continue,
            };
            for line in content.lines().skip(1) {
                let cols: Vec<&str> = line.split_whitespace().collect();
                // local_address  rem_address  st ... inode is column 9.
                if cols.len() < 10 {
                    continue;
                }
                // st == 0A means LISTEN.
                if cols[3] != "0A" {
                    continue;
                }
                let inode: u64 = match cols[9].parse() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                if !inodes.contains(&inode) {
                    continue;
                }
                // local_address is HEXIP:HEXPORT.
                if let Some(port_hex) = cols[1].split(':').nth(1) {
                    if let Ok(port) = u16::from_str_radix(port_hex, 16) {
                        if port > 0 {
                            ports.insert(port);
                        }
                    }
                }
            }
        }
        let mut v: Vec<u16> = ports.into_iter().collect();
        v.sort_unstable();
        v
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::ProcInfo;
    use std::collections::HashSet;
    use std::process::Command;

    pub fn find_processes(needles: &[&str]) -> Vec<ProcInfo> {
        // `ps -axww -o pid=,command=`: every process, full (unwrapped) argv.
        let out = match Command::new("ps")
            .args(["-axww", "-o", "pid=,command="])
            .output()
        {
            Ok(o) if o.status.success() => o,
            _ => return Vec::new(),
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut procs = Vec::new();
        for line in text.lines() {
            let line = line.trim_start();
            let (pid_str, cmdline) = match line.split_once(char::is_whitespace) {
                Some((p, rest)) => (p, rest.trim_start()),
                None => continue,
            };
            let pid: i32 = match pid_str.parse() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let lower = cmdline.to_lowercase();
            if needles.iter().all(|n| lower.contains(&n.to_lowercase())) {
                procs.push(ProcInfo {
                    pid,
                    cmdline: cmdline.to_string(),
                });
            }
        }
        procs
    }

    pub fn listening_ports(pid: i32) -> Vec<u16> {
        // `lsof -nP -iTCP -sTCP:LISTEN -a -p <pid>`: this pid's listening TCP
        // sockets, numeric host/port. NAME column ends in `:<port>` (LISTEN).
        let out = match Command::new("lsof")
            .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-a", "-p", &pid.to_string()])
            .output()
        {
            Ok(o) if o.status.success() => o,
            _ => return Vec::new(),
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut ports: HashSet<u16> = HashSet::new();
        for line in text.lines().skip(1) {
            // NAME is the last column, e.g. `127.0.0.1:42100` or `*:42100`.
            if let Some(name) = line.split_whitespace().last() {
                if let Some(port_str) = name.rsplit(':').next() {
                    if let Ok(port) = port_str.parse::<u16>() {
                        if port > 0 {
                            ports.insert(port);
                        }
                    }
                }
            }
        }
        let mut v: Vec<u16> = ports.into_iter().collect();
        v.sort_unstable();
        v
    }
}

// Windows and any other OS: process introspection not yet implemented. Returns
// empty so providers that rely on local-process discovery degrade gracefully.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    use super::ProcInfo;

    pub fn find_processes(_needles: &[&str]) -> Vec<ProcInfo> {
        Vec::new()
    }

    pub fn listening_ports(_pid: i32) -> Vec<u16> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_flag_space_and_equals() {
        let cmd = "language_server --csrf_token ABC --extension_server_port=42100 --x";
        assert_eq!(extract_flag(cmd, "--csrf_token").as_deref(), Some("ABC"));
        assert_eq!(
            extract_flag(cmd, "--extension_server_port").as_deref(),
            Some("42100")
        );
        assert_eq!(extract_flag(cmd, "--missing"), None);
    }

    // find_processes/listening_ports must never panic on any platform, even for
    // a pid that does not exist. (On Windows/other this hits the empty stub.)
    #[test]
    fn discovery_is_safe_for_bogus_inputs() {
        let _ = find_processes(&["spanreed-no-such-process-xyz"]);
        assert!(listening_ports(-1).is_empty());
    }
}
