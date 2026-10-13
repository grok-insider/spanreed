#![cfg(windows)]
#![allow(unsafe_code, missing_docs)]

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LocalFree};
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;

struct Descriptor(PSECURITY_DESCRIPTOR);

impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn open_state(path: &Path, access: u32) -> io::Result<File> {
    let file = OpenOptions::new()
        .access_mode(access)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    verify_plain_handle(&file)?;
    Ok(file)
}

fn verify_plain_handle(file: &File) -> io::Result<()> {
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { info.assume_init() }.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other("state paths cannot be reparse points"));
    }
    Ok(())
}

#[allow(missing_docs)]
pub fn set_owner_only(path: &Path) -> io::Result<()> {
    let file = open_state(path, READ_CONTROL | WRITE_DAC)?;
    set_file_owner_only(&file)
}

pub(crate) fn set_file_owner_only(file: &File) -> io::Result<()> {
    verify_plain_handle(file)?;
    let sid = current_user_sid_string()?;
    if !owner_matches(file, &sid)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "state path belongs to another owner",
        ));
    }
    let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let mut descriptor = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
            || descriptor.is_null()
        {
            return Err(io::Error::last_os_error());
        }
        let descriptor = Descriptor(descriptor);
        let mut dacl = ptr::null_mut();
        let mut defaulted = 0;
        let mut present = 0;
        if GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) == 0
            || dacl.is_null()
            || present == 0
        {
            return Err(io::Error::last_os_error());
        }
        let code = SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null_mut(),
        );
        if code != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
    }
    Ok(())
}

fn owner_matches(file: &File, expected: &str) -> io::Result<bool> {
    unsafe {
        let mut owner = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        let code = GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        );
        if code != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        let _descriptor = Descriptor(descriptor);
        if owner.is_null() {
            return Ok(false);
        }
        Ok(sid_string(owner)? == expected)
    }
}

#[allow(missing_docs)]
pub fn is_owner_only(path: &Path) -> io::Result<bool> {
    let file = open_state(path, READ_CONTROL)?;
    let current = current_user_sid_string()?;
    unsafe {
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
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        let descriptor = Descriptor(descriptor);
        let mut control = 0;
        let mut revision = 0;
        if owner.is_null()
            || dacl.is_null()
            || descriptor.0.is_null()
            || GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) == 0
            || control & SE_DACL_PROTECTED == 0
            || sid_string(owner)? != current
        {
            return Ok(false);
        }
        let mut current_allowed = false;
        for index in 0..(*dacl).AceCount as u32 {
            let mut ace = ptr::null_mut();
            if GetAce(dacl, index, &mut ace) == 0 || ace.is_null() {
                return Err(io::Error::last_os_error());
            }
            let header = &*ace.cast::<ACE_HEADER>();
            if header.AceType != 0
                || (header.AceSize as usize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
            {
                return Ok(false);
            }
            let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            let sid = sid_string(ptr::addr_of!(allowed.SidStart) as PSID)?;
            if sid != current && sid != "S-1-5-18" {
                return Ok(false);
            }
            if u32::from(header.AceFlags) & INHERITED_ACE != 0 {
                return Ok(false);
            }
            if sid == current
                && u32::from(header.AceFlags) & INHERIT_ONLY_ACE == 0
                && allowed.Mask & FILE_ALL_ACCESS == FILE_ALL_ACCESS
            {
                current_allowed = true;
            }
        }
        Ok(current_allowed)
    }
}

fn sid_string(sid: PSID) -> io::Result<String> {
    unsafe {
        if IsValidSid(sid) == 0 {
            return Err(io::Error::other("invalid state SID"));
        }
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 || text.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        Ok(value)
    }
}

