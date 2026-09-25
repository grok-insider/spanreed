//! Opt-in local reset-expiry delivery, independent of sharing preferences.
use fabrials_types::{Availability, Freshness, Observation, ResetInventory};
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[cfg_attr(feature = "contracts", ts(rename = "ResetNotificationSettings"))]
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub reset_expiry: bool,
}

pub fn settings() -> Result<Settings, String> {
    use std::io::Read;
    let file = match std::fs::File::open(crate::app::config_dir().join("notifications.json")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default());
        }
        Err(_) => return Err("Notification settings unavailable".into()),
    };
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "Notification settings unavailable")?;
    if bytes.len() > 4096 {
        return Err("Notification settings too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid notification settings".into())
}
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let bytes = serde_json::to_vec(&Settings {
        reset_expiry: enabled,
    })
    .map_err(|_| "Invalid notification settings")?;
    fabrials_runtime::files::atomic_write_private(
        &crate::app::config_dir().join("notifications.json"),
        &bytes,
    )
    .map_err(|_| "Could not save notification settings".into())
}

fn expiring(observation: &Observation<ResetInventory>, now: i64) -> Option<(u32, i64)> {
    if observation.availability != Availability::Available
        || observation.freshness != Freshness::Fresh
    {
        return None;
    }
    let observed = observation.observed_at_ms?;
    if now.saturating_sub(observed) > 900_000 || observed.saturating_sub(now) > 60_000 {
        return None;
    }
    let inventory = observation.value.as_ref()?;
    if inventory.available == 0 {
        return None;
    }
    let expiries: Vec<i64> = inventory
        .credits
        .iter()
        .filter(|credit| {
            credit
                .valid_from_ms
                .is_none_or(|start| start.unsigned_abs() <= 8_640_000_000_000_000 && start <= now)
        })
        .filter_map(|credit| credit.expires_at_ms)
        .filter(|expiry| *expiry > now && expiry.saturating_sub(now) <= 86_400_000)
        .collect();
    Some((
        (expiries.len() as u32).min(inventory.available),
        *expiries.iter().min()?,
    ))
}

pub fn deliver_outputs(
    outputs: &[crate::model::ProviderOutput],
    mut send: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<u32, String> {
    if !settings()?.reset_expiry {
        return Ok(0);
    }
    let store = fabrials_runtime::notifications::DeliveryStore::open(
        &crate::app::data_dir().join("runtime.sqlite3"),
    )?;
    let now = crate::util::now_ms();
    let mut delivered = 0;
    for output in outputs {
        let Some((count, expiry)) = output
            .reset_inventory
            .as_ref()
            .and_then(|observation| expiring(observation, now))
        else {
            continue;
        };
        if !settings()?.reset_expiry {
            return Ok(delivered);
        }
        let key = format!("{}:{expiry}", output.provider_id);
        let Some(claim) = store.claim("local-reset-expiry", &key, now, expiry)? else {
            continue;
        };
        let name: String = output
            .display_name
            .chars()
            .filter(|c| !c.is_control())
            .take(120)
            .collect();
        send(
            "Reset credits expire soon",
            &format!(
                "{name}: {count} reset credit(s) expire within 24 hours. Open Spanreed for expiry dates."
            ),
        )?;
        store.acknowledge(&claim)?;
        delivered += 1;
    }
    Ok(delivered)
}

pub fn deliver_background(outputs: &[crate::model::ProviderOutput]) -> Result<u32, String> {
    deliver_outputs(outputs, deliver_user_visible)
}

/// `Notifier` for reset-credit expiry through OS notifications.
pub struct ResetExpiryNotifier;

impl crate::ports::Notifier for ResetExpiryNotifier {
    fn notify(&self, outputs: &[crate::model::ProviderOutput]) -> Result<u32, String> {
        deliver_background(outputs)
    }
}

/// Deliver an alert, then show a dialog when the banner command is not proof
/// the user saw it. Capture-down and other alerts share this path.
pub fn deliver_user_visible(title: &str, body: &str) -> Result<(), String> {
    let delivered = deliver_os(title, body);
    if !notification_confirmed(std::env::consts::OS, delivered.is_ok()) {
        show_unconfirmed_dialog(title, body);
    }
    delivered
}

fn show_unconfirmed_dialog(title: &str, body: &str) {
    #[cfg(windows)]
    {
        let script = windows_dialog_script(title, body);
        let _ = std::process::Command::new(powershell_program())
            .args(windows_dialog_args(&script))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new(osascript_program())
            .args(["-e", &osascript_dialog(title, body)])
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("zenity")
            .args(zenity_dialog_args(title, body))
            .spawn();
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (title, body);
    }
}

/// macOS `display notification` and a Windows toast both exit successfully when
/// the banner is dropped. Neither command is proof the user saw the alert.
#[cfg_attr(not(feature = "tray"), allow(dead_code))]
pub(crate) fn notification_confirmed(platform: &str, command_ok: bool) -> bool {
    !matches!(platform, "macos" | "windows") && command_ok
}

pub fn deliver_os(title: &str, body: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    #[cfg(target_os = "linux")]
    let output = deliver_linux(title, body);
    #[cfg(target_os = "windows")]
    let output = std::process::Command::new(powershell_program())
        .creation_flags(0x08000000)
        .args(["-NoProfile", "-NonInteractive", "-Command", WINDOWS_NOTIFY])
        .env("SPANREED_NOTIFICATION_TITLE", title)
        .env("SPANREED_NOTIFICATION_BODY", body)
        .stdin(std::process::Stdio::null())
        .output();
    #[cfg(target_os = "macos")]
    let output = std::process::Command::new(osascript_program())
        .args(["-e", &osascript_notification(title, body)])
        .stdin(std::process::Stdio::null())
        .output();
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = (title, body);
        return Err("Background notifications have not been qualified on this platform".into());
    }
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    return match output {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            let detail = detail.trim();
            if detail.is_empty() {
                Err("Could not deliver the notification through the operating system".into())
            } else {
                Err(format!(
                    "Could not deliver the notification through the operating system: {detail}"
                ))
            }
        }
        Err(error) => Err(format!(
            "Could not deliver the notification through the operating system: {error}"
        )),
    };
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn osascript_program() -> &'static str {
    "/usr/bin/osascript"
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn powershell_program() -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
    root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}

