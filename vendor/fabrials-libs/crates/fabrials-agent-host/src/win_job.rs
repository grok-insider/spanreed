#![cfg(windows)]
#![allow(unsafe_code, missing_docs)]

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Threading::*;

#[derive(Debug)]
pub struct KillOnDropJob {
    job: OwnedHandle,
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

impl KillOnDropJob {
    fn create() -> io::Result<Self> {
        let job = owned(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })?;
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { job })
    }

    fn assign(&self, process: HANDLE) -> io::Result<()> {
        if unsafe { AssignProcessToJobObject(self.job.as_raw_handle(), process) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn for_pid(pid: u32) -> io::Result<Self> {
        let job = Self::create()?;
        let process = owned(unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) })?;
        job.assign(process.as_raw_handle())?;
        Ok(job)
    }

    pub(crate) fn spawn(command: &mut Command) -> io::Result<(Child, Self)> {
        Self::spawn_checked(
            command,
            |job, process| job.assign(process),
            resume_initial_thread,
        )
    }

    fn spawn_checked(
        command: &mut Command,
        assign: impl FnOnce(&Self, HANDLE) -> io::Result<()>,
        resume: impl FnOnce(HANDLE) -> io::Result<()>,
    ) -> io::Result<(Child, Self)> {
        let job = Self::create()?;
        command
            .creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW)
            .kill_on_drop(true);
        let child = command.spawn()?;
        let process = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("suspended process handle unavailable"))?;
        if let Err(error) = assign(&job, process).and_then(|()| resume(process)) {
            unsafe {
                TerminateProcess(process, 1);
                WaitForSingleObject(process, 5000);
            }
            return Err(error);
        }
        Ok((child, job))
    }
}

fn resume_initial_thread(process: HANDLE) -> io::Result<()> {
    let pid = unsafe { GetProcessId(process) };
    if pid == 0 {
        return Err(io::Error::last_os_error());
    }
    let snapshot = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    if unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut thread_id = None;
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::other("suspended thread lookup timed out"));
        }
        if entry.th32OwnerProcessID == pid {
            if thread_id.replace(entry.th32ThreadID).is_some() {
                return Err(io::Error::other("suspended process has multiple threads"));
            }
        }
        if unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } == 0 {
            if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
                return Err(io::Error::last_os_error());
            }
            break;
        }
    }
    let thread_id = thread_id.ok_or_else(|| io::Error::other("suspended process has no thread"))?;
    let thread = owned(unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
            0,
            thread_id,
        )
    })?;
    if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != pid {
        return Err(io::Error::other("suspended thread identity changed"));
    }
    if unsafe { ResumeThread(thread.as_raw_handle()) } != 1 {
        return Err(io::Error::other(
            "suspended thread could not be resumed exactly once",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::time::timeout;

    fn marker_command(path: &std::path::Path) -> Command {
        let mut command = Command::new("cmd");
        command
            .args(["/D", "/C"])
            .raw_arg(format!("echo synthetic>\"{}\"", path.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    fn process_is_dead(pid: u32) -> bool {
        let process = match owned(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) }) {
            Ok(process) => process,
            Err(error) => return error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32),
        };
        unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) == WAIT_OBJECT_0 }
    }

    #[tokio::test]
    async fn initial_thread_cannot_run_before_job_assignment() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("started.txt");
        let (mut child, job) = KillOnDropJob::spawn_checked(
            &mut marker_command(&marker),
            |job, process| {
                std::thread::sleep(Duration::from_millis(250));
                assert!(!marker.exists());
                job.assign(process)
            },
            resume_initial_thread,
        )
        .unwrap();
        assert!(
            timeout(Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        assert!(marker.exists());
        drop(job);
    }

    #[tokio::test]
    async fn assignment_and_resume_preflight_failure_kill_before_any_child_code() {
        let root = tempfile::tempdir().unwrap();
        for fail_assignment in [true, false] {
            let marker = root.path().join(format!("unsafe-{fail_assignment}.txt"));
            let mut pid = 0;
            let result = KillOnDropJob::spawn_checked(
                &mut marker_command(&marker),
                |job, process| {
                    pid = unsafe { GetProcessId(process) };
                    if fail_assignment {
                        Err(io::Error::other("synthetic assignment rejection"))
                    } else {
                        job.assign(process)
                    }
                },
                |_| Err(io::Error::other("synthetic thread lookup rejection")),
            );
            assert!(result.is_err());
            assert!(pid > 0 && process_is_dead(pid));
            assert!(!marker.exists());
        }
    }

    #[tokio::test]
    async fn dropping_job_terminates_the_live_child_and_its_descendant() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("descendant.pid");
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(format!("$p=Start-Process -FilePath powershell.exe -ArgumentList '-NoProfile -NonInteractive -Command Start-Sleep -Seconds 60' -PassThru -WindowStyle Hidden; [IO.File]::WriteAllText('{}',$p.Id.ToString()); Start-Sleep -Seconds 60",marker.display().to_string().replace("'","''")))
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let (mut child, job) = KillOnDropJob::spawn(&mut command).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !marker.exists() {
            assert!(Instant::now() < deadline, "descendant did not start");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let descendant: u32 = std::fs::read_to_string(marker).unwrap().parse().unwrap();
        let descendant_handle =
            owned(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, descendant) }).unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(descendant_handle.as_raw_handle(), 0) },
            WAIT_TIMEOUT
        );
        drop(job);
        assert_eq!(
            unsafe { WaitForSingleObject(descendant_handle.as_raw_handle(), 5000) },
            WAIT_OBJECT_0
        );
        assert!(
            timeout(Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .is_ok()
        );
    }
}
