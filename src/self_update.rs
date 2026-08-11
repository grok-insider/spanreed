//! Check GitHub Releases and optionally replace the installed binary.
//!
//! ```text
//! spanreed self-update --check [--json]
//! spanreed self-update [--yes] [--dry-run]
//! ```
//!
//! Never applies an update without an explicit CLI invocation (or tray menu
//! action that runs this command). `SPANREED_OFFLINE=1` refuses network calls.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sha2::{Digest, Sha256};

use crate::creds;
use crate::http;
use crate::setup;

const DEFAULT_REPO: &str = "grok-insider/spanreed";

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub current: String,
    pub latest: String,
    pub newer: bool,
    pub tag: String,
    pub asset_name: String,
    pub asset_url: String,
    pub sha_url: String,
}

#[derive(Debug, serde::Serialize)]
struct CheckJson<'a> {
    current: &'a str,
    latest: &'a str,
    newer: bool,
    tag: &'a str,
    asset_name: &'a str,
    asset_url: &'a str,
    sha_url: &'a str,
}

/// CLI entry for `self-update`.
pub fn cmd(args: &[String]) -> ExitCode {
    let mut check_only = false;
    let mut json = false;
    let mut yes = false;
    let mut dry_run = false;
    for a in args {
        match a.as_str() {
            "--check" => check_only = true,
            "--json" => json = true,
            "--yes" | "-y" => yes = true,
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("self-update: unknown flag: {other}");
                print_help();
                return ExitCode::FAILURE;
            }
        }
    }

    if offline() {
        eprintln!("self-update: SPANREED_OFFLINE=1 — not checking GitHub");
        return ExitCode::FAILURE;
    }

    let result = match check_for_update() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("self-update: {e}");
            return ExitCode::FAILURE;
        }
    };

    if json {
        let j = CheckJson {
            current: &result.current,
            latest: &result.latest,
            newer: result.newer,
            tag: &result.tag,
            asset_name: &result.asset_name,
            asset_url: &result.asset_url,
            sha_url: &result.sha_url,
        };
        println!("{}", serde_json::to_string_pretty(&j).unwrap_or_default());
    } else if result.newer {
        println!(
            "update available: {} → {} ({})",
            result.current, result.latest, result.tag
        );
        println!("asset: {}", result.asset_name);
    } else {
        println!("up to date: {} ({})", result.current, result.tag);
    }

    if check_only {
        return if result.newer {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }

    if !result.newer {
        return ExitCode::SUCCESS;
    }

    if !yes && !dry_run && !confirm_apply(&result) {
        eprintln!("self-update: cancelled");
        return ExitCode::FAILURE;
    }

    match apply_update(&result, dry_run) {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("self-update: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "spanreed self-update — check or install the latest GitHub Release\n\n\
         \t--check       Only report; exit 0 if current, 2 if newer, 1 on error\n\
         \t--json        Machine-readable check result\n\
         \t--yes / -y    Apply without interactive confirmation\n\
         \t--dry-run     Download + verify checksum; do not replace the binary\n\
         \nEnv: SPANREED_REPO (default {DEFAULT_REPO}), SPANREED_OFFLINE=1"
    );
}

fn offline() -> bool {
    creds::env("SPANREED_OFFLINE").is_some()
}

fn repo() -> String {
    creds::env("SPANREED_REPO").unwrap_or_else(|| DEFAULT_REPO.into())
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Triple used in GitHub Release asset names (matches install scripts).
pub fn target_triple() -> Result<&'static str, String> {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        _ => Err(format!("unsupported platform for self-update: {os}/{arch}")),
    }
}

/// Compare `a` and `b` as `x.y.z` (optional leading `v`). Returns true if `b` > `a`.
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_semver(current), parse_semver(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches('v');
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Parse GitHub Releases API JSON into tag + version (no network).
pub fn parse_latest_tag(body: &str) -> Result<(String, String), String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid releases JSON: {e}"))?;
    let tag = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "releases JSON missing tag_name".to_string())?
        .to_string();
    let version = tag.trim_start_matches('v').to_string();
    if parse_semver(&version).is_none() {
        return Err(format!("unparseable release version in tag: {tag}"));
    }
    Ok((tag, version))
}

/// Parse a `.sha256` sidecar file (`hash  filename` or `hash`).
pub fn parse_sha256_file(text: &str) -> Result<String, String> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .ok_or_else(|| "empty sha256 file".to_string())?;
    let hash = line
        .split_whitespace()
        .next()
        .ok_or_else(|| "malformed sha256 line".to_string())?
        .to_ascii_lowercase();
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("invalid sha256 hash: {hash}"));
    }
    Ok(hash)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let dig = hasher.finalize();
    dig.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn check_for_update() -> Result<CheckResult, String> {
    let current = current_version().to_string();
    let triple = target_triple()?;
    let repo = repo();
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let resp = http::Request::get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()?;
    if !(200..300).contains(&resp.status) {
        return Err(format!(
            "GitHub releases API HTTP {} — {}",
            resp.status,
            resp.body.chars().take(200).collect::<String>()
        ));
    }
    check_from_release_json(&resp.body, &current, triple, &repo)
}

