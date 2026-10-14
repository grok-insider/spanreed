#![allow(missing_docs)]

#[cfg(target_os = "linux")]
mod tests {
    use fabrials_agent_host::acp::{AgentCommand, AgentHandle, AgentMode, AgentSession};
    use serde_json::{Value, json};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    fn fixture(root: &Path, mode: AgentMode) -> AgentCommand {
        let program = root.join("fake-agent");
        std::fs::write(&program, include_bytes!("fixtures/lifetime_agent.py")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        AgentCommand::new(program.to_string_lossy())
            .with_mode(mode)
            .with_working_directory(root)
            .with_env([
                (
                    "FABRIALS_LIFETIME_ROOT".into(),
                    root.to_string_lossy().into_owned(),
                ),
                (
                    "FABRIALS_LIFETIME_TOKEN".into(),
                    "synthetic-environment-preserved".into(),
                ),
            ])
    }

    fn running(pid: u64) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|value| {
                value
                    .rsplit_once(") ")
                    .map(|(_, fields)| !fields.starts_with('Z'))
            })
            .unwrap_or(false)
    }

    fn cleanup_processes(root: &Path) {
        for file in [
            "worker.json",
            "leader-worker.json",
            "leader.json",
            "client.json",
        ] {
            if let Ok(bytes) = std::fs::read(root.join(file))
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                && let Some(pid) = value["pid"]
                    .as_i64()
                    .and_then(|raw| i32::try_from(raw).ok())
                    .filter(|raw| *raw > 1)
                    .and_then(rustix::process::Pid::from_raw)
                && let Ok(stat) =
                    std::fs::read_to_string(format!("/proc/{}/stat", pid.as_raw_nonzero()))
                && stat
                    .rsplit_once(") ")
                    .and_then(|(_, fields)| fields.split_whitespace().nth(19))
                    == value["startTime"].as_str()
            {
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            }
        }
    }

    struct ProcessCleanup(PathBuf);
    impl Drop for ProcessCleanup {
        fn drop(&mut self) {
            cleanup_processes(&self.0);
        }
    }

    async fn stopped(pid: u64) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while running(pid) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("synthetic process {pid} survived shutdown"));
    }

    fn check_transport(result: &Value, root: &Path) {
        assert_eq!(result["cwd"], root.to_string_lossy().as_ref());
        assert_eq!(result["environment"], "synthetic-environment-preserved");
        assert_eq!(result["pid"], result["pgid"]);
    }

    #[tokio::test]
    async fn isolated_shutdown_and_drop_kill_group_with_inherited_pipes_and_large_stderr() {
        for api in ["handle", "session"] {
            for (explicit, early_exit) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                let root = tempfile::tempdir().unwrap();
                let _cleanup = ProcessCleanup(root.path().to_owned());
                let command = fixture(root.path(), AgentMode::Isolated).with_env([(
                    "FABRIALS_LIFETIME_EARLY_EXIT".into(),
                    if early_exit { "1" } else { "0" }.into(),
                )]);
                let result = tokio::time::timeout(Duration::from_secs(5), async {
                    if api == "handle" {
                        let (agent, _events) = AgentHandle::spawn(&command).unwrap();
                        agent.initialize().await.unwrap();
                        let value = agent.request("lifetime", json!({})).await.unwrap();
                        if early_exit {
                            stopped(value["pid"].as_u64().unwrap()).await;
                        }
                        if explicit {
                            agent.shutdown().await.unwrap();
                        }
                        drop(agent);
                        value
                    } else {
                        let mut agent = AgentSession::spawn(&command).unwrap();
                        agent.initialize().await.unwrap();
                        let value = agent.request("lifetime", json!({})).await.unwrap();
                        if early_exit {
                            stopped(value["pid"].as_u64().unwrap()).await;
                        }
                        if explicit {
                            agent.shutdown().await.unwrap();
                        } else {
                            drop(agent);
                        }
                        value
                    }
                })
                .await
                .expect("stderr must drain without blocking ACP");
                check_transport(&result, root.path());
                stopped(result["pid"].as_u64().unwrap()).await;
                stopped(result["leader"]["worker"].as_u64().unwrap()).await;
            }
        }
    }

    fn write_json(path: &Path, value: &Value) {
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec(value).unwrap()).unwrap();
        std::fs::rename(temporary, path).unwrap();
    }

    #[tokio::test]
    #[ignore = "internal helper started in a synthetic systemd user service"]
    async fn service_child_fixture() {
        let Some(configuration) = std::env::var_os("FABRIALS_LIFETIME_CONFIGURATION") else {
            return;
        };
        let config: Value = serde_json::from_slice(&std::fs::read(configuration).unwrap()).unwrap();
        let root = Path::new(config["root"].as_str().unwrap());
        let mut command = fixture(root, AgentMode::Shared);
        let fail = config["fail"].as_bool().unwrap();
        if fail {
            command = command.with_env([
                (
                    "DBUS_SESSION_BUS_ADDRESS".into(),
                    format!("unix:path={}/missing-bus", root.display()),
                ),
                (
                    "XDG_RUNTIME_DIR".into(),
                    root.join("missing-runtime").to_string_lossy().into_owned(),
                ),
            ]);
        }
        if config["api"] == "handle" {
            let (agent, _events) = AgentHandle::spawn(&command).unwrap();
            let initialized = agent.initialize().await;
            if fail {
                write_json(
                    &root.join("report.json"),
                    &json!({"error": initialized.unwrap_err().to_string(), "hostCgroup": std::fs::read_to_string("/proc/self/cgroup").unwrap()}),
                );
                agent.shutdown().await.unwrap();
                return;
            }
            initialized.unwrap();
            let result = agent.request("lifetime", json!({})).await.unwrap();
            write_json(&root.join("report.json"), &result);
            while !root.join("shutdown").exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            agent.shutdown().await.unwrap();
        } else {
            let mut agent = AgentSession::spawn(&command).unwrap();
            let initialized = agent.initialize().await;
            if fail {
                write_json(
                    &root.join("report.json"),
                    &json!({"error": initialized.unwrap_err().to_string(), "hostCgroup": std::fs::read_to_string("/proc/self/cgroup").unwrap()}),
                );
                agent.shutdown().await.unwrap();
                return;
            }
            initialized.unwrap();
            let result = agent.request("lifetime", json!({})).await.unwrap();
            write_json(&root.join("report.json"), &result);
            while !root.join("shutdown").exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            agent.shutdown().await.unwrap();
        }
        write_json(&root.join("shutdown.json"), &json!({"shutdown":true}));
    }

    struct ServiceFixture {
        unit: String,
        root: tempfile::TempDir,
    }

    impl ServiceFixture {
        fn new(api: &str, fail: bool) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = tempfile::Builder::new()
                .prefix("spanreed-$literal space-")
                .tempdir()
                .unwrap();
            let unit = format!(
                "spanreed-lifetime-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            write_json(
                &root.path().join("configuration.json"),
                &json!({"root":root.path(),"api":api,"fail":fail}),
            );
            Self { unit, root }
        }
        fn start(&self) {
            let status = Command::new("systemd-run")
                .args([
                    "--user",
                    "--quiet",
                    "--collect",
                    "--no-ask-password",
                    "--property=KillMode=control-group",
                    "--property=TimeoutStopSec=3",
                ])
                .arg(format!("--unit={}.service", self.unit))
                .arg(format!(
                    "--setenv=FABRIALS_LIFETIME_CONFIGURATION={}",
                    self.root.path().join("configuration.json").display()
                ))
                .arg("--")
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::service_child_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .status()
                .unwrap();
            assert!(status.success());
        }
        fn stop(&self) {
            let status = Command::new("systemctl")
                .args(["--user", "stop", &format!("{}.service", self.unit)])
                .status()
                .unwrap();
            assert!(status.success());
        }
        async fn report(&self, name: &str) -> Value {
            let path = self.root.path().join(name);
            let until = Instant::now() + Duration::from_secs(10);
            while !path.exists() {
                assert!(
                    Instant::now() < until,
                    "missing synthetic receipt: {}",
                    path.display()
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
        }
    }

    impl Drop for ServiceFixture {
        fn drop(&mut self) {
            let _ = Command::new("systemctl")
                .args(["--user", "stop", &format!("{}.service", self.unit)])
                .output();
            cleanup_processes(self.root.path());
        }
    }

    #[tokio::test]
    async fn shutdown_closes_pending_requests_when_descendants_hold_stdio_open() {
        let root = tempfile::tempdir().unwrap();
        let _cleanup = ProcessCleanup(root.path().to_owned());
        let (agent, _events) =
            AgentHandle::spawn(&fixture(root.path(), AgentMode::Isolated)).unwrap();
        agent.initialize().await.unwrap();
        let result = agent.request("lifetime", json!({})).await.unwrap();
        let pending = std::sync::Arc::clone(&agent);
        let request = tokio::spawn(async move { pending.request("never", json!({})).await });
        tokio::time::timeout(Duration::from_secs(3), async {
            while !root.path().join("pending.json").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        agent.shutdown().await.unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), request)
                .await
                .unwrap()
                .unwrap(),
            Err(fabrials_agent_host::acp::AcpError::Closed)
        ));
        stopped(result["pid"].as_u64().unwrap()).await;
        stopped(result["leader"]["worker"].as_u64().unwrap()).await;
    }

    #[tokio::test]
    #[ignore = "requires an available systemd user manager; uses disposable units only"]
    async fn shared_survives_service_stop_restart_and_rejects_missing_user_manager() {
        for api in ["handle", "session"] {
            let fixture = ServiceFixture::new(api, false);
            fixture.start();
            let first = fixture.report("report.json").await;
            check_transport(&first, fixture.root.path());
            let leader = &first["leader"];
            let leader_pid = leader["pid"].as_u64().unwrap();
            let client_pid = first["pid"].as_u64().unwrap();
            let worker_pid = leader["worker"].as_u64().unwrap();
            assert!(first["cgroup"].as_str().unwrap().contains(".scope"));
            assert!(!first["cgroup"].as_str().unwrap().contains(&fixture.unit));
            assert_eq!(first["cgroup"], leader["cgroup"]);
            fixture.stop();
            stopped(client_pid).await;
            assert!(running(leader_pid));
            assert!(running(worker_pid));
            std::fs::remove_file(fixture.root.path().join("report.json")).unwrap();
            fixture.start();
            let second = fixture.report("report.json").await;
            assert_eq!(second["leader"]["pid"], leader_pid);
            assert_ne!(second["pid"], client_pid);
            std::fs::write(fixture.root.path().join("shutdown"), b"shutdown").unwrap();
            fixture.report("shutdown.json").await;
            stopped(second["pid"].as_u64().unwrap()).await;
            assert!(running(leader_pid));
            assert!(running(worker_pid));
            drop(fixture);
            stopped(leader_pid).await;
            stopped(worker_pid).await;
            let fixture = ServiceFixture::new(api, true);
            fixture.start();
            let failed = fixture.report("report.json").await;
            assert!(
                failed["error"]
                    .as_str()
                    .unwrap()
                    .contains("independent systemd user scope")
            );
            assert!(!fixture.root.path().join("client.json").exists());
            assert!(!fixture.root.path().join("leader.json").exists());
        }
    }
}
