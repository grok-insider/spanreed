//! Discover addons (compiled-in, config toml, PATH) and dispatch.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Deserialize;

use super::protocol::{dispatch_inproc, AddonRequest, AddonResponse};
use super::{command_owner, compiled_in, Addon};
use crate::app;
use crate::model::ProviderOutput;

const EXEC_TIMEOUT_MS: u64 = 4000;

#[derive(Debug, Clone)]
pub struct AddonListing {
    pub id: String,
    pub name: String,
    pub source: &'static str,
    pub enabled: bool,
    pub commands: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    id: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

pub fn cmd(args: &[String]) -> std::process::ExitCode {
    match args.first().map(String::as_str) {
        None | Some("list") => {
            for row in list_all() {
                let on = if row.enabled { "on" } else { "off" };
                let cmds = if row.commands.is_empty() {
                    String::new()
                } else {
                    format!(" cmds={}", row.commands.join(","))
                };
                println!(
                    "{:<16} {:<8} {:<12} {}{cmds}",
                    row.id, on, row.source, row.name
                );
            }
            std::process::ExitCode::SUCCESS
        }
        Some("enable") | Some("disable") => {
            eprintln!("addon enable/disable: edit ~/.config/spanreed/addons/<id>.toml");
            std::process::ExitCode::FAILURE
        }
        Some(other) => {
            eprintln!("unknown addon subcommand: {other}\nusage: spanreed addon list");
            std::process::ExitCode::FAILURE
        }
    }
}

pub fn list_all() -> Vec<AddonListing> {
    let mut out = Vec::new();
    for a in compiled_in() {
        let h = a.hello();
        out.push(AddonListing {
            id: h.id,
            name: h.name,
            source: "inproc",
            enabled: true,
            commands: h.caps.commands,
        });
    }
    for (path, man) in read_manifests() {
        if out.iter().any(|r| r.id == man.id) {
            continue;
        }
        out.push(AddonListing {
            id: man.id,
            name: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            source: "toml",
            enabled: man.enabled.unwrap_or(true),
            commands: Vec::new(),
        });
    }
    for (id, exe) in path_addons() {
        if out.iter().any(|r| r.id == id) {
            continue;
        }
        out.push(AddonListing {
            id,
            name: exe.display().to_string(),
            source: "path",
            enabled: true,
            commands: Vec::new(),
        });
    }
    out
}

fn manifests_dir() -> PathBuf {
    app::config_dir().join("addons")
}

fn read_manifests() -> Vec<(PathBuf, Manifest)> {
    let dir = manifests_dir();
    let rd = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut rows = Vec::new();
    for ent in rd.flatten() {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(man) = toml::from_str::<Manifest>(&text) {
            rows.push((path, man));
        }
    }
    rows
}

fn path_addons() -> Vec<(String, PathBuf)> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for dir in std::env::split_paths(&path) {
        let rd = match std::fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for ent in rd.flatten() {
            let name = ent.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(id) = name.strip_prefix("spanreed-addon-") else {
                continue;
            };
            let id = id.strip_suffix(".exe").unwrap_or(id);
            if id.is_empty() {
                continue;
            }
            if found.iter().any(|(i, _)| i == id) {
                continue;
            }
            found.push((id.to_string(), ent.path()));
        }
    }
    found
}

/// Extra provider ids contributed by compiled-in addons.
pub fn extra_provider_ids() -> Vec<(String, String, bool)> {
    let mut rows = Vec::new();
    for a in compiled_in() {
        let h = a.hello();
        for pid in h.caps.providers {
            let detected = a.detect(&pid);
            rows.push((pid, h.name.clone(), detected));
        }
    }
    rows
}

pub fn extra_detected_outputs() -> Vec<ProviderOutput> {
    let mut outs = Vec::new();
    for a in compiled_in() {
        let h = a.hello();
        for pid in h.caps.providers {
            if a.detect(&pid) {
                outs.push(a.probe(&pid));
            }
        }
    }
    outs
}

pub fn probe_extra_one(id: &str) -> Option<ProviderOutput> {
    for a in compiled_in() {
        if a.hello().caps.providers.iter().any(|p| p == id) {
            return Some(a.probe(id));
        }
    }
    None
}

/// If `prefix` is owned by an addon, run its command and return an exit code.
pub fn dispatch_prefix(prefix: &str, rest: &[String]) -> Option<std::process::ExitCode> {
    if let Some(addon) = command_owner(prefix) {
        let mut argv = vec![prefix.to_string()];
        argv.extend(rest.iter().cloned());
        return Some(run_command(addon.as_ref(), &argv));
    }
    if let Some(exe) = exec_for_command(prefix) {
        return Some(run_exec_command(&exe, prefix, rest));
    }
    None
}

fn exec_for_command(prefix: &str) -> Option<PathBuf> {
    for (_p, man) in read_manifests() {
        if !man.enabled.unwrap_or(true) {
            continue;
        }
        if man.id == prefix {
            return man.command.map(PathBuf::from);
        }
    }
    path_addons()
        .into_iter()
        .find(|(id, _)| id == prefix)
        .map(|(_, p)| p)
}

fn run_command(addon: &dyn Addon, argv: &[String]) -> std::process::ExitCode {
    let (stdout, stderr, code) = addon.command(argv);
    if !stdout.is_empty() {
        print!("{stdout}");
        if !stdout.ends_with('\n') {
            println!();
        }
    }
    if !stderr.is_empty() {
        eprint!("{stderr}");
        if !stderr.ends_with('\n') {
            eprintln!();
        }
    }
    if code == 0 {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

fn run_exec_command(exe: &Path, prefix: &str, rest: &[String]) -> std::process::ExitCode {
    let mut argv = vec![prefix.to_string()];
    argv.extend(rest.iter().cloned());
    match call_exec(exe, &AddonRequest::command(argv)) {
        Ok(resp) => {
            if let Some(s) = resp.stdout {
                print!("{s}");
            }
            if let Some(s) = resp.stderr {
                eprint!("{s}");
            }
            if resp.code.unwrap_or(1) == 0 {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("addon {prefix}: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// One-shot JSON round-trip with an external addon binary.
pub fn call_exec(exe: &Path, req: &AddonRequest) -> Result<AddonResponse, String> {
    let mut child = Command::new(exe)
        .env("SPANREED_ADDON", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
    {
        let stdin = child.stdin.as_mut().ok_or("stdin")?;
        let line = serde_json::to_string(req).map_err(|e| e.to_string())?;
        writeln!(stdin, "{line}").map_err(|e| e.to_string())?;
    }
    // Best-effort bounded wait (no async). Kill if we exceed the budget.
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > Duration::from_millis(EXEC_TIMEOUT_MS) => {
                let _ = child.kill();
                return Err("addon timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(out.stdout.as_slice());
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    serde_json::from_str(line.trim()).map_err(|e| format!("addon json: {e}"))
}

/// Serve the JSON protocol on stdin/stdout (used by `spanreed-addon-*` bins).
#[allow(dead_code)]
pub fn serve_stdio(addon: &dyn Addon) {
    let stdin = std::io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line).ok().unwrap_or(0) == 0 {
        return;
    }
    let req: AddonRequest = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::to_string(&AddonResponse::err(e.to_string())).unwrap_or_default()
            );
            return;
        }
    };
    let resp = dispatch_inproc(addon, &req);
    if let Ok(s) = serde_json::to_string(&resp) {
        let _ = writeln!(std::io::stdout(), "{s}");
    }
}