fn confirm_apply(r: &CheckResult) -> bool {
    eprint!(
        "Install spanreed {} → {} now? [y/N] ",
        r.current, r.latest
    );
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    http::Request::get(url.to_string())
        .header("Accept", "application/octet-stream")
        .send_bytes()
        .and_then(|r| {
            if !(200..300).contains(&r.status) {
                Err(format!(
                    "download HTTP {} for {}",
                    r.status,
                    url.chars().take(80).collect::<String>()
                ))
            } else {
                Ok(r.body)
            }
        })
}

pub fn apply_update(r: &CheckResult, dry_run: bool) -> Result<String, String> {
    if offline() {
        return Err("SPANREED_OFFLINE=1".into());
    }

    eprintln!("downloading {} …", r.asset_name);
    let archive = download_bytes(&r.asset_url)?;
    let sha_text = download_bytes(&r.sha_url)
        .and_then(|b| String::from_utf8(b).map_err(|e| format!("sha256 file not utf-8: {e}")))?;
    let expected = parse_sha256_file(&sha_text)?;
    let actual = sha256_hex(&archive);
    if actual != expected {
        return Err(format!(
            "checksum mismatch: expected {expected}, got {actual}"
        ));
    }
    eprintln!("checksum ok ({actual})");

    let bin_bytes = extract_binary(&archive, cfg!(windows))?;
    let dest = setup::install_bin_path();
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }

    if dry_run {
        return Ok(format!(
            "dry-run ok: verified {} ({} bytes) would install to {}",
            r.asset_name,
            bin_bytes.len(),
            dest.display()
        ));
    }

    stop_capture_processes();
    replace_binary(&dest, &bin_bytes)?;

    // Best-effort restart capture if it was part of normal setup.
    let ensure_msg = match setup::service_ensure(false) {
        Ok(m) => format!("; capture: {m}"),
        Err(e) => format!("; capture ensure: {e} (run: spanreed capture ensure)"),
    };

    Ok(format!(
        "installed {} → {} ({}){}",
        r.latest,
        dest.display(),
        r.tag,
        ensure_msg
    ))
}

fn extract_binary(archive: &[u8], windows: bool) -> Result<Vec<u8>, String> {
    if windows {
        extract_zip_binary(archive)
    } else {
        extract_tar_gz_binary(archive)
    }
}

fn extract_zip_binary(archive: &[u8]) -> Result<Vec<u8>, String> {
    let cursor = io::Cursor::new(archive);
    let mut zip = zip::ZipArchive::new(cursor).map_err(|e| format!("zip open: {e}"))?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| format!("zip entry {i}: {e}"))?;
        let name = file.name().to_string();
        let base = Path::new(&name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if base == "spanreed.exe" || base == "spanreed" {
            let mut buf = Vec::new();
            io::copy(&mut file, &mut buf).map_err(|e| format!("zip read: {e}"))?;
            return Ok(buf);
        }
    }
    Err("zip archive has no spanreed binary".into())
}

fn extract_tar_gz_binary(archive: &[u8]) -> Result<Vec<u8>, String> {
    use flate2::read::GzDecoder;
    let gz = GzDecoder::new(io::Cursor::new(archive));
    let mut tar = tar::Archive::new(gz);
    let entries = tar.entries().map_err(|e| format!("tar entries: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("tar entry: {e}"))?;
        let path = entry.path().map_err(|e| format!("tar path: {e}"))?;
        let base = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if base == "spanreed" || base == "spanreed.exe" {
            let mut buf = Vec::new();
            io::copy(&mut entry, &mut buf).map_err(|e| format!("tar read: {e}"))?;
            return Ok(buf);
        }
    }
    Err("tar.gz archive has no spanreed binary".into())
}

fn replace_binary(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = dest.with_extension("new");
    {
        let mut f = File::create(&tmp).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        f.write_all(bytes)
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = f
                .metadata()
                .map_err(|e| format!("metadata: {e}"))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&tmp, perms).map_err(|e| format!("chmod: {e}"))?;
        }
    }

    #[cfg(windows)]
    {
        let old = dest.with_extension("exe.old");
        let _ = fs::remove_file(&old);
        if dest.exists() {
            fs::rename(dest, &old).map_err(|e| {
                format!(
                    "could not move old binary aside (is it running?): {}: {e}",
                    dest.display()
                )
            })?;
        }
        fs::rename(&tmp, dest).map_err(|e| format!("install new binary: {e}"))?;
        let _ = fs::remove_file(&old);
    }

    #[cfg(not(windows))]
    {
        // Atomic replace on the same filesystem.
        fs::rename(&tmp, dest).map_err(|e| format!("install new binary: {e}"))?;
    }

    Ok(())
}

