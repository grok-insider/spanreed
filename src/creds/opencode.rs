use super as creds;
use std::path::PathBuf;

pub fn config_home() -> PathBuf {
    opencode_xdg_root(std::env::var_os("XDG_CONFIG_HOME"), "~/.config")
}

pub fn data_home() -> PathBuf {
    opencode_xdg_root(std::env::var_os("XDG_DATA_HOME"), "~/.local/share")
}

// OpenCode uses xdg-basedir on every platform, including Windows and macOS.
// Spanreed's own OS-native directory defaults do not describe client files.
fn opencode_xdg_root(value: Option<std::ffi::OsString>, fallback: &str) -> PathBuf {
    value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| creds::expand(fallback))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opencode_uses_xdg_defaults_instead_of_native_known_folders() {
        assert_eq!(
            opencode_xdg_root(None, "~/.config"),
            creds::expand("~/.config")
        );
        assert_eq!(
            opencode_xdg_root(None, "~/.local/share"),
            creds::expand("~/.local/share")
        );
        assert_eq!(
            opencode_xdg_root(Some("".into()), "~/.config"),
            creds::expand("~/.config")
        );
    }

    #[test]
    fn opencode_xdg_override_is_authoritative() {
        let custom = std::env::temp_dir().join("opencode-custom-root");
        assert_eq!(
            opencode_xdg_root(Some(custom.clone().into_os_string()), "~/.config"),
            custom
        );
    }
}
