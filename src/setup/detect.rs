//! Detect Grok Build and OpenCode installs / configs.

use std::path::PathBuf;

use crate::creds;

#[derive(Debug, Clone, Default)]
pub struct Detection {
    pub grok: ClientHit,
    pub opencode: OpenCodeHit,
}

#[derive(Debug, Clone, Default)]
pub struct ClientHit {
    pub detected: bool,
    pub reasons: Vec<String>,
}

impl ClientHit {
    pub fn hint(&self) -> String {
        if !self.detected {
            return "not found".into();
        }
        if self.reasons.is_empty() {
            "detected".into()
        } else {
            self.reasons.join(", ")
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OpenCodeHit {
    pub detected: bool,
    pub config_path: Option<PathBuf>,
    pub reasons: Vec<String>,
}

impl OpenCodeHit {
    pub fn hint(&self) -> String {
        if !self.detected {
            return "not found".into();
        }
        if let Some(p) = &self.config_path {
            return format!("{}", p.display());
        }
        if self.reasons.is_empty() {
            "detected".into()
        } else {
            self.reasons.join(", ")
        }
    }
}

pub fn scan() -> Detection {
    Detection {
        grok: detect_grok(),
        opencode: detect_opencode(),
    }
}

fn detect_grok() -> ClientHit {
    let mut reasons = Vec::new();
    let home = creds::expand("~/.grok");
    if home.join("auth.json").exists() {
        reasons.push("~/.grok/auth.json".into());
    }
    if home.join("config.toml").exists() {
        reasons.push("~/.grok/config.toml".into());
    }
    if which_on_path("grok") {
        reasons.push("grok on PATH".into());
    }
    ClientHit {
        detected: !reasons.is_empty(),
        reasons,
    }
}

fn detect_opencode() -> OpenCodeHit {
    let mut reasons = Vec::new();
    let config_path = opencode_config_candidates()
        .into_iter()
        .find(|p| p.exists());
    if let Some(ref p) = config_path {
        reasons.push(format!("{}", p.display()));
    }
    // auth under data dir
    let auth = opencode_data_dir().join("auth.json");
    if auth.exists() {
        reasons.push(format!("{}", auth.display()));
    }
    if which_on_path("opencode") {
        reasons.push("opencode on PATH".into());
    }
    OpenCodeHit {
        detected: !reasons.is_empty(),
        config_path,
        reasons,
    }
}

/// Global OpenCode config candidates (never project-local).
pub fn opencode_config_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let cfg = creds::config_home();
    v.push(cfg.join("opencode").join("opencode.json"));
    // Some installs use ~/.config even when dirs::config_dir differs.
    let xdg = creds::expand("~/.config/opencode/opencode.json");
    if !v.iter().any(|p| p == &xdg) {
        v.push(xdg);
    }
    v
}

pub fn opencode_data_dir() -> PathBuf {
    creds::data_home().join("opencode")
}

/// Preferred path to write: existing config, else first candidate.
pub fn opencode_config_write_path(hit: &OpenCodeHit) -> PathBuf {
    hit.config_path
        .clone()
        .unwrap_or_else(|| opencode_config_candidates()[0].clone())
}

fn which_on_path(name: &str) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if p.is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            let pe = dir.join(format!("{name}.exe"));
            if pe.is_file() {
                return true;
            }
            let pc = dir.join(format!("{name}.cmd"));
            if pc.is_file() {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_candidates_non_empty() {
        assert!(!opencode_config_candidates().is_empty());
    }
}
