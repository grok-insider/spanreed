//! Read-only diagnostics for endpoint overrides outside the reviewed TOML file.
use std::io::Read;

const OVERRIDES: [&str; 3] = [
    "GROK_CLI_CHAT_PROXY_BASE_URL",
    "GROK_XAI_API_BASE_URL",
    "GROK_MODELS_BASE_URL",
];

fn referenced_overrides(script: &[u8]) -> Vec<&'static str> {
    let Ok(text) = std::str::from_utf8(script) else {
        return Vec::new();
    };
    if !text.starts_with("#!") && !text.trim_start().starts_with("@echo") {
        return Vec::new();
    }
    OVERRIDES
        .into_iter()
        .filter(|name| text.contains(name))
        .collect()
}

pub(super) fn warnings() -> Vec<String> {
    let mut warnings = Vec::new();
    for name in OVERRIDES {
        if std::env::var_os(name).is_some_and(|value| !value.is_empty()) {
            warnings.push(format!("{name} is set in this environment and can override the saved endpoint. Review the launch environment before using this connection."));
        }
    }
    let paths = std::env::var_os("PATH").unwrap_or_default();
    'search: for directory in std::env::split_paths(&paths) {
        for name in ["grok", "grok.exe", "grok.cmd"] {
            let path = directory.join(name);
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            let mut bytes = Vec::new();
            if let Ok(file) = std::fs::File::open(&path) {
                if file.take(64 * 1024).read_to_end(&mut bytes).is_ok() {
                    let names = referenced_overrides(&bytes);
                    if !names.is_empty() {
                        warnings.push(format!("The grok launcher references {}. It may override this file; review the wrapper or use a launch command with the intended endpoints. Endpoint values and credentials are not shown.", names.join(", ")));
                    }
                }
            }
            break 'search;
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_diagnostic_returns_names_without_secret_values() {
        let names = referenced_overrides(b"#!/bin/sh\nexport GROK_CLI_CHAT_PROXY_BASE_URL='https://user:secret@example.invalid'\nexport XAI_API_KEY=private\n");
        assert_eq!(names, vec!["GROK_CLI_CHAT_PROXY_BASE_URL"]);
        assert!(referenced_overrides(b"\x7fELF GROK_MODELS_BASE_URL").is_empty());
        assert!(referenced_overrides(b"#!/bin/sh\nexec /bin/grok\n").is_empty());
        assert_eq!(
            referenced_overrides(b"@echo off\nset GROK_MODELS_BASE_URL=private"),
            vec!["GROK_MODELS_BASE_URL"]
        );
    }
}
