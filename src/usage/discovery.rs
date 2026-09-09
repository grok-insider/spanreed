use fabrials_providers::usage::catalog::ClientDefinition;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UsageSettings {
    pub additional_roots: BTreeMap<String, Vec<String>>,
    pub disabled_clients: Vec<String>,
}

pub fn settings() -> Result<UsageSettings, String> {
    let path = crate::app::config_dir().join("usage-sources.json");
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid usage source settings: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(UsageSettings::default()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save(value: &UsageSettings) -> Result<(), String> {
    let clients = fabrials_providers::usage::catalog::clients();
    for id in value
        .additional_roots
        .keys()
        .chain(value.disabled_clients.iter())
    {
        if !clients.iter().any(|c| &c.id == id) {
            return Err(format!("Unknown usage client: {id}"));
        }
    }
    for path in value.additional_roots.values().flatten() {
        if !crate::creds::expand(path).is_absolute() {
            return Err("Usage roots must be absolute paths".into());
        }
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    fabrials_runtime::files::atomic_write_private(
        &crate::app::config_dir().join("usage-sources.json"),
        &bytes,
    )
    .map_err(|e| e.to_string())
}

pub fn roots(client: &ClientDefinition, settings: &UsageSettings) -> Vec<PathBuf> {
    let home = crate::creds::expand("~");
    let mut paths = Vec::new();
    let base = match client.root.as_str() {
        "XdgData" => crate::creds::data_home(),
        "Config" => crate::app::data_dir().join("usage-cache"),
        "AppData" => crate::creds::config_home(),
        "EnvVar" => client
            .environment
            .as_deref()
            .and_then(crate::creds::env)
            .map(|v| crate::creds::expand(&v))
            .unwrap_or_else(|| home.join(client.fallback.as_deref().unwrap_or(""))),
        "ReasonixHome" => crate::creds::env("REASONIX_STATE_HOME")
            .or_else(|| crate::creds::env("REASONIX_HOME"))
            .map(|v| crate::creds::expand(&v))
            .unwrap_or_else(|| crate::creds::data_home().join("reasonix")),
        _ => home.clone(),
    };
    paths.push(base.join(&client.relative));
    if client.root == "Home" {
        if let Some(relative) = client.relative.strip_prefix(".config/") {
            paths.push(crate::creds::config_home().join(relative));
        }
        if let Some(relative) = client.relative.strip_prefix(".local/share/") {
            paths.push(crate::creds::data_home().join(relative));
        }
    }
    let explicit_home = client
        .environment
        .as_deref()
        .and_then(crate::creds::env)
        .is_some();
    if client.root == "Config" {
        paths.push(
            crate::creds::config_home()
                .join("tokscale")
                .join(&client.relative),
        );
    }
    match client.id.as_str() {
        "codex" => {
            paths.push(base.join("archived_sessions"));
            if !explicit_home {
                paths.push(crate::creds::config_home().join("codex/sessions"));
                paths.push(crate::creds::config_home().join("codex/archived_sessions"));
            }
            paths.push(crate::app::data_dir().join("headless/codex"));
        }
        "claude" => {
            paths.push(base.join("transcripts"));
            if !explicit_home {
                paths.push(crate::creds::config_home().join("claude/projects"));
            }
        }
        "hermes" => paths.push(base.join("profiles")),
        "kimi" => paths.push(
            crate::creds::env("KIMI_CODE_HOME")
                .map(|root| crate::creds::expand(&root))
                .unwrap_or_else(|| home.join(".kimi-code"))
                .join("sessions"),
        ),
        "goose" => {
            if let Some(root) = crate::creds::env("GOOSE_PATH_ROOT") {
                paths.clear();
                paths.push(crate::creds::expand(&root).join("data/sessions/sessions.db"));
            }
        }
        "crush" => {
            if let Some(root) = crate::creds::env("CRUSH_GLOBAL_DATA") {
                paths.push(crate::creds::expand(&root).join("projects.json"));
            }
        }
        "codebuff" | "freebuff" => {
            if !explicit_home {
                for channel in ["manicode", "manicode-dev", "manicode-staging"] {
                    paths.push(crate::creds::config_home().join(channel).join("projects"));
                }
            }
        }
        "copilot" => paths.push(crate::creds::config_home().join("Code/User/workspaceStorage")),
        "opencode" => paths.push(crate::creds::data_home().join("opencode/opencode.db")),
        "cursor" => paths.push(crate::app::data_dir().join("usage-cache/cursor")),
        "grok" => paths.push(base.join("logs/unified.jsonl")),
        "kiro" => paths.push(crate::creds::data_home().join("amazon-q/data.sqlite3")),
        "devin-desktop" => paths.push(crate::creds::config_home().join("Devin/User/acp-events")),
        "zcode" => paths.push(home.join(".zcode/zcode.db")),
        _ => {}
    }
    #[cfg(target_os = "windows")]
    if matches!(client.id.as_str(), "cline" | "roocode" | "kilocode") {
        if let Some(relative) = client.relative.strip_prefix(".config/") {
            paths.push(crate::creds::config_home().join(relative));
        }
    }
    if let Some(extra) = settings.additional_roots.get(&client.id) {
        for raw in extra {
            let path = crate::creds::expand(raw);
            if client.id == "codex"
                && (path.join("sessions").is_dir() || path.join("archived_sessions").is_dir())
            {
                paths.push(path.join("sessions"));
                paths.push(path.join("archived_sessions"));
            } else {
                paths.push(path);
            }
        }
    }
    paths
}

pub struct Discovery {
    pub files: Vec<PathBuf>,
    pub errors: Vec<String>,
}

pub fn discover(client: &ClientDefinition, settings: &UsageSettings) -> Discovery {
    let mut result = Discovery {
        files: Vec::new(),
        errors: Vec::new(),
    };
    let mut visited = HashSet::new();
    for root in roots(client, settings) {
        walk(&root, client, &mut visited, &mut result, 0);
    }
    result.files.sort();
    result
}

fn walk(
    path: &Path,
    client: &ClientDefinition,
    visited: &mut HashSet<PathBuf>,
    out: &mut Discovery,
    depth: usize,
) {
    if depth > 24 {
        out.errors
            .push("Usage directory nesting exceeds 24 levels".into());
        return;
    }
    let metadata = match std::fs::metadata(path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            out.errors
                .push(format!("Cannot inspect {}: {e}", path.display()));
            return;
        }
    };
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            out.errors.push(e.to_string());
            return;
        }
    };
    if !visited.insert(canonical.clone()) {
        return;
    }
    if metadata.is_file()
        && client.id == "crush"
        && path.file_name().is_some_and(|n| n == "projects.json")
    {
        let registry = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| e.to_string())
            });
        match registry {
            Ok(value) => match value.get("projects").and_then(|v| v.as_array()) {
                Some(projects) => {
                    for project in projects {
                        let (Some(root), Some(data)) = (
                            project.get("path").and_then(|v| v.as_str()),
                            project.get("data_dir").and_then(|v| v.as_str()),
                        ) else {
                            out.errors
                                .push("Invalid Crush project registry entry".into());
                            continue;
                        };
                        let data = PathBuf::from(data);
                        let directory = if data.is_absolute() {
                            data
                        } else {
                            Path::new(root).join(data)
                        };
                        walk(&directory.join("crush.db"), client, visited, out, depth + 1);
                    }
                }
                None => out.errors.push("Invalid Crush project registry".into()),
            },
            Err(error) => out
                .errors
                .push(format!("Cannot read Crush registry: {error}")),
        }
        return;
    }
    if metadata.is_file() {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if client
            .pattern
            .split('|')
            .any(|pattern| wildcard(pattern, name))
            || extra_file(&client.id, name)
        {
            out.files.push(canonical);
        }
        return;
    }
    if !metadata.is_dir() {
        return;
    }
    let entries = match std::fs::read_dir(path) {
        Ok(v) => v,
        Err(e) => {
            out.errors.push(e.to_string());
            return;
        }
    };
    for entry in entries {
        match entry {
            Ok(entry) => walk(&entry.path(), client, visited, out, depth + 1),
            Err(e) => out.errors.push(e.to_string()),
        }
    }
}

