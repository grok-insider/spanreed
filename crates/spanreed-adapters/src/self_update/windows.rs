use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::mem::{size_of, size_of_val, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::Threading::*;

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn failure(label: &str) -> String {
    format!("{label}: {}", io::Error::last_os_error())
}

fn owned(handle: HANDLE) -> Result<OwnedHandle, String> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(failure("Windows handle"))
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

pub(super) fn validate(path: &Path, directory: bool) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .share_mode(FILE_SHARE_READ)
        .open(path)
        .map_err(|error| format!("open protected install path {}: {error}", path.display()))?;
    unsafe {
        let mut information: BY_HANDLE_FILE_INFORMATION = zeroed();
        if GetFileInformationByHandle(file.as_raw_handle(), &mut information) == 0 {
            return Err(failure("inspect install handle"));
        }
        if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        {
            return Err("install path has an unexpected type or is a reparse point".into());
        }
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(failure("read updater identity"));
        }
        let token = owned(token)?;
        let mut needed = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut needed,
        );
        let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err(failure("read updater SID"));
        }
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut owner = ptr::null_mut();
        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        let code = GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        );
        if code != ERROR_SUCCESS {
            return Err(format!(
                "read install ACL: {}",
                io::Error::from_raw_os_error(code as i32)
            ));
        }
        let result = (|| {
            if owner.is_null() || EqualSid(owner, user.User.Sid) == 0 || dacl.is_null() {
                return Err(
                    "install path must have the updater's owner and an explicit DACL".into(),
                );
            }
            let mut system = [0usize; 16];
            let mut system_len = size_of_val(&system) as u32;
            if CreateWellKnownSid(
                WinLocalSystemSid,
                ptr::null_mut(),
                system.as_mut_ptr().cast(),
                &mut system_len,
            ) == 0
            {
                return Err(failure("read SYSTEM SID"));
            }
            let write = FILE_WRITE_DATA
                | FILE_APPEND_DATA
                | FILE_WRITE_EA
                | FILE_WRITE_ATTRIBUTES
                | DELETE
                | WRITE_DAC
                | WRITE_OWNER
                | GENERIC_WRITE
                | GENERIC_ALL
                | if directory { FILE_DELETE_CHILD } else { 0 };
            for index in 0..(*dacl).AceCount as u32 {
                let mut ace = ptr::null_mut();
                if GetAce(dacl, index, &mut ace) == 0 || ace.is_null() {
                    return Err(failure("read install ACE"));
                }
                let header = &*ace.cast::<ACE_HEADER>();
                if u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0 || header.AceType == 1 {
                    continue;
                }
                if header.AceType != 0
                    || (header.AceSize as usize) < size_of::<ACCESS_ALLOWED_ACE>()
                {
                    return Err(
                        "unsupported install ACE; use an owner-only installation directory".into(),
                    );
                }
                let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                let sid = ptr::addr_of!(allowed.SidStart) as PSID;
                if IsValidSid(sid) == 0 {
                    return Err("invalid install SID".into());
                }
                if allowed.Mask & write != 0
                    && EqualSid(sid, user.User.Sid) == 0
                    && EqualSid(sid, system.as_mut_ptr().cast()) == 0
                {
                    return Err(
                        "install path permits another principal to modify or replace it".into(),
                    );
                }
            }
            Ok(())
        })();
        LocalFree(descriptor);
        result?;
    }
    Ok(file)
}

pub(super) fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = wide(source.as_os_str());
    let destination = wide(destination.as_os_str());
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

struct Confined {
    process: OwnedHandle,
    job: OwnedHandle,
}

impl Drop for Confined {
    fn drop(&mut self) {
        unsafe {
            TerminateJobObject(self.job.as_raw_handle(), 1);
            WaitForSingleObject(self.process.as_raw_handle(), 5000);
        }
    }
}

struct Attributes(Vec<usize>);

impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast());
        }
    }
}