fn current_user_sid_string() -> std::io::Result<String> {
    // SAFETY: token/SID buffers are sized via GetTokenInformation; LocalFree
    // for ConvertSidToStringSid allocation; CloseHandle for the token.
    unsafe {
        use windows_sys::Win32::Foundation::{
            CloseHandle, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
        };
        use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
        use windows_sys::Win32::Security::{
            GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if token.is_null() || token == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }

        let mut needed: u32 = 0;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            CloseHandle(token);
            return Err(std::io::Error::last_os_error());
        }
        let mut buf = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            CloseHandle(token);
            return Err(std::io::Error::last_os_error());
        }
        CloseHandle(token);

        let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str: *mut u16 = ptr::null_mut();
        if ConvertSidToStringSidW(token_user.User.Sid, &mut sid_str) == 0 || sid_str.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let mut len = 0usize;
        while *sid_str.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(sid_str, len));
        LocalFree(sid_str as *mut _);
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::{is_owner_only, set_owner_only};
    use std::fs;
    use std::process::Command;

    fn checked_output(command: &mut Command) -> Vec<u8> {
        let result = command.output().unwrap();
        assert!(result.status.success(), "fixture inspection failed");
        assert!(
            !result.stdout.is_empty(),
            "fixture inspection returned no data"
        );
        result.stdout
    }

    #[test]
    fn freshly_secured_directory_and_file_are_owner_only() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("private");
        fs::create_dir(&directory).unwrap();
        let file = directory.join("secret.json");
        fs::write(&file, b"{}").unwrap();
        for path in [&directory, &file] {
            set_owner_only(path).unwrap();
            assert!(is_owner_only(path).unwrap());
            set_owner_only(path).unwrap();
            assert!(is_owner_only(path).unwrap());
        }
    }

    #[test]
    fn every_foreign_principal_and_inherited_acl_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("private");
        fs::create_dir(&directory).unwrap();
        for sid in ["S-1-1-0", "S-1-5-11", "S-1-5-32-545", "S-1-5-32-544"] {
            set_owner_only(&directory).unwrap();
            let status = Command::new("icacls")
                .arg(&directory)
                .args(["/grant", &format!("*{sid}:(OI)(CI)(R)")])
                .output()
                .unwrap();
            assert!(status.status.success(), "icacls grant failed");
            assert!(
                !is_owner_only(&directory).unwrap(),
                "foreign grant accepted: {sid}"
            );
        }
        set_owner_only(&directory).unwrap();
        assert!(
            Command::new("icacls")
                .arg(&directory)
                .arg("/inheritance:e")
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(!is_owner_only(&directory).unwrap());
    }

    #[test]
    fn securing_a_junction_is_refused_without_changing_its_target() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let link = root.path().join("link");
        fs::create_dir(&target).unwrap();
        let before = checked_output(Command::new("icacls").arg(&target));
        assert!(
            Command::new("cmd")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(&link)
                .arg(&target)
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(set_owner_only(&link).is_err());
        assert!(is_owner_only(&link).is_err());
        let after = checked_output(Command::new("icacls").arg(&target));
        assert_eq!(before, after);
        fs::remove_dir(&link).unwrap();
    }
    #[test]
    #[ignore = "requires a foreign-owned directory with full control granted to the test user"]
    fn foreign_owner_is_refused_without_mutating_acl_or_owner() {
        let directory = std::path::PathBuf::from(
            std::env::var_os("FABRIALS_WINDOWS_FOREIGN_OWNER_FIXTURE")
                .expect("foreign owner fixture"),
        );
        assert!(directory.is_dir());
        let acl = || checked_output(Command::new("icacls").arg(&directory));
        let owner = || {
            checked_output(
                Command::new("powershell.exe")
                    .args(["-NoProfile", "-NonInteractive", "-Command"])
                    .arg(format!(
                        "(Get-Acl -LiteralPath '{}').Owner",
                        directory.display().to_string().replace("'", "''")
                    )),
            )
        };
        let before = (acl(), owner());
        assert!(!is_owner_only(&directory).unwrap());
        assert_eq!(
            set_owner_only(&directory).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(crate::instance::ensure_private_directory(&directory).is_err());
        assert_eq!(before, (acl(), owner()));
    }
}
