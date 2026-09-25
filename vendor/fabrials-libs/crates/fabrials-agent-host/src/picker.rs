//! The host-owned directory picker.
//!
//! Light ADR 0009: the browser never supplies a filesystem path. It may ask
//! the host to open a picker, and the host decides everything about it — where
//! it starts, what it filters, and what comes back. The browser then refers to
//! the result by an opaque id.
//!
//! On Linux the picker is `xdg-desktop-portal`'s `FileChooser`. The portal
//! returns URIs as opaque strings with almost no validation, so this module
//! parses and constrains them itself rather than trusting the value.
//!
//! On macOS the picker is AppleScript's `choose folder`, run through
//! `osascript`. Its output is parsed by [`parse_osascript_choice`], which is
//! platform-independent so it is tested everywhere.

use std::path::PathBuf;
use std::sync::Arc;

use crate::workspace::WorkspaceRef;

/// Errors produced while picking a directory.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PickerError {
    /// The user closed the picker without choosing.
    #[error("no directory was selected")]
    Cancelled,
    /// The portal returned something that is not a local directory URI.
    #[error("selection was not a local directory")]
    NotLocal,
    /// The desktop portal is unavailable on this platform or session.
    #[error("no directory picker is available")]
    Unavailable,
    /// A picker is already open.
    #[error("a directory picker is already open")]
    AlreadyOpen,
}

/// Convert a portal URI into a local directory path.
///
/// Only a `file` URI naming this machine is accepted. A remote host, another
/// scheme, or a relative reference is refused rather than coerced, because the
/// value crosses from an external process into a path the agent will work in.
///
/// # Errors
///
/// Returns [`PickerError::NotLocal`] for anything that is not a local `file`
/// URI.
pub fn uri_to_directory(uri: &str) -> Result<PathBuf, PickerError> {
    let parsed = url::Url::parse(uri).map_err(|_| PickerError::NotLocal)?;
    if parsed.scheme() != "file" {
        return Err(PickerError::NotLocal);
    }
    // `file://host/path` names another machine. An empty host, or the literal
    // `localhost`, is this one.
    match parsed.host_str() {
        None | Some("" | "localhost") => {}
        Some(_) => return Err(PickerError::NotLocal),
    }
    // `to_file_path` handles percent-decoding and rejects a non-absolute URI.
    parsed.to_file_path().map_err(|()| PickerError::NotLocal)
}

/// Opens a directory picker owned by the host.
///
/// A trait so the interactive portal can be swapped for a deterministic
/// implementation in tests: the portal itself cannot be driven without a
/// desktop session.
#[async_trait::async_trait]
pub trait DirectoryPicker: std::fmt::Debug + Send + Sync {
    /// Ask the user to choose a directory.
    ///
    /// # Errors
    ///
    /// Returns [`PickerError::Cancelled`] only when the user closes the dialog
    /// themselves, and [`PickerError::Unavailable`] when no portal is reachable
    /// or a reachable one fails to answer. The two are kept apart so a broken
    /// portal is never reported as the user's own decision.
    async fn pick_directory(&self) -> Result<PathBuf, PickerError>;
}

/// Decide whether a failed portal request was the user's decision or a fault.
///
/// Only the portal's explicit `Cancelled` response means the user closed the
/// dialog. A transport failure, a portal-side error, or a missing reply is a
/// fault: reporting it as a cancel would hide a broken desktop portal behind
/// a message that blames nobody and prompts no repair.
#[cfg(target_os = "linux")]
fn classify_portal_error(error: &ashpd::Error) -> PickerError {
    match error {
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled) => PickerError::Cancelled,
        _ => PickerError::Unavailable,
    }
}

/// The `xdg-desktop-portal` implementation.
#[cfg(target_os = "linux")]
#[derive(Debug, Default)]
pub struct PortalDirectoryPicker;

#[cfg(target_os = "linux")]
#[async_trait::async_trait]
impl DirectoryPicker for PortalDirectoryPicker {
    async fn pick_directory(&self) -> Result<PathBuf, PickerError> {
        use ashpd::desktop::file_chooser::OpenFileRequest;

        // Every option is chosen here, not by the browser.
        let request = OpenFileRequest::default()
            .title(PICKER_PROMPT)
            .directory(true)
            .multiple(false)
            .modal(true)
            .send()
            .await
            .map_err(|_| PickerError::Unavailable)?;

        // A closed dialog and a broken portal both surface as an error here.
        // Only an explicit cancel is a cancel: reporting a fault as one would
        // turn a crashed portal, a dead session bus, or a refused request into
        // a silent no-op the user cannot tell from their own decision.
        let selected = match request.response() {
            Ok(selected) => selected,
            Err(error) => return Err(classify_portal_error(&error)),
        };
        // A success carrying no URI is not a cancel either; the portal did not
        // answer the question that was asked.
        let uri = selected.uris().first().ok_or(PickerError::Unavailable)?;
        uri_to_directory(uri.as_str())
    }
}