/// One AppleScript statement. Newlines would break the `-e` string.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn osascript_notification(title: &str, body: &str) -> String {
    fn escape(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace(['\n', '\r'], " ")
    }
    format!(
        "display notification \"{}\" with title \"{}\"",
        escape(body),
        escape(title)
    )
}

/// Modal fallback when the Linux notification service is missing or rejects the alert.
#[cfg_attr(not(feature = "tray"), allow(dead_code))]
pub(crate) fn zenity_dialog_args(title: &str, body: &str) -> Vec<String> {
    vec![
        "--warning".into(),
        "--no-wrap".into(),
        "--title".into(),
        title.into(),
        "--text".into(),
        body.into(),
    ]
}

/// WPF `MessageBox` throws unless the PowerShell host is STA, and that throw
/// is easy to miss because the dialog is spawned. WinForms shows on the STA
/// host Windows PowerShell already uses when `-STA` is set.
/// `DefaultDesktopOnly` is for services and can put the box on another desktop.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn windows_dialog_script(title: &str, body: &str) -> String {
    fn escape(value: &str) -> String {
        value.replace('\'', "''").replace(['\n', '\r'], " ")
    }
    format!(
        "Add-Type -AssemblyName System.Windows.Forms; \
         [System.Windows.Forms.MessageBox]::Show('{}','{}') | Out-Null",
        escape(body),
        escape(title)
    )
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn windows_dialog_args(script: &str) -> Vec<String> {
    vec![
        "-NoProfile".into(),
        "-STA".into(),
        "-Command".into(),
        script.into(),
    ]
}

/// Modal fallback when Notification Center rejects the banner.
/// A direct `display dialog` does not need Automation permission. Asking
/// System Events can leave the alert blocked on that prompt.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn osascript_dialog(title: &str, body: &str) -> String {
    fn escape(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace(['\n', '\r'], " ")
    }
    format!(
        "display dialog \"{}\" with title \"{}\" buttons {{\"OK\"}} default button \"OK\"",
        escape(body),
        escape(title)
    )
}

#[cfg(target_os = "linux")]
fn deliver_linux(title: &str, body: &str) -> std::io::Result<std::process::Output> {
    deliver_linux_with(&[], title, body)
}

