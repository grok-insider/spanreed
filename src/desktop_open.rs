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

/// Read a queued route without clearing it. The desktop process uses this to
/// show a hidden window before the page script can run.
pub fn peek() -> Option<String> {
    let text = std::fs::read_to_string(crate::app::data_dir().join(ROUTE)).ok()?;
    let text = text.trim();
    text.starts_with("#/local/").then(|| text.to_string())
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

/// JavaScript that applies a known local route. Unknown text is ignored.
pub fn route_location_script(href: &str) -> Option<String> {
    match href {
        "#/local/overview" | "#/local/usage" | "#/local/accounts" | "#/local/routing"
        | "#/local/connect" | "#/local/settings" => Some(format!(
            "location.hash = {href:?}; dispatchEvent(new HashChangeEvent(\"hashchange\"))"
        )),
        _ => None,
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
    let alive = pid == std::process::id() || process_alive(pid);
    if !alive {
        return false;
    }
    command_confirms_desktop(process_command(pid).as_deref())
}

/// An unreadable command is not the desktop. Otherwise a live pid that is not
/// the app would keep the tray from opening the dashboard.
fn command_confirms_desktop(command: Option<&str>) -> bool {
    command.is_some_and(command_is_desktop)
}

fn command_is_desktop(command: &str) -> bool {
    command.split(['\0', ' ', '\\', '/']).any(|part| {
        desktop_binary_names().iter().any(|name| {
            part == *name
                || part
                    .strip_prefix('.')
                    .and_then(|rest| rest.strip_suffix("-wrapped"))
                    == Some(*name)
        })
    })
}

fn process_command(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        if bytes.is_empty() {
            return None;
        }
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        Some(text.to_string())
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let image = text.split(',').next()?.trim().trim_matches('"');
        if image.is_empty() || image.eq_ignore_ascii_case("INFO:") {
            return None;
        }
        Some(image.to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = pid;
        None
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn spanreed_install_dir(root: impl Into<PathBuf>) -> PathBuf {
    root.into().join("Spanreed")
}

#[cfg(windows)]
fn windows_install_dirs() -> Vec<PathBuf> {
    ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .filter(|value| !value.is_empty())
        .map(spanreed_install_dir)
        .collect()
}

fn desktop_binary_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["spanreed-desktop.exe", "Spanreed.exe"]
    } else {
        &["spanreed-desktop", "Spanreed"]
    }
}

fn spawn_desktop() -> Result<(), String> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("SPANREED_DESKTOP") {
        if !path.is_empty() {
            candidates.push(PathBuf::from(path));
        }
    }
    for name in desktop_binary_names() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join(name));
                #[cfg(target_os = "macos")]
                candidates.push(dir.join("../Spanreed.app/Contents/MacOS").join(name));
            }
        }
        #[cfg(target_os = "macos")]
        {
            candidates.push(PathBuf::from("/Applications/Spanreed.app/Contents/MacOS").join(name));
            if let Some(home) = std::env::var_os("HOME") {
                candidates.push(
                    PathBuf::from(home)
                        .join("Applications/Spanreed.app/Contents/MacOS")
                        .join(name),
                );
            }
        }
        candidates.push(PathBuf::from(name));
        #[cfg(windows)]
        for dir in windows_install_dirs() {
            candidates.push(dir.join(name));
        }
    }
    let mut last = "Install the desktop package.".to_string();
    for path in candidates {
        let Some(path) = existing_desktop(&path) else {
            continue;
        };
        match Command::new(&path).spawn() {
            Ok(_) => return Ok(()),
            Err(error) => last = format!("{}: {error}", path.display()),
        }
    }
    Err(format!("Could not open Spanreed Desktop: {last}"))
}

/// A bare name is only usable when it is a real file on `PATH`.
/// `Spanreed.exe` is also the CLI on Windows, so that file is not a desktop.
fn existing_desktop(path: &std::path::Path) -> Option<PathBuf> {
    let resolved = if path.components().count() == 1 {
        std::env::var_os("PATH").and_then(|entries| {
            std::env::split_paths(&entries).find_map(|dir| {
                let candidate = dir.join(path);
                candidate.is_file().then_some(candidate)
            })
        })?
    } else if path.is_file() {
        path.to_path_buf()
    } else {
        return None;
    };
    (!is_cli_binary(&resolved)).then_some(resolved)
}

