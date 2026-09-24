//! Hand a page to the desktop window, starting it when it is not already running.

use std::path::PathBuf;
use std::process::Command;

const LOCK: &str = "desktop.lock";
const ROUTE: &str = "desktop-route";
const ALERT: &str = "desktop-alert";
const ALERT_ACK: &str = "desktop-alert-ack";

/// Queue a local desktop page and make sure the window process is up.
pub fn request(page: &str) -> Result<(), String> {
    let href = href(page)?;
    let dir = crate::app::data_dir();
    std::fs::create_dir_all(&dir).map_err(|error| format!("mkdir desktop route: {error}"))?;
    std::fs::write(dir.join(ROUTE), href)
        .map_err(|error| format!("write desktop route: {error}"))?;
    if desktop_alive() {
        return Ok(());
    }
    spawn_desktop()
}

/// Read and clear a queued route. The desktop window applies it once.
pub fn take() -> Option<String> {
    let path = crate::app::data_dir().join(ROUTE);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let text = text.trim();
    text.starts_with("#/local/").then(|| text.to_string())
}

pub fn mark_running() {
    let dir = crate::app::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(LOCK), format!("{}\n", std::process::id()));
}

pub struct PendingAlert {
    pub id: String,
    pub title: String,
    pub body: String,
}

/// Ask a running desktop to show an alert. False when it is not running or
/// does not acknowledge in time, so the caller can deliver it directly.
pub fn hand_off_alert(title: &str, body: &str) -> bool {
    if !desktop_alive() {
        return false;
    }
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos().to_string())
        .unwrap_or_else(|_| "0".into());
    let dir = crate::app::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::remove_file(dir.join(ALERT_ACK));
    let payload = format!("{id}\n{}\n{}", one_line(title), one_line(body));
    if std::fs::write(dir.join(ALERT), payload).is_err() {
        return false;
    }
    for _ in 0..16 {
        if std::fs::read_to_string(dir.join(ALERT_ACK))
            .ok()
            .is_some_and(|text| text.trim() == id)
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

pub fn take_alert() -> Option<PendingAlert> {
    let path = crate::app::data_dir().join(ALERT);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let mut lines = text.lines();
    let id = lines.next()?.trim().to_string();
    if id.is_empty() {
        return None;
    }
    let title = lines.next().unwrap_or("").to_string();
    let body = lines.collect::<Vec<_>>().join("\n");
    Some(PendingAlert { id, title, body })
}

pub fn ack_alert(id: &str) {
    let dir = crate::app::data_dir();
    let _ = std::fs::write(dir.join(ALERT_ACK), id);
}

fn one_line(value: &str) -> String {
    value.replace(['\n', '\r'], " ").chars().take(240).collect()
}

pub fn unmark_running() {
    let path = crate::app::data_dir().join(LOCK);
    if std::fs::read_to_string(&path)
        .ok()
        .is_some_and(|text| text.trim() == std::process::id().to_string())
    {
        let _ = std::fs::remove_file(&path);
    }
}

fn href(page: &str) -> Result<String, String> {
    match page {
        "overview" | "usage" | "accounts" | "routing" | "connect" | "settings" => {
            Ok(format!("#/local/{page}"))
        }
        _ => Err("Unknown desktop page".into()),
    }
}

fn desktop_alive() -> bool {
    let path = crate::app::data_dir().join(LOCK);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(pid) = text.trim().parse::<u32>() else {
        return false;
    };
    pid == std::process::id() || process_alive(pid)
}

fn spawn_desktop() -> Result<(), String> {
    let name = if cfg!(windows) {
        "spanreed-desktop.exe"
    } else {
        "spanreed-desktop"
    };
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("SPANREED_DESKTOP") {
        if !path.is_empty() {
            candidates.push(PathBuf::from(path));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(name));
            #[cfg(target_os = "macos")]
            candidates.push(dir.join("../Spanreed.app/Contents/MacOS/spanreed-desktop"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Applications/Spanreed.app/Contents/MacOS/spanreed-desktop",
        ));
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(
                PathBuf::from(home)
                    .join("Applications/Spanreed.app/Contents/MacOS/spanreed-desktop"),
            );
        }
    }
    candidates.push(PathBuf::from(name));
    let mut last = "Install the desktop package.".to_string();
    for path in candidates {
        match Command::new(&path).spawn() {
            Ok(_) => return Ok(()),
            Err(error) => last = error.to_string(),
        }
    }
    Err(format!("Could not open Spanreed Desktop: {last}"))
}

