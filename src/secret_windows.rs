use std::ffi::c_void;
use std::ptr;

// wincred.h CREDENTIALW: repr(C) preserves pointer alignment on both Windows ABIs.
#[repr(C)]
struct Credential {
    flags: u32,
    kind: u32,
    target: *mut u16,
    comment: *mut u16,
    last_written: [u32; 2],
    blob_size: u32,
    blob: *mut u8,
    persist: u32,
    attribute_count: u32,
    attributes: *mut c_void,
    target_alias: *mut u16,
    username: *mut u16,
}

#[link(name = "advapi32")]
extern "system" {
    fn CredReadW(
        target: *const u16,
        kind: u32,
        flags: u32,
        credential: *mut *mut Credential,
    ) -> i32;
    fn CredWriteW(credential: *const Credential, flags: u32) -> i32;
    fn CredDeleteW(target: *const u16, kind: u32, flags: u32) -> i32;
    fn CredFree(buffer: *mut c_void);
}

fn wide(value: &str) -> Option<Vec<u16>> {
    if value.contains('\0') {
        return None;
    }
    Some(value.encode_utf16().chain(Some(0)).collect())
}

fn target(service: &str) -> String {
    format!("spanreed:{service}")
}

fn target_user(service: &str, username: &str) -> String {
    format!("spanreed:{service}:{username}")
}

struct OwnedCredential(*mut Credential);
impl Drop for OwnedCredential {
    fn drop(&mut self) {
        // CredReadW transfers a single allocation that must be released by CredFree.
        unsafe { CredFree(self.0.cast()) };
    }
}

fn cred_read(target: &str) -> Option<String> {
    let target = wide(target)?;
    let mut raw = ptr::null_mut();
    // target is terminated and raw is an initialized output pointer.
    if unsafe { CredReadW(target.as_ptr(), 1, 0, &mut raw) } == 0 || raw.is_null() {
        return None;
    }
    let owned = OwnedCredential(raw);
    let credential = unsafe { &*owned.0 };
    let size = credential.blob_size as usize;
    if size == 0 || size > 2560 || size % 2 != 0 || credential.blob.is_null() {
        return None;
    }
    // Copy bytes instead of assuming the blob pointer is aligned for u16.
    let bytes = unsafe { std::slice::from_raw_parts(credential.blob, size) };
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

fn cred_write(target: &str, secret: &str) -> bool {
    let Some(mut target) = wide(target) else {
        return false;
    };
    // Keep the UTF-16LE representation used by the previous cmdkey writer.
    let mut blob: Vec<u8> = secret.encode_utf16().flat_map(u16::to_le_bytes).collect();
    if blob.is_empty() || blob.len() > 2560 {
        return false;
    }
    let mut username = wide("spanreed").unwrap();
    let credential = Credential {
        flags: 0,
        kind: 1,
        target: target.as_mut_ptr(),
        comment: ptr::null_mut(),
        last_written: [0; 2],
        blob_size: blob.len() as u32,
        blob: blob.as_mut_ptr(),
        persist: 2,
        attribute_count: 0,
        attributes: ptr::null_mut(),
        target_alias: ptr::null_mut(),
        username: username.as_mut_ptr(),
    };
    // All borrowed buffers outlive the synchronous API call; the secret never enters argv.
    unsafe { CredWriteW(&credential, 0) != 0 }
}

fn cred_delete(target: &str) -> bool {
    let Some(target) = wide(target) else {
        return false;
    };
    unsafe { CredDeleteW(target.as_ptr(), 1, 0) != 0 }
}

pub fn lookup(service: &str) -> Option<String> {
    cred_read(&target(service))
}
pub fn lookup_user(service: &str, username: &str) -> Option<String> {
    cred_read(&target_user(service, username))
}
pub fn store(service: &str, _label: &str, secret: &str) -> bool {
    cred_write(&target(service), secret)
}
pub fn store_user(service: &str, username: &str, _label: &str, secret: &str) -> bool {
    cred_write(&target_user(service, username), secret)
}
pub fn exists(service: &str) -> bool {
    lookup(service).is_some()
}
pub fn delete(service: &str) -> bool {
    cred_delete(&target(service))
}
pub fn delete_user(service: &str, username: &str) -> bool {
    cred_delete(&target_user(service, username))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Writes a unique fixture into the current Windows user's Credential Manager"]
    fn credential_roundtrip() {
        let service = format!(
            "qa-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                delete(&self.0);
            }
        }
        let _cleanup = Cleanup(service.clone());
        assert!(lookup(&service).is_none());
        // Exercise migration from the old cmdkey writer using a non-secret fixture.
        use std::os::windows::process::CommandExt;
        let legacy = "legacy-fixture-key";
        let output = std::process::Command::new("cmdkey")
            .creation_flags(0x0800_0000)
            .args([
                format!("/generic:{}", target(&service)),
                "/user:spanreed".to_string(),
                format!("/pass:{legacy}"),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "cmdkey fixture failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(lookup(&service).as_deref(), Some(legacy));
        assert!(store(&service, "QA", "fixture ' quoted 🦀 é\n"));
        assert_eq!(lookup(&service).as_deref(), Some("fixture ' quoted 🦀 é\n"));
        assert!(exists(&service));
        assert!(store(&service, "QA", "replacement"));
        assert_eq!(lookup(&service).as_deref(), Some("replacement"));
        assert!(delete(&service));
        assert!(lookup(&service).is_none());
    }
}
