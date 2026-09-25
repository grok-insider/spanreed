//! Local OS hostname, used only as this owner's installation label.
//! Never add it to a share snapshot, usage ingest, metrics publication, or log line.

pub fn profile_label() -> Option<String> {
    sanitize_hostname(&read_os_hostname()?)
}

pub(crate) fn sanitize_hostname(raw: &str) -> Option<String> {
    let value = raw.trim().trim_matches('\0').trim().trim_end_matches('.');
    if value.is_empty() || value.len() > 63 || !value.split('.').all(hostname_label) {
        return None;
    }
    Some(value.to_owned())
}

fn hostname_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}

#[cfg(unix)]
fn read_os_hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is writable and its length is passed through. On success
    // the kernel writes a NUL-terminated hostname that fits.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|byte| *byte == 0).unwrap_or(buf.len());
    std::str::from_utf8(&buf[..end]).ok().map(str::to_owned)
}

#[cfg(windows)]
fn read_os_hostname() -> Option<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetComputerNameExW(format: u32, buffer: *mut u16, size: *mut u32) -> i32;
    }
    const COMPUTER_NAME_DNS_HOSTNAME: u32 = 1;
    let mut size = 0u32;
    // SAFETY: both calls pass a buffer whose length is the size argument. The
    // first call only reports the required length. The second writes into `buffer`.
    unsafe {
        let _ = GetComputerNameExW(COMPUTER_NAME_DNS_HOSTNAME, std::ptr::null_mut(), &mut size);
        if size == 0 || size > 256 {
            return None;
        }
        let mut buffer = vec![0u16; size as usize];
        if GetComputerNameExW(COMPUTER_NAME_DNS_HOSTNAME, buffer.as_mut_ptr(), &mut size) == 0 {
            return None;
        }
        String::from_utf16(&buffer[..size as usize]).ok()
    }
}

#[cfg(not(any(unix, windows)))]
fn read_os_hostname() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_a_machine_label_and_drops_anything_else() {
        assert_eq!(
            sanitize_hostname("  spanreed-desk.\0"),
            Some("spanreed-desk".into())
        );
        assert_eq!(sanitize_hostname("studio.mini"), Some("studio.mini".into()));
        for rejected in [
            "",
            ".",
            "-desk",
            "desk-",
            "has space",
            "line\nbreak",
            "user@host",
            "a/b",
            &"h".repeat(64),
        ] {
            assert_eq!(sanitize_hostname(rejected), None);
        }
    }

    #[test]
    fn local_hostname_is_a_bounded_label() {
        let Some(label) = profile_label() else {
            return;
        };
        assert!(label.len() <= 63);
        assert!(!label.chars().any(|character| character.is_control()));
        assert!(sanitize_hostname(&label).is_some());
    }
}
