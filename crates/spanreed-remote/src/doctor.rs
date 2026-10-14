use fabrials_agent_host::control::{self, ControlRequest, ControlResponse};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

struct Inputs {
    program: PathBuf,
    search_path: OsString,
    grok_home: Option<PathBuf>,
    leader_socket: Option<PathBuf>,
}

impl Inputs {
    fn read(lookup: impl Fn(&str) -> Option<OsString>) -> Self {
        let nonempty = |key| lookup(key).filter(|value| !value.is_empty());
        Self {
            program: nonempty("FABRIALS_AGENT_PROGRAM")
                .unwrap_or_else(|| "grok".into())
                .into(),
            search_path: nonempty("PATH").unwrap_or_default(),
            grok_home: nonempty("GROK_HOME").map(PathBuf::from).or_else(|| {
                nonempty(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .map(|home| PathBuf::from(home).join(".grok"))
            }),
            leader_socket: nonempty("GROK_LEADER_SOCKET").map(PathBuf::from),
        }
    }
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn resolve_program(inputs: &Inputs) -> Option<PathBuf> {
    if inputs.program.components().count() > 1 || inputs.program.is_absolute() {
        return executable(&inputs.program).then(|| inputs.program.clone());
    }
    for directory in std::env::split_paths(&inputs.search_path) {
        let candidate = directory.join(&inputs.program);
        if executable(&candidate) {
            return Some(candidate);
        }
        #[cfg(windows)]
        if candidate.extension().is_none() {
            for extension in ["exe", "cmd", "bat"] {
                let candidate = candidate.with_extension(extension);
                if executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn check(id: &str, status: &str, code: &str, remedy: &str) -> Value {
    json!({"id":id,"status":status,"code":code,"remedy":remedy})
}

fn local_checks(inputs: &Inputs, directory: &Path, private_origin: Option<&str>) -> Vec<Value> {
    let mut checks = Vec::new();
    let program = resolve_program(inputs);
    checks.push(check("grok-program", if program.is_some() {"passed"} else {"failed"}, if program.is_some() {"executable_found"} else {"grok_not_on_calling_process_path"}, "Set FABRIALS_AGENT_PROGRAM to the installed Grok executable in the Remote service environment."));
    let home_exists = inputs
        .grok_home
        .as_ref()
        .is_some_and(|home| home.is_absolute() && home.is_dir());
    checks.push(check("grok-home", if home_exists {"passed"} else {"failed"}, if home_exists {"directory_found"} else {"grok_home_missing_or_relative"}, "Use the same absolute GROK_HOME and OS user for the terminal and the Remote service. Do not copy credentials into Remote."));
    let state = std::fs::symlink_metadata(directory);
    let state_safe = state.as_ref().is_ok_and(|metadata| {
        if !metadata.is_dir() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o077 == 0
        }
        #[cfg(not(unix))]
        {
            true
        }
    });
    checks.push(check("state-directory", if state_safe {"passed"} else {"failed"}, if state_safe {"directory_present"} else {"state_missing_or_not_private"}, "Start the configured host once to initialize its private state; this inspection does not create or repair files."));
    let identity = state_safe
        && fabrials_agent_host::state::load(directory).is_ok_and(|identity| identity.is_some());
    checks.push(check("host-identity", if identity {"passed"} else {"failed"}, if identity {"identity_present"} else {"identity_unavailable"}, "Start the configured host and inspect its startup error. Keep the existing identity to preserve browser pairing."));
    if let Some(origin) = private_origin {
        let valid = super::services::command_args(Some(origin), None).is_ok();
        checks.push(check("private-origin", if valid {"passed"} else {"failed"}, if valid {"exact_https_origin"} else {"invalid_private_origin"}, "Use an exact HTTPS origin for your Tailscale or custom domain, without a path, query or fragment."));
    } else {
        let registered = state_safe
            && fabrials_remote_host::state::load(directory).is_ok_and(|config| config.enabled);
        checks.push(check("registration", if registered {"passed"} else {"failed"}, if registered {"enabled_locally"} else {"remote_not_enabled"}, "Link the installation, then run spanreed remote enable <label>. This check does not refresh credentials."));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let mut candidates = Vec::new();
        if let Some(path) = &inputs.leader_socket {
            candidates.push(path.clone());
        } else if let Some(home) = &inputs.grok_home
            && let Ok(entries) = std::fs::read_dir(home)
        {
            for entry in entries.take(256).flatten() {
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("leader") && name.ends_with(".sock"))
                {
                    candidates.push(entry.path());
                }
            }
        }
        let found = candidates.iter().any(|path| {
            std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket())
        });
        checks.push(check("leader-socket", "unverified", if found {"socket_present_liveness_unverified"} else {"leader_socket_not_observed"}, "Use grok --leader and the same GROK_HOME/GROK_LEADER_SOCKET. A socket file alone cannot establish a shared active engine."));
    }
    #[cfg(not(unix))]
    checks.push(check(
        "leader-socket",
        "unverified",
        "platform_runtime_check_required",
        "Verify shared-leader continuity with the terminal on this platform.",
    ));
    checks.push(check("terminal-continuity", "unverified", "terminal_mode_requires_live_test", "Embedded terminal sessions are not adopted live. After their turn finishes, reopen them with grok --leader --resume <session-id>. Verify both directions."));
    checks.push(check("service-environment", "unverified", "calling_process_only", "This report describes its calling process. Check the Remote service uses the same executable, OS user, GROK_HOME and leader socket."));
    checks.push(check("browser-route", "unverified", "end_to_end_probe_required", "Open the PWA from the target device and verify HTTPS, pairing, revocation and reconnect. A local status file cannot validate this route."));
    checks
}

pub(super) async fn run(args: &[String]) -> Result<(), String> {
    let origin =
        match args {
            [] => None,
            [flag, origin] if flag == "--private-origin" => Some(origin.as_str()),
            _ => return Err(
                "usage: spanreed remote doctor --inspect [--private-origin <https://host.example>]"
                    .into(),
            ),
        };
    let directory = super::directory()?;
    let inputs = Inputs::read(|name| std::env::var_os(name));
    let mut checks = local_checks(&inputs, &directory, origin);
    let running = matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            control::call(&directory, &ControlRequest::Status)
        )
        .await,
        Ok(Ok(ControlResponse::Status { .. }))
    );
    checks.push(check("host-control", if running {"passed"} else {"failed"}, if running {"owner_control_responded"} else {"host_not_running_or_unresponsive"}, "Start the configured Remote service and check its logs. Inspection never starts or stops the host."));
    if origin.is_none() {
        let status = fabrials_remote_host::status::read(&directory)
            .ok()
            .flatten();
        let fresh = running && status.as_ref().is_some_and(|value| value.is_fresh());
        let connected = fresh
            && status.as_ref().is_some_and(|value| {
                value.connection == fabrials_remote_host::status::Connection::Connected
            });
        checks.push(check("relay-status", if connected {"passed"} else {"failed"}, if connected {"fresh_connected_status"} else {"relay_not_connected_or_status_stale"}, "Start Remote and inspect its connection error. Verify account access, DNS and the relay route separately."));
    }
    let failed = checks.iter().any(|value| value["status"] == "failed");
    println!(
        "{}",
        json!({"schema":1,"scope":"calling-process","readOnly":true,"qualified":false,"checks":checks})
    );
    if failed {
        Err("inspection_found_actionable_failures".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_environment_is_reported_without_creating_state() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("absent");
        let inputs = Inputs::read(|_| None);
        let checks = local_checks(&inputs, &state, None);
        for id in [
            "grok-program",
            "grok-home",
            "state-directory",
            "host-identity",
            "registration",
        ] {
            assert!(
                checks
                    .iter()
                    .any(|value| value["id"] == id && value["status"] == "failed")
            );
        }
        assert!(!state.exists());
        assert!(
            checks
                .iter()
                .any(|value| value["id"] == "browser-route" && value["status"] == "unverified")
        );
    }

    #[test]
    fn configured_program_and_home_override_shell_defaults() {
        let root = tempfile::tempdir().unwrap();
        let program = root.path().join("custom-grok");
        std::fs::write(&program, b"not executed").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let inputs = Inputs::read(|name| match name {
            "FABRIALS_AGENT_PROGRAM" => Some(program.clone().into_os_string()),
            "GROK_HOME" => Some(root.path().as_os_str().into()),
            _ => None,
        });
        assert_eq!(resolve_program(&inputs), Some(program));
        let checks = local_checks(
            &inputs,
            &root.path().join("absent"),
            Some("https://device.tail.ts.net"),
        );
        for id in ["grok-program", "grok-home", "private-origin"] {
            assert!(
                checks
                    .iter()
                    .any(|value| value["id"] == id && value["status"] == "passed")
            );
        }
        assert!(std::fs::read_dir(root.path()).unwrap().count() == 1);
    }

    #[test]
    fn malformed_origin_is_rejected_without_echoing_its_secret() {
        let root = tempfile::tempdir().unwrap();
        let checks = local_checks(
            &Inputs::read(|_| None),
            root.path(),
            Some("https://secret@device.tail.ts.net/?token=secret"),
        );
        assert!(
            checks
                .iter()
                .any(|value| value["id"] == "private-origin" && value["status"] == "failed")
        );
        assert!(!serde_json::to_string(&checks).unwrap().contains("secret"));
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_socket_is_never_reported_as_a_live_leader() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("custom.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let inputs = Inputs::read(|name| {
            (name == "GROK_LEADER_SOCKET").then(|| socket.clone().into_os_string())
        });
        let checks = local_checks(&inputs, root.path(), None);
        assert!(checks.iter().any(|value| value["id"] == "leader-socket"
            && value["status"] == "unverified"
            && value["code"] == "socket_present_liveness_unverified"));
    }
}