/// `env` is added to the helper processes only, so callers never mutate this process.
#[cfg(target_os = "linux")]
fn deliver_linux_with(
    env: &[(&str, &std::ffi::OsStr)],
    title: &str,
    body: &str,
) -> std::io::Result<std::process::Output> {
    let sent = std::process::Command::new("notify-send")
        .envs(env.iter().copied())
        .args([
            "--app-name=Spanreed",
            "--icon=com.fabrials.spanreed",
            "--",
            title,
            body,
        ])
        .stdin(std::process::Stdio::null())
        .output();
    if sent.as_ref().is_ok_and(|output| output.status.success()) {
        return sent;
    }
    let bus = std::process::Command::new("gdbus")
        .envs(env.iter().copied())
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.Notifications",
            "--object-path",
            "/org/freedesktop/Notifications",
            "--method",
            "org.freedesktop.Notifications.Notify",
            "Spanreed",
            "0",
            "com.fabrials.spanreed",
            title,
            body,
            "[]",
            "{}",
            "5000",
        ])
        .stdin(std::process::Stdio::null())
        .output();
    match &bus {
        Ok(output) if output.status.success() => bus,
        _ if sent.is_err() => bus,
        _ => sent,
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WINDOWS_NOTIFY: &str = r#"
$ErrorActionPreference = 'Stop'
function Show-SpanreedBalloon {
  Add-Type -AssemblyName System.Windows.Forms
  Add-Type -AssemblyName System.Drawing
  $notify = New-Object System.Windows.Forms.NotifyIcon
  $notify.Icon = [System.Drawing.SystemIcons]::Information
  $notify.Visible = $true
  $notify.ShowBalloonTip(8000, $env:SPANREED_NOTIFICATION_TITLE, $env:SPANREED_NOTIFICATION_BODY, [System.Windows.Forms.ToolTipIcon]::Info)
  $deadline = (Get-Date).AddSeconds(2)
  while ((Get-Date) -lt $deadline) {
    [System.Windows.Forms.Application]::DoEvents()
    Start-Sleep -Milliseconds 50
  }
  $notify.Dispose()
}
$toastShown = $false
try {
  [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null
  [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] > $null
  $document = New-Object Windows.Data.Xml.Dom.XmlDocument
  $document.LoadXml('<toast><visual><binding template="ToastGeneric"><text/><text/></binding></visual></toast>')
  $nodes = $document.GetElementsByTagName('text')
  $nodes.Item(0).AppendChild($document.CreateTextNode($env:SPANREED_NOTIFICATION_TITLE)) > $null
  $nodes.Item(1).AppendChild($document.CreateTextNode($env:SPANREED_NOTIFICATION_BODY)) > $null
  $toast = [Windows.UI.Notifications.ToastNotification]::new($document)
  $script:toastFailed = $false
  $toast.add_Failed({ $script:toastFailed = $true })
  # PowerShell's own AppUserModelID is registered on Windows. An unregistered
  # id such as com.fabrials.spanreed accepts Show and then drops the toast.
  [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe').Show($toast)
  Start-Sleep -Milliseconds 500
  if (-not $script:toastFailed) { $toastShown = $true }
} catch {}
try {
  Show-SpanreedBalloon
} catch {}
if ($script:toastFailed -or -not $toastShown) { throw "Spanreed notification was not shown" }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires an isolated OS notification session and SPANREED_NOTIFICATION_QA_ROOT"]
    fn background_notification_survives_restart_without_duplicate_delivery() {
        let root = std::path::PathBuf::from(
            std::env::var("SPANREED_NOTIFICATION_QA_ROOT").expect("isolated QA root required"),
        );
        assert!(root.is_absolute());
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", root.join("config")) };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("XDG_DATA_HOME", root.join("data")) };
        let now = crate::util::now_ms();
        std::fs::create_dir_all(&root).unwrap();
        let restart = std::env::var_os("SPANREED_NOTIFICATION_QA_RESTART").is_some();
        let expiry = if restart {
            std::fs::read_to_string(root.join("expiry"))
                .unwrap()
                .parse()
                .unwrap()
        } else {
            let expiry = now + 3_600_000;
            std::fs::write(root.join("expiry"), expiry.to_string()).unwrap();
            expiry
        };
        let mut output = crate::model::ProviderOutput::error(
            "qa/reset-fixture",
            "Spanreed QA fixture (test)",
            "Test observation",
        );
        output.reset_inventory = Some(Observation {
            availability: Availability::Available,
            freshness: Freshness::Fresh,
            observed_at_ms: Some(now),
            source: "isolated QA".into(),
            error: None,
            value: Some(ResetInventory {
                available: 1,
                details_complete: true,
                credits: vec![fabrials_types::ResetCredit {
                    valid_from_ms: None,
                    expires_at_ms: Some(expiry),
                }],
            }),
        });
        set_enabled(true).unwrap();
        assert_eq!(
            deliver_background(std::slice::from_ref(&output)).unwrap(),
            if restart { 0 } else { 1 }
        );
        assert_eq!(deliver_background(&[output]).unwrap(), 0);
        if !restart {
            assert!(std::process::Command::new(std::env::current_exe().unwrap()).args(["notifications::tests::background_notification_survives_restart_without_duplicate_delivery","--ignored","--exact"]).env("SPANREED_NOTIFICATION_QA_RESTART","1").status().unwrap().success());
            set_enabled(false).unwrap();
        }
    }
    #[test]
    fn notification_policy_requires_recent_current_credits() {
        let mut observed = Observation {
            availability: Availability::Available,
            freshness: Freshness::Fresh,
            observed_at_ms: Some(1_000_000),
            source: "fixture".into(),
            error: None,
            value: Some(ResetInventory {
                available: 1,
                details_complete: true,
                credits: vec![fabrials_types::ResetCredit {
                    valid_from_ms: None,
                    expires_at_ms: Some(2_000_000),
                }],
            }),
        };
        assert_eq!(expiring(&observed, 1_000_000), Some((1, 2_000_000)));
        assert_eq!(expiring(&observed, 2_000_000), None);
        observed.freshness = Freshness::Stale;
        assert_eq!(expiring(&observed, 1_000_000), None);
        observed.freshness = Freshness::Fresh;
        observed.observed_at_ms = None;
        assert_eq!(expiring(&observed, 1_000_000), None);
    }

    #[test]
    fn windows_toast_uses_a_registered_app_id() {
        assert!(WINDOWS_NOTIFY.contains(
            r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe"
        ));
        assert!(WINDOWS_NOTIFY.contains("Show-SpanreedBalloon"));
        assert!(WINDOWS_NOTIFY.contains("Show($toast)"));
        assert!(WINDOWS_NOTIFY.contains("add_Failed"));
        assert!(WINDOWS_NOTIFY.contains("if (-not $script:toastFailed) { $toastShown = $true }"));
        assert!(WINDOWS_NOTIFY.contains(
            "if ($script:toastFailed -or -not $toastShown) { throw \"Spanreed notification was not shown\" }"
        ));
        assert!(WINDOWS_NOTIFY.contains("[System.Windows.Forms.Application]::DoEvents()"));
        assert!(!WINDOWS_NOTIFY.contains("CreateToastNotifier('com.fabrials.spanreed')"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_delivery_hands_the_alert_and_bus_to_notify_send() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "spanreed-notify-send-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("args.txt");
        let fake = dir.join("notify-send");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$DBUS_SESSION_BUS_ADDRESS\" \"$@\" > '{}'\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = [
            ("PATH", dir.as_os_str()),
            (
                "DBUS_SESSION_BUS_ADDRESS",
                std::ffi::OsStr::new("unix:path=/run/spanreed-test-bus"),
            ),
        ];
        let output = deliver_linux_with(&env, "Capture proxy is DOWN", "Ensure capture.").unwrap();
        assert!(output.status.success());
        let args = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = args.lines().collect();
        assert_eq!(lines[0], "unix:path=/run/spanreed-test-bus");
        assert!(lines.contains(&"--app-name=Spanreed"));
        assert_eq!(
            &lines[lines.len() - 2..],
            ["Capture proxy is DOWN", "Ensure capture."]
        );
        assert_ne!(
            std::env::var_os("DBUS_SESSION_BUS_ADDRESS").as_deref(),
            Some(std::ffi::OsStr::new("unix:path=/run/spanreed-test-bus"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn capture_alert_reaches_a_linux_notification_service() {
        use std::io::{BufRead, BufReader};
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = std::env::temp_dir().join(format!("spanreed-notify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("received.txt");
        let ready = dir.join("ready.txt");
        let stderr_log = dir.join("service.err");
        let script = dir.join("service.py");
        std::fs::write(
            &script,
            r#"import sys
from dbus.mainloop.glib import DBusGMainLoop
DBusGMainLoop(set_as_default=True)
import dbus
import dbus.service
from gi.repository import GLib
class Notifications(dbus.service.Object):
    @dbus.service.method("org.freedesktop.Notifications", in_signature="", out_signature="as")
    def GetCapabilities(self):
        return []
    @dbus.service.method("org.freedesktop.Notifications", in_signature="", out_signature="ssss")
    def GetServerInformation(self):
        return ("Spanreed", "fabrials", "1", "1.2")
    @dbus.service.method("org.freedesktop.Notifications", in_signature="susssasa{sv}i", out_signature="u")
    def Notify(self, app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout):
        with open(sys.argv[1], "w", encoding="utf-8") as handle:
            handle.write(summary + "\n" + body)
        GLib.idle_add(loop.quit)
        return dbus.UInt32(1)
loop = GLib.MainLoop()
bus = dbus.SessionBus()
name = dbus.service.BusName("org.freedesktop.Notifications", bus)
Notifications(bus, "/org/freedesktop/Notifications")
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    handle.write("ready\n")
loop.run()
"#,
        )
        .unwrap();
        let mut daemon = match std::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(daemon) => daemon,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let _ = std::fs::remove_dir_all(&dir);
                return;
            }
            Err(error) => panic!("dbus-daemon: {error}"),
        };
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().expect("address"))
            .read_line(&mut address)
            .unwrap();
        let address = address.trim().to_string();
        let python = std::path::Path::new("/usr/bin/python3");
        let python = if python.exists() {
            python
        } else {
            std::path::Path::new("python3")
        };
        let has_bindings = std::process::Command::new(python)
            .args(["-c", "import dbus, gi"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !has_bindings {
            eprintln!("skipped: python3 lacks the dbus/gi bindings for the fake service");
            let _ = daemon.kill();
            let _ = daemon.wait();
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let stderr = std::fs::File::create(&stderr_log).unwrap();
        let mut service = match std::process::Command::new(python)
            .arg(&script)
            .arg(&log)
            .arg(&ready)
            .env("DBUS_SESSION_BUS_ADDRESS", &address)
            .stderr(stderr)
            .spawn()
        {
            Ok(service) => service,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let _ = daemon.kill();
                let _ = daemon.wait();
                let _ = std::fs::remove_dir_all(&dir);
                return;
            }
            Err(error) => panic!("notification service: {error}"),
        };
        for _ in 0..50 {
            if ready.exists() || service.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let bus_env = [(
            "DBUS_SESSION_BUS_ADDRESS",
            std::ffi::OsStr::new(address.as_str()),
        )];
        let mut text = String::new();
        for _ in 0..40 {
            let _ = deliver_linux_with(
                &bus_env,
                "Capture proxy is DOWN",
                "Ensure capture before new hops.",
            );
            if let Ok(body) = std::fs::read_to_string(&log) {
                text = body;
                if text.contains("Capture proxy is DOWN") {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = service.kill();
        let _ = service.wait();
        let _ = daemon.kill();
        let _ = daemon.wait();
        let service_err = std::fs::read_to_string(&stderr_log).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            text.contains("Capture proxy is DOWN"),
            "notification service did not receive the alert: {text} service: {service_err}"
        );
        assert!(text.contains("Ensure capture before new hops."));
    }

    #[test]
    fn macos_banner_does_not_count_as_delivered() {
        assert!(!notification_confirmed("macos", true));
        assert!(!notification_confirmed("macos", false));
        assert!(notification_confirmed("linux", true));
        assert!(!notification_confirmed("windows", true));
        assert!(!notification_confirmed("windows", false));
    }

    #[test]
    fn notification_programs_do_not_depend_on_path() {
        assert_eq!(osascript_program(), "/usr/bin/osascript");
        let powershell = powershell_program();
        assert!(powershell.ends_with(r"System32\WindowsPowerShell\v1.0\powershell.exe"));
    }

    #[test]
    fn windows_dialog_stays_visible_without_wpf() {
        let script = windows_dialog_script("Capture 'down'", "Line one\nLine two");
        assert!(script.contains("System.Windows.Forms.MessageBox"));
        assert!(!script.contains("DefaultDesktopOnly"));
        assert!(!script.contains("PresentationFramework"));
        assert!(script.contains("Capture ''down''"));
        assert!(script.contains("Line one Line two"));
        assert!(!script.contains('\n'));
        let args = windows_dialog_args(&script);
        assert_eq!(
            args,
            vec![
                "-NoProfile".to_string(),
                "-STA".to_string(),
                "-Command".to_string(),
                script,
            ]
        );
    }

    #[test]
    fn macos_notification_script_stays_one_statement() {
        let script = osascript_notification("Capture \"down\"", "Line one\nLine two");
        assert!(!script.contains('\n'));
        assert!(script.contains("display notification \"Line one Line two\""));
        assert!(script.contains("with title \"Capture \\\"down\\\"\""));
        let dialog = osascript_dialog("Capture \"down\"", "Line one\nLine two");
        assert!(!dialog.contains('\n'));
        assert!(dialog.starts_with("display dialog \"Line one Line two\""));
        assert!(!dialog.contains("System Events"));
        let zenity = zenity_dialog_args("Capture down", "Ensure capture");
        assert_eq!(
            zenity,
            vec![
                "--warning".to_string(),
                "--no-wrap".to_string(),
                "--title".to_string(),
                "Capture down".to_string(),
                "--text".to_string(),
                "Ensure capture".to_string(),
            ]
        );
    }
}
