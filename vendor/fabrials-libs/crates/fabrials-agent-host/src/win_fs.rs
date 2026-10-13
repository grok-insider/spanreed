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
