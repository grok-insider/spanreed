use std::fs::{self, OpenOptions};
#[cfg(not(windows))]
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::process::{Command, Stdio};
use std::time::Duration;
#[cfg(not(windows))]
use std::time::Instant;

use fabrials_agent_host::instance::{InstanceError, InstanceLock};

pub(super) fn claim_host(directory: &Path) -> Result<InstanceLock, String> {
    InstanceLock::acquire(directory).map_err(|error| match error {
        InstanceError::AlreadyRunning => "agent or Remote host is active; finish your sessions and explicitly stop the host before retrying the update. No session was interrupted".into(),
        other => format!("cannot safely exclude agent host startup during update: {other}"),
    })
}

pub(super) struct Candidate {
    destination: PathBuf,
    directory: PathBuf,
    staged: PathBuf,
    previous: PathBuf,
    preserve: bool,
}

impl Candidate {
    pub(super) fn prepare(destination: &Path, bytes: &[u8], version: &str) -> Result<Self, String> {
        let candidate = Self::stage(destination, bytes)?;
        version_probe(&candidate.staged, version, Duration::from_secs(5))?;
        Ok(candidate)
    }

    fn stage(destination: &Path, bytes: &[u8]) -> Result<Self, String> {
        validate_destination(destination)?;
        let parent = destination.parent().ok_or("install path has no parent")?;
        let directory = parent.join(format!(
            ".spanreed-update-{}",
            fabrials_fabric::accounting::new_request_id()
        ));
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            builder
        };
        #[cfg(not(unix))]
        let builder = fs::DirBuilder::new();
        builder
            .create(&directory)
            .map_err(|error| format!("create private update directory: {error}"))?;
        let candidate = Self {
            destination: destination.into(),
            staged: directory.join(if cfg!(windows) {
                "candidate.exe"
            } else {
                "candidate"
            }),
            previous: directory.join(if cfg!(windows) {
                "previous.exe"
            } else {
                "previous"
            }),
            directory,
            preserve: false,
        };
        fabrials_agent_host::instance::ensure_private_directory(&candidate.directory)
            .map_err(|error| format!("secure update directory: {error}"))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o700);
        }
        let mut file = options
            .open(&candidate.staged)
            .map_err(|error| format!("create update candidate: {error}"))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("persist update candidate: {error}"))?;
        #[cfg(windows)]
        fabrials_agent_host::win_acl::set_owner_only(&candidate.staged)
            .map_err(|error| format!("secure update candidate: {error}"))?;
        sync_directory(&candidate.directory)?;
        Ok(candidate)
    }

    pub(super) fn install(self, version: &str) -> Result<(), String> {
        super::durable::install(&self.destination, &self.staged, version)
    }

    #[cfg(test)]
    fn install_with(
        mut self,
        mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
        healthcheck: impl FnOnce(&Path) -> Result<(), String>,
        move_previous: bool,
    ) -> Result<(), String> {
        validate_destination(&self.destination)?;
        let existed = self
            .destination
            .try_exists()
            .map_err(|error| format!("inspect previous binary: {error}"))?;
        if existed {
            if move_previous {
                rename(&self.destination, &self.previous)
                    .map_err(|error| format!("retain previous binary: {error}"))?;
            } else {
                fs::hard_link(&self.destination, &self.previous)
                    .map_err(|error| format!("retain previous binary: {error}"))?;
            }
            self.preserve = true;
            if let Err(error) = sync_directory(&self.directory) {
                return Err(self.recover(error, existed, move_previous, &mut rename));
            }
        }
        self.preserve = true;
        if let Err(error) = rename(&self.staged, &self.destination) {
            return Err(self.recover(
                format!("install new binary: {error}"),
                existed,
                move_previous,
                &mut rename,
            ));
        }
        let checked = sync_directory(self.destination.parent().expect("validated install parent"))
            .and_then(|()| healthcheck(&self.destination));
        if let Err(error) = checked {
            return Err(self.recover(
                format!("installed candidate failed validation: {error}"),
                existed,
                move_previous,
                &mut rename,
            ));
        }
        self.preserve = false;
        Ok(())
    }

    #[cfg(test)]
    fn recover(
        &mut self,
        failure: String,
        existed: bool,
        move_previous: bool,
        rename: &mut impl FnMut(&Path, &Path) -> std::io::Result<()>,
    ) -> String {
        let restored = (|| -> Result<(), String> {
            if move_previous || !existed {
                match fs::remove_file(&self.destination) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(format!("remove rejected candidate: {error}")),
                }
            }
            if existed {
                rename(&self.previous, &self.destination)
                    .map_err(|error| format!("restore previous binary: {error}"))?;
            }
            sync_directory(self.destination.parent().expect("validated install parent"))
        })();
        match restored {
            Ok(()) => {
                self.preserve = false;
                format!(
                    "{failure}; {}",
                    if existed {
                        "previous binary restored"
                    } else {
                        "rejected candidate removed"
                    }
                )
            }
            Err(error) => format!(
                "{failure}; rollback failed: {error}; recovery files retained at {}",
                self.directory.display()
            ),
        }
    }
}