pub(super) fn version_probe(path: &Path, version: &str, timeout: Duration) -> Result<(), String> {
    let _file = validate(path, false)?;
    let (child, reader) = unsafe { spawn_probe(path)? };
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let result = reader.take(4097).read_to_end(&mut output).map(|_| output);
        let _ = send.send(result);
    });
    let deadline = Instant::now() + timeout;
    let waited = unsafe {
        WaitForSingleObject(
            child.process.as_raw_handle(),
            timeout.as_millis().min(u32::MAX as u128) as u32,
        )
    };
    if waited != WAIT_OBJECT_0 {
        return Err("candidate version check timed out or could not be observed".into());
    }
    let mut exit = 1;
    if unsafe { GetExitCodeProcess(child.process.as_raw_handle(), &mut exit) } == 0 || exit != 0 {
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

unsafe fn spawn_probe(path: &Path) -> Result<(Confined, File), String> {
    unsafe {
        let job = owned(CreateJobObjectW(ptr::null(), ptr::null()))?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of_val(&limits) as u32,
        ) == 0
        {
            return Err(failure("configure candidate Job"));
        }
        let mut security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: 1,
        };
        let mut read = ptr::null_mut();
        let mut write = ptr::null_mut();
        if CreatePipe(&mut read, &mut write, &mut security, 0) == 0 {
            return Err(failure("create candidate output pipe"));
        }
        let reader = owned(read)?;
        let writer = owned(write)?;
        if SetHandleInformation(reader.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(failure("protect candidate output handle"));
        }
        let null = owned(CreateFileW(
            wide(std::ffi::OsStr::new("NUL")).as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &security,
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        ))?;
        let handles = [writer.as_raw_handle(), null.as_raw_handle()];
        let mut bytes = 0;
        InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut bytes);
        let mut attributes = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        if InitializeProcThreadAttributeList(attributes.as_mut_ptr().cast(), 1, 0, &mut bytes) == 0
        {
            return Err(failure("initialize candidate handle list"));
        }
        let mut attributes = Attributes(attributes);
        if UpdateProcThreadAttribute(
            attributes.0.as_mut_ptr().cast(),
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr().cast(),
            size_of_val(&handles),
            ptr::null_mut(),
            ptr::null(),
        ) == 0
        {
            return Err(failure("restrict candidate inherited handles"));
        }
        let mut startup: STARTUPINFOEXW = zeroed();
        startup.StartupInfo.cb = size_of_val(&startup) as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = null.as_raw_handle();
        startup.StartupInfo.hStdOutput = writer.as_raw_handle();
        startup.StartupInfo.hStdError = null.as_raw_handle();
        startup.lpAttributeList = attributes.0.as_mut_ptr().cast();
        let executable = wide(path.as_os_str());
        let mut command: Vec<u16> = std::iter::once(b'"' as u16)
            .chain(path.as_os_str().encode_wide())
            .chain("\" --version".encode_utf16())
            .chain(Some(0))
            .collect();
        let mut process: PROCESS_INFORMATION = zeroed();
        if CreateProcessW(
            executable.as_ptr(),
            command.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            CREATE_SUSPENDED | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT,
            ptr::null(),
            ptr::null(),
            &startup.StartupInfo,
            &mut process,
        ) == 0
        {
            return Err(failure("create suspended candidate"));
        }
        let process_handle = owned(process.hProcess)?;
        let thread = owned(process.hThread)?;
        #[cfg(test)]
        let reject_assignment = path
            .file_stem()
            .is_some_and(|name| name == "assignment-marker");
        #[cfg(not(test))]
        let reject_assignment = false;
        if reject_assignment
            || AssignProcessToJobObject(job.as_raw_handle(), process_handle.as_raw_handle()) == 0
        {
            TerminateProcess(process_handle.as_raw_handle(), 1);
            WaitForSingleObject(process_handle.as_raw_handle(), 5000);
            return Err(failure("confine suspended candidate"));
        }
        let confined = Confined {
            process: process_handle,
            job,
        };
        if ResumeThread(thread.as_raw_handle()) == u32::MAX {
            return Err(failure("resume confined candidate"));
        }
        drop(writer);
        Ok((confined, File::from(reader)))
    }
}