/// Title shown by the host-owned picker.
pub const PICKER_PROMPT: &str = "Choose a workspace for Grok Build";

/// AppleScript error number for a dialog the user dismissed.
const OSASCRIPT_USER_CANCELED: &str = "(-128)";

/// Interpret the result of `osascript -e 'POSIX path of (choose folder …)'`.
///
/// A successful run prints the chosen folder as a POSIX path with a trailing
/// `/`. A dismissed dialog exits unsuccessfully with AppleScript error
/// `-128`; any other failure means the picker itself is unusable.
///
/// # Errors
///
/// Returns [`PickerError::Cancelled`] for a dismissed dialog,
/// [`PickerError::NotLocal`] for output that is not an absolute path, and
/// [`PickerError::Unavailable`] for any other failure or empty output.
pub fn parse_osascript_choice(
    succeeded: bool,
    stdout: &str,
    stderr: &str,
) -> Result<PathBuf, PickerError> {
    if !succeeded {
        return Err(if stderr.contains(OSASCRIPT_USER_CANCELED) {
            PickerError::Cancelled
        } else {
            PickerError::Unavailable
        });
    }
    let line = stdout.trim_end_matches(['\n', '\r']);
    if line.is_empty() || line.contains('\n') {
        return Err(PickerError::Unavailable);
    }
    if !line.starts_with('/') {
        return Err(PickerError::NotLocal);
    }
    let trimmed = line.trim_end_matches('/');
    Ok(PathBuf::from(if trimmed.is_empty() {
        "/"
    } else {
        trimmed
    }))
}

/// The macOS implementation, backed by `osascript`. It compiles on every Unix
/// so Linux CI type-checks it; only macOS selects it.
#[cfg(unix)]
#[derive(Debug, Default)]
pub struct OsascriptDirectoryPicker;

#[cfg(unix)]
#[async_trait::async_trait]
impl DirectoryPicker for OsascriptDirectoryPicker {
    async fn pick_directory(&self) -> Result<PathBuf, PickerError> {
        let script = format!("POSIX path of (choose folder with prompt \"{PICKER_PROMPT}\")");
        let output = tokio::process::Command::new("/usr/bin/osascript")
            .args(["-e", &script])
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|_| PickerError::Unavailable)?;
        parse_osascript_choice(
            output.status.success(),
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        )
    }
}

/// The directory picker for the current platform: the desktop portal on
/// Linux, `osascript` on macOS, and [`UnavailableDirectoryPicker`] elsewhere.
#[must_use]
pub fn platform_picker() -> Arc<dyn DirectoryPicker> {
    #[cfg(target_os = "linux")]
    {
        Arc::new(PortalDirectoryPicker)
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(OsascriptDirectoryPicker)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Arc::new(UnavailableDirectoryPicker)
    }
}

/// A picker that is never available, for platforms without a portal.
#[derive(Debug, Default)]
pub struct UnavailableDirectoryPicker;

#[async_trait::async_trait]
impl DirectoryPicker for UnavailableDirectoryPicker {
    async fn pick_directory(&self) -> Result<PathBuf, PickerError> {
        Err(PickerError::Unavailable)
    }
}