/// Stop capture workers so the install path can be replaced on Windows.
fn stop_capture_processes() {
    #[cfg(windows)]
    {
        let script = r#"
Get-CimInstance Win32_Process -Filter "Name='spanreed.exe'" -ErrorAction SilentlyContinue |
  Where-Object { $_.CommandLine -match 'capture' } |
  ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Start-Sleep -Milliseconds 400
"#;
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output();
    }
    #[cfg(not(windows))]
    {
        // Best-effort: pkill patterns; ignore errors.
        let _ = std::process::Command::new("pkill")
            .args(["-f", "spanreed capture"])
            .output();
    }
}

/// Path helper used by tray / tests.
#[allow(dead_code)]
pub fn install_path() -> PathBuf {
    setup::install_bin_path()
}

/// Build the release asset basename for a version + triple (testable without net).
pub fn asset_name_for(version: &str, triple: &str) -> String {
    if triple.contains("windows") {
        format!("spanreed-{version}-{triple}.zip")
    } else {
        format!("spanreed-{version}-{triple}.tar.gz")
    }
}

/// From a releases/latest JSON body + triple, produce a check result vs `current`.
pub fn check_from_release_json(
    body: &str,
    current: &str,
    triple: &str,
    repo: &str,
) -> Result<CheckResult, String> {
    let (tag, latest) = parse_latest_tag(body)?;
    let asset_name = asset_name_for(&latest, triple);
    let base = format!("https://github.com/{repo}/releases/download/{tag}");
    let asset_url = format!("{base}/{asset_name}");
    let sha_url = format!("{asset_url}.sha256");
    Ok(CheckResult {
        newer: is_newer(current, &latest),
        current: current.to_string(),
        latest,
        tag,
        asset_name,
        asset_url,
        sha_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_newer() {
        assert!(is_newer("0.0.1", "0.0.2"));
        assert!(is_newer("0.0.2", "0.1.0"));
        assert!(!is_newer("0.0.2", "0.0.2"));
        assert!(!is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("v0.0.1", "v0.0.2"));
    }

    #[test]
    fn parse_tag() {
        let body = r#"{"tag_name":"v0.0.2","name":"v0.0.2"}"#;
        let (tag, ver) = parse_latest_tag(body).unwrap();
        assert_eq!(tag, "v0.0.2");
        assert_eq!(ver, "0.0.2");
    }

    #[test]
    fn parse_sha() {
        assert_eq!(
            parse_sha256_file(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  spanreed.zip\n"
            )
            .unwrap(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        assert!(parse_sha256_file("short").is_err());
    }

    #[test]
    fn sha256_known() {
        // echo -n "abc" | sha256sum
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn asset_names_match_release_layout() {
        assert_eq!(
            asset_name_for("0.0.2", "x86_64-pc-windows-msvc"),
            "spanreed-0.0.2-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            asset_name_for("0.0.2", "x86_64-unknown-linux-musl"),
            "spanreed-0.0.2-x86_64-unknown-linux-musl.tar.gz"
        );
        assert_eq!(
            asset_name_for("1.2.3", "aarch64-apple-darwin"),
            "spanreed-1.2.3-aarch64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn check_from_fixture_detects_newer() {
        let body = r#"{"tag_name":"v0.0.9","name":"v0.0.9"}"#;
        let r = check_from_release_json(
            body,
            "0.0.2",
            "x86_64-pc-windows-msvc",
            "grok-insider/spanreed",
        )
        .unwrap();
        assert!(r.newer);
        assert_eq!(r.latest, "0.0.9");
        assert_eq!(r.asset_name, "spanreed-0.0.9-x86_64-pc-windows-msvc.zip");
        assert!(r.asset_url.ends_with(&r.asset_name));
        assert!(r.sha_url.ends_with(".sha256"));
    }

    #[test]
    fn check_from_fixture_up_to_date() {
        let body = r#"{"tag_name":"v0.0.2"}"#;
        let r = check_from_release_json(
            body,
            "0.0.2",
            "x86_64-unknown-linux-musl",
            "grok-insider/spanreed",
        )
        .unwrap();
        assert!(!r.newer);
        assert_eq!(r.tag, "v0.0.2");
    }

    #[test]
    fn target_triple_is_known_for_this_host() {
        let t = target_triple().expect("host should be a release target");
        assert!(
            t.contains("linux") || t.contains("darwin") || t.contains("windows"),
            "unexpected triple {t}"
        );
    }
}