fn extra_file(client: &str, name: &str) -> bool {
    match client {
        "opencode" => name == "opencode.db",
        "crush" => name == "crush.db",
        "zcode" => name == "zcode.db",
        "kiro" => name == "data.sqlite3",
        "openclaw" => {
            name == "sessions.json" || name == "sessions.db" || name == "openclaw-agent.sqlite"
        }
        "grok" => name.ends_with(".jsonl"),
        _ => false,
    }
}

fn wildcard(pattern: &str, value: &str) -> bool {
    if let Some((left, right)) = pattern.split_once('*') {
        if right.contains('*') {
            let Some(rest) = value.strip_prefix(left) else {
                return false;
            };
            return (0..=rest.len())
                .filter(|i| rest.is_char_boundary(*i))
                .any(|i| wildcard(right, &rest[i..]));
        }
        value.starts_with(left) && value.ends_with(right) && value.len() >= left.len() + right.len()
    } else {
        pattern == value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn patterns_do_not_accept_unrelated_credentials() {
        assert!(wildcard("*.jsonl*", "session.jsonl.deleted"));
        assert!(wildcard("T-*.json", "T-session.json"));
        assert!(!wildcard("T-*.json", "auth.json"));
        assert!(!wildcard("*.jsonl", "auth.json"));
    }
}