impl Drop for Candidate {
    fn drop(&mut self) {
        if !self.preserve {
            let _ = fs::remove_file(&self.staged);
            let _ = fs::remove_file(&self.previous);
            let _ = fs::remove_dir(&self.directory);
        }
    }
}

pub(super) fn validate_destination(destination: &Path) -> Result<(), String> {
    let parent = destination.parent().ok_or("install path has no parent")?;
    for (path, directory) in [(parent, true), (destination, false)] {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if !directory && error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("inspect install path {}: {error}", path.display())),
        };
        if metadata.file_type().is_symlink()
            || (directory && !metadata.is_dir())
            || (!directory && !metadata.is_file())
        {
            return Err(format!(
                "install path must be a regular owned {}: {}",
                if directory { "directory" } else { "file" },
                path.display()
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
                return Err(format!(
                    "install path is not exclusively writable by its owner: {}",
                    path.display()
                ));
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(format!("reparse install path refused: {}", path.display()));
            }
            super::windows::validate(path, directory)?;
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("persist install directory: {error}"))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(windows)]
pub(super) fn version_probe(path: &Path, version: &str, timeout: Duration) -> Result<(), String> {
    super::windows::version_probe(path, version, timeout)
}

#[cfg(not(windows))]
pub(super) fn version_probe(path: &Path, version: &str, timeout: Duration) -> Result<(), String> {
    let mut command = Command::new(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command
        .spawn()
        .map_err(|error| format!("start candidate version check: {error}"))?;
    let mut guard = VersionProcess(child);
    let stdout = guard
        .0
        .stdout
        .take()
        .ok_or("candidate stdout unavailable")?;
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout.take(4097).read_to_end(&mut output).map(|_| output);
        let _ = send.send(result);
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match guard.0.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            result => {
                return Err(match result {
                    Err(error) => format!("candidate version check failed: {error}"),
                    _ => "candidate version check timed out".into(),
                });
            }
        }
    };
    if !status.success() {
        return Err("candidate version check exited unsuccessfully".into());
    }
    let output = receive
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "candidate version output timed out")?
        .map_err(|error| format!("read candidate version: {error}"))?;
    if output.len() > 4096
        || String::from_utf8_lossy(&output).trim() != format!("spanreed {version}")
    {
        return Err("candidate version differs from the selected release".into());
    }
    Ok(())
}

#[cfg(not(windows))]
struct VersionProcess(std::process::Child);

