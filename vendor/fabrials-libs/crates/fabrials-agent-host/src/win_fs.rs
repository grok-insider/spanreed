#![cfg(windows)]
#![allow(unsafe_code)]

use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, GetFileInformationByHandle,
};

pub(crate) fn identity(
    handle: &impl AsRawHandle,
) -> std::io::Result<crate::workspace::FilesystemIdentity> {
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    let result = unsafe { GetFileInformationByHandle(handle.as_raw_handle(), info.as_mut_ptr()) };
    if result == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let info = unsafe { info.assume_init() };
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(std::io::Error::other(
            "reparse points are not authorized directories",
        ));
    }
    Ok(crate::workspace::FilesystemIdentity {
        device: u64::from(info.dwVolumeSerialNumber),
        inode: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}

pub(crate) fn final_path(handle: &impl AsRawHandle) -> std::io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
    let length =
        unsafe { GetFinalPathNameByHandleW(handle.as_raw_handle(), std::ptr::null_mut(), 0, 0) };
    if length == 0 {
        return Err(std::io::Error::last_os_error());
    }
    if length > 32_768 {
        return Err(std::io::Error::other(
            "directory path exceeds supported length",
        ));
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let written = unsafe {
        GetFinalPathNameByHandleW(
            handle.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    };
    if written == 0 {
        return Err(std::io::Error::last_os_error());
    }
    if written as usize >= buffer.len() {
        return Err(std::io::Error::other(
            "directory changed while resolving handle",
        ));
    }
    buffer.truncate(written as usize);
    Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &buffer,
    )))
}

pub(crate) fn confined_path(
    root: &std::path::Path,
    handle: &impl AsRawHandle,
) -> std::io::Result<std::path::PathBuf> {
    let path = final_path(handle)?;
    if !path.starts_with(root) {
        return Err(std::io::Error::other(
            "directory handle escaped authorized root",
        ));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handle_paths_reject_an_outside_directory_and_prefix_collision() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        let sibling = directory.path().join("root-elsewhere");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&sibling).unwrap();
        let root = cap_std::fs::Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let sibling =
            cap_std::fs::Dir::open_ambient_dir(&sibling, cap_std::ambient_authority()).unwrap();
        let root_path = final_path(&root).unwrap();
        assert_eq!(confined_path(&root_path, &root).unwrap(), root_path);
        assert!(confined_path(&root_path, &sibling).is_err());
    }
}