pub(super) fn process_stamp(pid: u32) -> Result<u64, String> {
    let process = owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })?;
    stamp(&process)
}

fn stamp(process: &OwnedHandle) -> Result<u64, String> {
    unsafe {
        let mut created: FILETIME = zeroed();
        let mut exited: FILETIME = zeroed();
        let mut kernel: FILETIME = zeroed();
        let mut user: FILETIME = zeroed();
        if GetProcessTimes(
            process.as_raw_handle(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        ) == 0
        {
            return Err(failure("read updater process identity"));
        }
        Ok(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
    }
}

pub(super) fn wait_parent(pid: u32, created: u64) -> Result<(), String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
    if handle.is_null()
        && io::Error::last_os_error().raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32)
    {
        return Ok(());
    }
    let process = owned(handle)?;
    if stamp(&process)? != created {
        return Ok(());
    }
    if unsafe { WaitForSingleObject(process.as_raw_handle(), 60_000) } != WAIT_OBJECT_0 {
        return Err(
            "updater parent did not exit within 60 seconds; installation was not changed".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "spanreed-windows-update-{}",
                fabrials_fabric::accounting::new_request_id()
            ));
            fabrials_agent_host::instance::ensure_private_directory(&directory).unwrap();
            Self(directory)
        }

        fn file(&self, name: &str) -> std::path::PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, b"fixture").unwrap();
            fabrials_agent_host::win_acl::set_owner_only(&path).unwrap();
            path
        }

        fn grant(path: &Path, permission: &str) {
            let result = std::process::Command::new("icacls.exe")
                .arg(path)
                .args(["/grant", &format!("*S-1-5-32-545:{permission}")])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }

        fn probe(&self, scenario: &str) -> std::path::PathBuf {
            let fixture = std::env::var_os("SPANREED_UPDATE_PROBE_FIXTURE")
                .expect("native qualification requires SPANREED_UPDATE_PROBE_FIXTURE");
            let destination = self.0.join(format!("{scenario}.exe"));
            std::fs::copy(fixture, &destination).unwrap();
            fabrials_agent_host::win_acl::set_owner_only(&destination).unwrap();
            destination
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn owner_only_paths_and_other_users_read_access_are_accepted() {
        let fixture = Fixture::new();
        let path = fixture.file("safe");
        assert!(validate(&fixture.0, true).is_ok());
        assert!(validate(&path, false).is_ok());
        Fixture::grant(&path, "R");
        assert!(validate(&path, false).is_ok());
    }

    #[test]
    fn foreign_write_and_delete_permissions_are_rejected() {
        let fixture = Fixture::new();
        for permission in ["W", "D", "M", "F"] {
            let path = fixture.file(permission);
            Fixture::grant(&path, permission);
            assert!(validate(&path, false).is_err(), "{permission}");
        }
        Fixture::grant(&fixture.0, "(OI)(CI)M");
        assert!(validate(&fixture.0, true).is_err());
    }

    #[test]
    #[ignore = "run with the built update_probe_fixture on native Windows"]
    fn native_job_contains_rapid_exit_timeouts_and_descendants() {
        let fixture = Fixture::new();
        let rapid = fixture.probe("rapid");
        for _ in 0..10 {
            version_probe(&rapid, "1.2.3", Duration::from_secs(1)).unwrap();
        }
        for scenario in ["timeout", "child", "child-timeout", "excess", "wrong"] {
            let path = fixture.probe(scenario);
            let before = Instant::now();
            assert!(version_probe(&path, "1.2.3", Duration::from_millis(250)).is_err());
            assert!(before.elapsed() < Duration::from_secs(6));
        }
        let rejected = fixture.probe("assignment-marker");
        assert!(version_probe(&rejected, "1.2.3", Duration::from_secs(1)).is_err());
        std::thread::sleep(Duration::from_secs(3));
        assert!(std::fs::read_dir(&fixture.0).unwrap().all(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_none_or(|extension| extension != "escaped")
        }));
    }
}