#[cfg(not(windows))]
impl Drop for VersionProcess {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "spanreed-update-test-{}",
                fabrials_fabric::accounting::new_request_id()
            ));
            fabrials_agent_host::instance::ensure_private_directory(&path).unwrap();
            Self(path)
        }

        fn destination(&self) -> PathBuf {
            self.0.join("spanreed")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn active_host_is_rejected_without_releasing_its_lock() {
        let fixture = Fixture::new();
        let directory = fixture.0.join("host");
        let host = InstanceLock::acquire(&directory).unwrap();
        assert!(
            claim_host(&directory)
                .unwrap_err()
                .contains("No session was interrupted")
        );
        assert!(claim_host(&directory).is_err());
        drop(host);
        let update = claim_host(&directory).unwrap();
        assert!(InstanceLock::acquire(&directory).is_err());
        drop(update);
        assert!(InstanceLock::acquire(&directory).is_ok());
    }

    #[test]
    fn failed_install_restores_previous_for_both_replacement_strategies() {
        for moved in [false, true] {
            let fixture = Fixture::new();
            let destination = fixture.destination();
            fs::write(&destination, b"previous").unwrap();
            let candidate = Candidate::stage(&destination, b"replacement").unwrap();
            let staged = candidate.staged.clone();
            let error = candidate
                .install_with(
                    |from, to| {
                        if from == staged {
                            Err(std::io::Error::other("injected install failure"))
                        } else {
                            fs::rename(from, to)
                        }
                    },
                    |_| panic!("failed rename must never run healthcheck"),
                    moved,
                )
                .unwrap_err();
            assert!(error.contains("previous binary restored"));
            assert_eq!(fs::read(&destination).unwrap(), b"previous");
            assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
        }
    }

    #[test]
    fn failed_installed_healthcheck_restores_previous_for_both_strategies() {
        for moved in [false, true] {
            let fixture = Fixture::new();
            let destination = fixture.destination();
            fs::write(&destination, b"previous").unwrap();
            let candidate = Candidate::stage(&destination, b"replacement").unwrap();
            let error = candidate
                .install_with(
                    |from, to| fs::rename(from, to),
                    |path| {
                        assert_eq!(fs::read(path).unwrap(), b"replacement");
                        Err("injected bad installed version".into())
                    },
                    moved,
                )
                .unwrap_err();
            assert!(error.contains("previous binary restored"));
            assert_eq!(fs::read(&destination).unwrap(), b"previous");
            assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
        }
    }

    #[test]
    fn rollback_failure_preserves_previous_and_reports_recovery_path() {
        let fixture = Fixture::new();
        let destination = fixture.destination();
        fs::write(&destination, b"previous").unwrap();
        let candidate = Candidate::stage(&destination, b"replacement").unwrap();
        let previous = candidate.previous.clone();
        let error = candidate
            .install_with(
                |from, to| {
                    if from == previous {
                        Err(std::io::Error::other("injected rollback failure"))
                    } else {
                        fs::rename(from, to)
                    }
                },
                |_| Err("injected unhealthy candidate".into()),
                true,
            )
            .unwrap_err();
        assert!(error.contains("rollback failed"));
        assert_eq!(fs::read(&previous).unwrap(), b"previous");
        assert!(error.contains(previous.parent().unwrap().to_str().unwrap()));
    }

    #[test]
    fn successful_install_checks_installed_file_and_cleans_backup() {
        let fixture = Fixture::new();
        let destination = fixture.destination();
        fs::write(&destination, b"previous").unwrap();
        Candidate::stage(&destination, b"replacement")
            .unwrap()
            .install_with(
                |from, to| fs::rename(from, to),
                |path| {
                    assert_eq!(path, destination);
                    assert_eq!(fs::read(path).unwrap(), b"replacement");
                    Ok(())
                },
                cfg!(windows),
            )
            .unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"replacement");
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    fn failed_first_install_removes_unhealthy_binary() {
        let fixture = Fixture::new();
        let destination = fixture.destination();
        let error = Candidate::stage(&destination, b"replacement")
            .unwrap()
            .install_with(
                |from, to| fs::rename(from, to),
                |_| Err("injected invalid first install".into()),
                cfg!(windows),
            )
            .unwrap_err();
        assert!(error.contains("rejected candidate removed"));
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_shared_write_destinations_are_refused() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let fixture = Fixture::new();
        let destination = fixture.destination();
        let original = fixture.0.join("other");
        fs::write(&original, b"private original").unwrap();
        symlink(&original, &destination).unwrap();
        assert!(Candidate::stage(&destination, b"replacement").is_err());
        assert_eq!(fs::read(&original).unwrap(), b"private original");
        fs::remove_file(&destination).unwrap();
        fs::write(&destination, b"previous").unwrap();
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(Candidate::stage(&destination, b"replacement").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn destination_is_rechecked_after_staging() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let destination = fixture.destination();
        let other = fixture.0.join("other");
        fs::write(&other, b"unrelated").unwrap();
        let candidate = Candidate::stage(&destination, b"replacement").unwrap();
        symlink(&other, &destination).unwrap();
        assert!(
            candidate
                .install_with(
                    |from, to| fs::rename(from, to),
                    |_| panic!("unsafe target reached healthcheck"),
                    false
                )
                .is_err()
        );
        assert_eq!(fs::read(&other).unwrap(), b"unrelated");
    }

    #[cfg(unix)]
    #[test]
    fn real_candidate_version_is_checked_before_replacement_and_after_install() {
        let fixture = Fixture::new();
        let destination = fixture.destination();
        fs::write(&destination, b"previous").unwrap();
        assert!(
            Candidate::prepare(
                &destination,
                b"#!/bin/sh\nprintf 'spanreed 1.2.3\\n'\n",
                "1.2.4"
            )
            .is_err()
        );
        assert_eq!(fs::read(&destination).unwrap(), b"previous");
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
        Candidate::prepare(
            &destination,
            b"#!/bin/sh\nprintf 'spanreed 1.2.4\\n'\n",
            "1.2.4",
        )
        .unwrap()
        .install("1.2.4")
        .unwrap();
        version_probe(&destination, "1.2.4", Duration::from_secs(1)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn hung_candidate_and_descendant_held_output_have_deadlines() {
        let fixture = Fixture::new();
        for script in [
            b"#!/bin/sh\nwhile :; do :; done\n".as_slice(),
            b"#!/bin/sh\n(while :; do :; done) &\nprintf 'spanreed 1.2.3\\n'\n",
        ] {
            let candidate = Candidate::stage(&fixture.destination(), script).unwrap();
            let start = Instant::now();
            let error =
                version_probe(&candidate.staged, "1.2.3", Duration::from_millis(100)).unwrap_err();
            assert!(error.contains("timed out"));
            assert!(start.elapsed() < Duration::from_secs(2));
        }
    }
}
