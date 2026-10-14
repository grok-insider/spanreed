#![allow(missing_docs)]

#[cfg(windows)]
mod tests {
    use fabrials_agent_host::acp::{AgentCommand, AgentHandle, AgentMode};
    use std::process::Command;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    struct Descendant {
        pid: u32,
        started: u64,
    }
    impl Descendant {
        fn new(pid: u32) -> Self {
            let value = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command"])
                .arg(format!(
                    "[Diagnostics.Process]::GetProcessById({pid}).StartTime.ToUniversalTime().Ticks"
                ))
                .output()
                .unwrap();
            assert!(value.status.success());
            Self {
                pid,
                started: String::from_utf8(value.stdout)
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap(),
            }
        }
        fn exited(&self, timeout_ms: u32) -> bool {
            Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(format!("try {{ $p=[Diagnostics.Process]::GetProcessById({}); $null=$p.Handle; if ($p.StartTime.ToUniversalTime().Ticks -ne {}) {{ exit 0 }}; if ($p.WaitForExit({timeout_ms})) {{ exit 0 }} else {{ exit 1 }} }} catch [ArgumentException] {{ exit 0 }}", self.pid, self.started))
            .status().unwrap().success()
        }
    }
    impl Drop for Descendant {
        fn drop(&mut self) {
            let _ = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(format!("try {{ $p=[Diagnostics.Process]::GetProcessById({}); $null=$p.Handle; if ($p.StartTime.ToUniversalTime().Ticks -eq {}) {{ $p.Kill(); $p.WaitForExit(5000) | Out-Null }} }} catch [ArgumentException] {{}}", self.pid, self.started))
            .output();
        }
    }

    #[tokio::test]
    async fn shutdown_ends_isolated_job_with_a_retained_handle_and_preserves_shared_descendant() {
        for mode in [AgentMode::Isolated, AgentMode::Shared] {
            let root = tempfile::tempdir().unwrap();
            let marker = root.path().join("descendant.pid");
            let mut program = std::env::current_exe().unwrap();
            program.pop();
            program.pop();
            program.push("examples");
            program.push("fake_agent.exe");
            let command = AgentCommand::new(program.to_string_lossy())
                .with_mode(mode)
                .with_env([(
                    "FAKE_AGENT_LIFETIME_PID".into(),
                    marker.to_string_lossy().into_owned(),
                )]);
            let (agent, _events) = AgentHandle::spawn(&command).unwrap();
            let retained = Arc::clone(&agent);
            agent.initialize().await.unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            while !marker.exists() {
                assert!(Instant::now() < until);
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let descendant =
                Descendant::new(std::fs::read_to_string(marker).unwrap().parse().unwrap());
            assert!(!descendant.exited(0));
            agent.shutdown().await.unwrap();
            assert!(Arc::strong_count(&retained) >= 2);
            assert_eq!(descendant.exited(5000), mode == AgentMode::Isolated);
            agent.shutdown().await.unwrap();
            drop(retained);
        }
    }
}
