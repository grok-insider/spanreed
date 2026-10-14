use crate::acp::{AcpError, AgentCommand, AgentMode, CLIENT_IDENTIFIER, CLIENT_NAME_ENV};
use std::io;
use std::process::Stdio;
use tokio::process::{Child, Command};

#[derive(Debug)]
pub(crate) struct AgentProcess {
    pub(crate) child: Child,
    pub(crate) scoped: bool,
    stderr: tokio::task::JoinHandle<()>,
    #[cfg(unix)]
    group: Option<rustix::process::Pid>,
    #[cfg(windows)]
    _job: Option<crate::win_job::KillOnDropJob>,
}

impl AgentProcess {
    pub(crate) fn spawn(command: &AgentCommand) -> Result<Self, AcpError> {
        let scoped = needs_scope(command.mode).map_err(AcpError::Spawn)?;
        let mut builder = if scoped {
            let mut builder = Command::new("systemd-run");
            builder
                .args([
                    "--user",
                    "--scope",
                    "--quiet",
                    "--collect",
                    "--no-ask-password",
                    "--",
                ])
                .arg(&command.program);
            builder
        } else {
            Command::new(&command.program)
        };
        builder
            .args(command.argv())
            .envs(command.env.iter().map(|(key, value)| (key, value)))
            .env(CLIENT_NAME_ENV, CLIENT_IDENTIFIER)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(directory) = &command.working_directory {
            builder.current_dir(directory);
        }
        #[cfg(unix)]
        builder.process_group(0);
        #[cfg(not(windows))]
        let mut child = builder
            .spawn()
            .map_err(|error| scope_error(scoped, AcpError::Spawn(error)))?;
        #[cfg(windows)]
        let (mut child, job) = if command.mode == AgentMode::Isolated {
            let (child, job) =
                crate::win_job::KillOnDropJob::spawn(&mut builder).map_err(AcpError::Spawn)?;
            (child, Some(job))
        } else {
            (builder.spawn().map_err(AcpError::Spawn)?, None)
        };
        #[cfg(unix)]
        let group = if command.mode == AgentMode::Isolated {
            let raw = child
                .id()
                .and_then(|id| i32::try_from(id).ok())
                .filter(|id| *id > 1)
                .ok_or_else(|| AcpError::Spawn(io::Error::other("invalid agent process group")))?;
            let pid = rustix::process::Pid::from_raw(raw).expect("positive process id");
            if pid == rustix::process::getpgrp() {
                return Err(AcpError::Spawn(io::Error::other(
                    "agent process group is not isolated",
                )));
            }
            Some(pid)
        } else {
            None
        };
        let mut stderr = child.stderr.take().ok_or(AcpError::Pipe)?;
        let stderr = tokio::spawn(async move {
            let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
        });
        Ok(Self {
            child,
            scoped,
            stderr,
            #[cfg(unix)]
            group,
            #[cfg(windows)]
            _job: job,
        })
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), AcpError> {
        self.kill_group().map_err(AcpError::Transport)?;
        #[cfg(windows)]
        drop(self._job.take());
        self.child.start_kill().map_err(AcpError::Transport)?;
        let result = self.child.wait().await.map_err(AcpError::Transport);
        self.stderr.abort();
        result.map(|_| ())
    }

    fn kill_group(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(group) = self.group.take() {
            match rustix::process::kill_process_group(group, rustix::process::Signal::KILL) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(error) => {
                    self.group = Some(group);
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        let _ = self.kill_group();
        self.stderr.abort();
    }
}

pub(crate) fn scope_error(scoped: bool, error: AcpError) -> AcpError {
    if scoped {
        AcpError::SharedScope(Box::new(error))
    } else {
        error
    }
}

fn needs_scope(mode: AgentMode) -> io::Result<bool> {
    #[cfg(target_os = "linux")]
    if mode == AgentMode::Shared {
        return service_cgroup(
            &std::fs::read_to_string("/proc/self/cgroup").map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("cannot determine shared agent service ownership: {error}"),
                )
            })?,
        );
    }
    let _ = mode;
    Ok(false)
}

#[cfg(target_os = "linux")]
fn service_cgroup(cgroups: &str) -> io::Result<bool> {
    for line in cgroups.lines() {
        let mut parts = line.splitn(3, ':');
        let hierarchy = parts.next();
        let controllers = parts.next();
        let path = parts.next();
        if hierarchy == Some("0") && controllers == Some("")
            || controllers
                .is_some_and(|value| value.split(',').any(|value| value == "name=systemd"))
        {
            let path = path.filter(|path| path.starts_with('/')).ok_or_else(|| {
                io::Error::other("cannot determine the shared agent's owning service")
            })?;
            let components: Vec<_> = path.split('/').filter(|part| !part.is_empty()).collect();
            for (index, unit) in components.iter().enumerate() {
                if !unit.ends_with(".service") {
                    continue;
                }
                let manager = unit
                    .strip_prefix("user@")
                    .and_then(|unit| unit.strip_suffix(".service"))
                    .filter(|uid| !uid.is_empty() && uid.bytes().all(|byte| byte.is_ascii_digit()))
                    .is_some_and(|uid| {
                        index >= 2
                            && components[index - 2] == "user.slice"
                            && components[index - 1] == format!("user-{uid}.slice")
                    });
                if !manager
                    || !components[index + 1..]
                        .iter()
                        .any(|unit| unit.ends_with(".scope"))
                {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::service_cgroup;

    #[test]
    fn service_detection_does_not_mistake_delegated_names_for_scope_ownership() {
        for path in [
            "/system.slice/spanreed.service",
            "/system.slice/spanreed.service/delegated.scope/worker",
            "/user.slice/user-1000.slice/user@1000.service",
            "/user.slice/user-1000.slice/user@1000.service/app.slice/spanreed.service/delegated.scope",
            "/user.slice/user-1000.slice/user@1000.service/app.slice/spanreed.service",
            "/user.slice/user-1000.slice/user@1000.service/app.slice/spanreed.service/delegated/worker",
        ] {
            assert!(service_cgroup(&format!("0::{path}\n")).unwrap());
            assert!(service_cgroup(&format!("3:name=systemd:{path}\n")).unwrap());
        }
        for path in [
            "/",
            "/docker/1234",
            "/user.slice/user-1000.slice/session-2.scope",
            "/user.slice/user-1000.slice/user@1000.service/app.slice/editor.scope",
            "/user.slice/user-1000.slice/user@1000.service/app.slice/editor.scope/delegated",
        ] {
            assert!(!service_cgroup(&format!("0::{path}\n")).unwrap());
        }
        assert!(!service_cgroup("2:cpu:/example.service\n").unwrap());
        assert!(service_cgroup("2:name=systemd:/\n0::/system.slice/spanreed.service\n").unwrap());
        assert!(service_cgroup("0::relative.service").is_err());
        assert!(service_cgroup("2:name=systemd").is_err());
    }
}