fn is_cli_binary(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    if !name.eq_ignore_ascii_case(crate::app::bin_name()) {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let exe = exe.canonicalize().unwrap_or(exe);
    if path == exe || path.parent() == exe.parent().and_then(|dir| dir.parent()) {
        return true;
    }
    // `cargo test` builds spanreed.exe under target/{debug,release} and deps.
    // That file is the CLI, including when Windows matches Spanreed.exe.
    matches!(
        path.parent()
            .and_then(|dir| dir.file_name())
            .and_then(|name| name.to_str()),
        Some("debug" | "release" | "deps")
    )
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

    struct EnvRestore {
        path: Option<std::ffi::OsString>,
        desktop: Option<std::ffi::OsString>,
        #[cfg(windows)]
        local: Option<std::ffi::OsString>,
        #[cfg(windows)]
        program: Option<std::ffi::OsString>,
        #[cfg(windows)]
        program_x86: Option<std::ffi::OsString>,
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            restore_var("PATH", self.path.take());
            restore_var("SPANREED_DESKTOP", self.desktop.take());
            #[cfg(windows)]
            {
                restore_var("LOCALAPPDATA", self.local.take());
                restore_var("ProgramFiles", self.program.take());
                restore_var("ProgramFiles(x86)", self.program_x86.take());
            }
        }
    }

    fn restore_var(name: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

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
            let previous_path = std::env::var_os("PATH");
            let previous_desktop = std::env::var_os("SPANREED_DESKTOP");
            #[cfg(windows)]
            let previous_local = std::env::var_os("LOCALAPPDATA");
            #[cfg(windows)]
            let previous_program = std::env::var_os("ProgramFiles");
            #[cfg(windows)]
            let previous_program_x86 = std::env::var_os("ProgramFiles(x86)");
            let _restore = EnvRestore {
                path: previous_path.clone(),
                desktop: previous_desktop.clone(),
                #[cfg(windows)]
                local: previous_local.clone(),
                #[cfg(windows)]
                program: previous_program.clone(),
                #[cfg(windows)]
                program_x86: previous_program_x86.clone(),
            };
            std::env::remove_var("SPANREED_DESKTOP");
            std::env::set_var("PATH", dir);
            #[cfg(windows)]
            {
                std::env::set_var("LOCALAPPDATA", dir);
                std::env::set_var("ProgramFiles", dir);
                std::env::set_var("ProgramFiles(x86)", dir);
            }
            let error =
                request("overview").expect_err("no desktop binary is installed for this test");
            assert!(error.contains("Spanreed Desktop") || error.contains("spanreed-desktop"));
            let queued = std::fs::read_to_string(dir.join("spanreed").join(ROUTE)).unwrap();
            assert_eq!(queued, "#/local/overview");
            assert_eq!(peek().as_deref(), Some("#/local/overview"));
            assert_eq!(take().as_deref(), Some("#/local/overview"));
            assert!(peek().is_none());
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
        with_data_home(|dir| {
            let mut desktop = park_desktop_process(dir);
            write_lock(desktop.id());
            request("overview").expect("running desktop accepts overview");
            assert_eq!(take().as_deref(), Some("#/local/overview"));
            request("settings").expect("running desktop accepts settings");
            assert_eq!(take().as_deref(), Some("#/local/settings"));
            assert!(take().is_none());
            let _ = desktop.kill();
            let _ = desktop.wait();
        });
    }

    #[test]
    fn the_cli_binary_is_not_a_desktop() {
        let exe = std::env::current_exe().expect("current exe");
        let cli = exe
            .parent()
            .and_then(|dir| dir.parent())
            .expect("target dir")
            .join(crate::app::bin_name());
        if cli.is_file() {
            assert!(existing_desktop(&cli).is_none());
        }
        let name = exe.file_name().expect("exe name");
        assert!(
            is_cli_binary(&exe) || !name.eq_ignore_ascii_case(crate::app::bin_name()),
            "the test harness must not be treated as a packaged desktop"
        );
    }

    #[test]
    fn a_live_cli_process_is_not_the_desktop() {
        assert!(command_is_desktop(desktop_binary_names()[0]));
        assert!(command_is_desktop(&format!(
            "/nix/store/x/bin/.{}-wrapped",
            desktop_binary_names()[0]
        )));
        assert!(command_is_desktop(&format!(
            "/usr/bin/{}\0",
            desktop_binary_names()[1]
        )));
        assert!(!command_is_desktop("/usr/bin/spanreed\0"));
        assert!(!command_is_desktop("bash"));
        assert!(!command_is_desktop("spanreed.exe"));
        assert_eq!(command_is_desktop("Spanreed.exe"), cfg!(windows));
        assert!(!command_confirms_desktop(None));
        assert!(command_confirms_desktop(Some(desktop_binary_names()[0])));
    }

    #[test]
    fn route_script_only_accepts_local_pages() {
        assert_eq!(
            route_location_script("#/local/overview").as_deref(),
            Some(
                "location.hash = \"#/local/overview\"; dispatchEvent(new HashChangeEvent(\"hashchange\"))"
            )
        );
        assert_eq!(
            route_location_script("#/local/settings").as_deref(),
            Some(
                "location.hash = \"#/local/settings\"; dispatchEvent(new HashChangeEvent(\"hashchange\"))"
            )
        );
        assert!(route_location_script("#/local/overview'; alert(1)").is_none());
    }

    #[test]
    fn desktop_candidates_include_the_packaged_names() {
        let names = desktop_binary_names();
        assert!(names
            .iter()
            .any(|name| name.starts_with("spanreed-desktop")));
        assert!(names.iter().any(|name| name.starts_with("Spanreed")));
        assert_eq!(
            spanreed_install_dir("/tmp/local").join("Spanreed.exe"),
            PathBuf::from("/tmp/local/Spanreed/Spanreed.exe")
        );
    }

    #[test]
    fn running_desktop_accepts_an_alert() {
        with_data_home(|dir| {
            assert!(!hand_off_alert("Capture proxy is DOWN", "Ensure capture"));
            let mut desktop = park_desktop_process(dir);
            write_lock(desktop.id());
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
            let _ = desktop.kill();
            let _ = desktop.wait();
        });
    }

    fn write_lock(pid: u32) {
        let dir = crate::app::data_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LOCK), format!("{pid}\n")).unwrap();
    }

    fn park_desktop_process(dir: &std::path::Path) -> std::process::Child {
        let bin = dir.join("park");
        std::fs::create_dir_all(&bin).unwrap();
        #[cfg(unix)]
        {
            let stub = bin.join("spanreed-desktop");
            copy_named_stub(&stub, "sleep");
            Command::new(&stub).arg("30").spawn().expect("park desktop")
        }
        #[cfg(windows)]
        {
            let stub = bin.join("Spanreed.exe");
            copy_named_stub(&stub, "ping.exe");
            Command::new(&stub)
                .args(["-n", "31", "127.0.0.1"])
                .spawn()
                .expect("park desktop")
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = bin;
            panic!("no desktop process to park");
        }
    }

    fn copy_named_stub(destination: &std::path::Path, name: &str) {
        let from_path = std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path).find_map(|dir| {
                let candidate = dir.join(name);
                candidate.is_file().then_some(candidate)
            })
        });
        #[cfg(unix)]
        let fallbacks = [format!("/bin/{name}"), format!("/usr/bin/{name}")];
        #[cfg(windows)]
        let fallbacks = [format!(r"C:\Windows\System32\{name}")];
        #[cfg(not(any(unix, windows)))]
        let fallbacks: [String; 0] = [];
        let source = fallbacks
            .into_iter()
            .map(std::path::PathBuf::from)
            .find(|path| path.is_file())
            .or(from_path)
            .unwrap_or_else(|| panic!("{name}"));
        std::fs::copy(&source, destination).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(destination).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(destination, permissions).unwrap();
        }
    }

    #[cfg(unix)]
    fn copy_true_stub(destination: &std::path::Path) {
        let from_path = std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path).find_map(|dir| {
                let candidate = dir.join("true");
                candidate.is_file().then_some(candidate)
            })
        });
        let source = ["/bin/true", "/usr/bin/true"]
            .into_iter()
            .map(std::path::PathBuf::from)
            .find(|path| path.is_file())
            .or(from_path)
            .expect("true");
        std::fs::copy(source, destination).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn request_starts_the_desktop_named_by_the_environment() {
        with_data_home(|dir| {
            std::fs::create_dir_all(dir).unwrap();
            let stub = dir.join("spanreed-desktop");
            copy_true_stub(&stub);
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
            copy_true_stub(&stub);
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