/// The outcome of a completed pick, for the caller to enrol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickOutcome {
    /// A directory was chosen and enrolled.
    Enrolled(WorkspaceRef),
    /// The user closed the picker. Nothing changed.
    Cancelled,
    /// The pick failed for a reason worth reporting.
    Failed(PickerError),
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn the_macos_picker_is_a_directory_picker() {
        let picker: std::sync::Arc<dyn super::DirectoryPicker> =
            std::sync::Arc::new(super::OsascriptDirectoryPicker);
        drop(picker);
    }

    use super::{PickerError, parse_osascript_choice, uri_to_directory};

    #[test]
    fn an_osascript_choice_becomes_a_path_without_the_trailing_slash() {
        assert_eq!(
            parse_osascript_choice(true, "/Users/friend/my project/\n", ""),
            Ok(std::path::PathBuf::from("/Users/friend/my project"))
        );
        assert_eq!(
            parse_osascript_choice(true, "/Volumes/Data/café/\r\n", ""),
            Ok(std::path::PathBuf::from("/Volumes/Data/café"))
        );
    }

    #[test]
    fn choosing_the_root_keeps_the_root() {
        assert_eq!(
            parse_osascript_choice(true, "/\n", ""),
            Ok(std::path::PathBuf::from("/"))
        );
    }

    #[test]
    fn a_dismissed_osascript_dialog_is_a_cancel() {
        assert_eq!(
            parse_osascript_choice(false, "", "0:69: execution error: User canceled. (-128)\n"),
            Err(PickerError::Cancelled)
        );
    }

    #[test]
    fn other_osascript_failures_are_not_a_cancel() {
        assert_eq!(
            parse_osascript_choice(
                false,
                "",
                "execution error: Not authorised to send Apple events. (-1743)"
            ),
            Err(PickerError::Unavailable)
        );
        assert_eq!(
            parse_osascript_choice(true, "", ""),
            Err(PickerError::Unavailable)
        );
        assert_eq!(
            parse_osascript_choice(true, "/a/\n/b/\n", ""),
            Err(PickerError::Unavailable)
        );
    }

    #[test]
    fn a_relative_or_hfs_osascript_path_is_refused() {
        for output in ["Macintosh HD:Users:friend:\n", "relative/dir/\n"] {
            assert_eq!(
                parse_osascript_choice(true, output, ""),
                Err(PickerError::NotLocal),
                "{output:?} must be refused"
            );
        }
    }

    // Unix absolute `file:///` shapes; Windows `url::to_file_path` rejects them.
    #[cfg(unix)]
    #[test]
    fn a_plain_local_file_uri_becomes_a_path() {
        assert_eq!(
            uri_to_directory("file:///home/friend/project").expect("path"),
            std::path::PathBuf::from("/home/friend/project")
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_explicit_localhost_is_this_machine() {
        assert_eq!(
            uri_to_directory("file://localhost/srv/data").expect("path"),
            std::path::PathBuf::from("/srv/data")
        );
    }

    #[cfg(unix)]
    #[test]
    fn percent_encoding_is_decoded() {
        assert_eq!(
            uri_to_directory("file:///home/friend/my%20project").expect("path"),
            std::path::PathBuf::from("/home/friend/my project")
        );
        assert_eq!(
            uri_to_directory("file:///tmp/caf%C3%A9").expect("path"),
            std::path::PathBuf::from("/tmp/café")
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_file_uri_becomes_a_path() {
        let path = uri_to_directory("file:///C:/Users/friend/project").expect("path");
        assert!(path.is_absolute());
        assert!(path.to_string_lossy().contains("project"));
    }

    #[test]
    fn a_remote_host_is_refused() {
        // `file://host/path` names another machine; the agent must not be
        // pointed at it because a portal handed back that string.
        for uri in [
            "file://fileserver/share/project",
            "file://192.168.1.10/export",
            "file://evil.example/tmp",
        ] {
            assert_eq!(
                uri_to_directory(uri).unwrap_err(),
                PickerError::NotLocal,
                "{uri} must be refused"
            );
        }
    }

    #[test]
    fn another_scheme_is_refused() {
        for uri in [
            "http://example.com/x",
            "https://example.com/x",
            "smb://server/share",
            "sftp://host/path",
            "data:text/plain,hello",
        ] {
            assert_eq!(
                uri_to_directory(uri).unwrap_err(),
                PickerError::NotLocal,
                "{uri} must be refused"
            );
        }
    }

    #[test]
    fn a_malformed_reference_is_refused() {
        for uri in ["", "not a uri", "/plain/path", "://nope"] {
            assert_eq!(
                uri_to_directory(uri).unwrap_err(),
                PickerError::NotLocal,
                "{uri} must be refused"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_scheme_relative_file_uri_normalises_to_an_absolute_path() {
        // `url` resolves `file:relative` to `/relative` rather than rejecting
        // it. That is not an escape — it is an absolute path at the root — and
        // enrolment is the backstop: it canonicalises and requires an existing
        // directory, so a path invented this way does not become a workspace.
        let path = uri_to_directory("file:relative").expect("normalised");
        assert_eq!(path, std::path::PathBuf::from("/relative"));
        assert!(path.is_absolute());
    }

    #[cfg(unix)]
    #[test]
    fn traversal_inside_the_uri_is_resolved_before_it_is_used() {
        // Enrolment canonicalises afterwards, but the path handed on must
        // already be absolute and free of a scheme.
        let path = uri_to_directory("file:///home/friend/../friend/project").expect("path");
        assert!(path.is_absolute());
        assert!(!path.to_string_lossy().contains("file:"));
    }

    #[cfg(target_os = "linux")]
    mod portal_errors {
        use super::super::{PickerError, classify_portal_error};
        use ashpd::desktop::ResponseError;

        #[test]
        fn an_explicit_cancel_is_the_users_decision() {
            assert_eq!(
                classify_portal_error(&ashpd::Error::Response(ResponseError::Cancelled)),
                PickerError::Cancelled
            );
        }

        #[test]
        fn a_portal_side_failure_is_not_a_cancel() {
            // `Other` is the portal saying the request failed. Calling that a
            // cancel would tell the user they closed a dialog they never saw.
            assert_eq!(
                classify_portal_error(&ashpd::Error::Response(ResponseError::Other)),
                PickerError::Unavailable
            );
        }

        #[test]
        fn a_silent_portal_is_not_a_cancel() {
            assert_eq!(
                classify_portal_error(&ashpd::Error::NoResponse),
                PickerError::Unavailable
            );
        }

        #[test]
        fn a_broken_transport_is_not_a_cancel() {
            // A dead session bus is the case that matters most: the picker
            // never appeared, so there was no decision to attribute.
            assert_eq!(
                classify_portal_error(&ashpd::Error::ParseError("bus")),
                PickerError::Unavailable
            );
        }
    }
}