fn process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn with_data_home(f: impl FnOnce(&std::path::Path)) {
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "spanreed-desktop-open-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let previous = std::env::var_os("XDG_DATA_HOME");
        std::env::set_var("XDG_DATA_HOME", &dir);
        f(&dir);
        unmark_running();
        match previous {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn request_queues_a_local_overview_route() {
        with_data_home(|dir| {
            let error = request("overview").unwrap_err();
            assert!(error.contains("Spanreed Desktop") || error.contains("spanreed-desktop"));
            let queued = std::fs::read_to_string(dir.join("spanreed").join(ROUTE)).unwrap();
            assert_eq!(queued, "#/local/overview");
            assert_eq!(take().as_deref(), Some("#/local/overview"));
            assert!(take().is_none());
            let error = request("settings").unwrap_err();
            assert!(error.contains("Spanreed Desktop") || error.contains("spanreed-desktop"));
            assert_eq!(
                std::fs::read_to_string(dir.join("spanreed").join(ROUTE)).unwrap(),
                "#/local/settings"
            );
            assert!(request("nope").is_err());
        });
    }

    #[test]
    fn request_reuses_a_running_desktop_for_both_pages() {
        with_data_home(|_| {
            mark_running();
            request("overview").expect("running desktop accepts overview");
            assert_eq!(take().as_deref(), Some("#/local/overview"));
            request("settings").expect("running desktop accepts settings");
            assert_eq!(take().as_deref(), Some("#/local/settings"));
            assert!(take().is_none());
        });
    }

    #[test]
    fn running_desktop_accepts_an_alert() {
        with_data_home(|_| {
            assert!(!hand_off_alert("Capture proxy is DOWN", "Ensure capture"));
            mark_running();
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stop_worker = stop.clone();
            let worker = std::thread::spawn(move || {
                while !stop_worker.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Some(alert) = take_alert() {
                        assert_eq!(alert.title, "Capture proxy is DOWN");
                        ack_alert(&alert.id);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            });
            assert!(hand_off_alert(
                "Capture proxy is DOWN",
                "Ensure capture before new hops."
            ));
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            worker.join().unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn request_starts_the_desktop_named_by_the_environment() {
        with_data_home(|dir| {
            std::fs::create_dir_all(dir).unwrap();
            let stub = dir.join("spanreed-desktop");
            std::fs::copy("/bin/true", &stub).unwrap();
            let mut permissions = std::fs::metadata(&stub).unwrap().permissions();
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o755);
            std::fs::set_permissions(&stub, permissions).unwrap();
            let previous = std::env::var_os("SPANREED_DESKTOP");
            std::env::set_var("SPANREED_DESKTOP", &stub);
            request("overview").expect("configured desktop starts");
            assert_eq!(take().as_deref(), Some("#/local/overview"));
            std::thread::sleep(std::time::Duration::from_millis(50));
            match previous {
                Some(value) => std::env::set_var("SPANREED_DESKTOP", value),
                None => std::env::remove_var("SPANREED_DESKTOP"),
            }
        });
    }

    #[cfg(unix)]
    #[test]
    fn request_starts_a_desktop_binary_on_path() {
        with_data_home(|dir| {
            let bin = dir.join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let stub = bin.join("spanreed-desktop");
            std::fs::copy("/bin/true", &stub).unwrap();
            let mut permissions = std::fs::metadata(&stub).unwrap().permissions();
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o755);
            std::fs::set_permissions(&stub, permissions).unwrap();
            let previous = std::env::var_os("PATH");
            let joined = match &previous {
                Some(value) => {
                    let mut path = std::ffi::OsString::from(bin.as_os_str());
                    path.push(":");
                    path.push(value);
                    path
                }
                None => bin.as_os_str().to_os_string(),
            };
            std::env::set_var("PATH", joined);
            request("settings").expect("stub desktop starts");
            assert_eq!(take().as_deref(), Some("#/local/settings"));
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = std::fs::remove_dir_all(&bin);
            match previous {
                Some(value) => std::env::set_var("PATH", value),
                None => std::env::remove_var("PATH"),
            }
        });
    }
}
