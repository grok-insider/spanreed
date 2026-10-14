//! Check GitHub Releases and optionally replace the installed binary.
//!
//! ```text
//! spanreed self-update --check [--json]
//! spanreed self-update [--yes] [--dry-run]
//! ```
//!
//! Never applies an update without an explicit CLI invocation (or tray menu
//! action that runs this command). `SPANREED_OFFLINE=1` refuses network calls.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::creds;
use crate::http;
use crate::setup;

mod durable;
mod install;
#[cfg(windows)]
mod windows;

pub const DEFAULT_REPO: &str = crate::product::GITHUB_REPO;

pub use spanreed_app::app::updates::CheckResult;
fn offline() -> bool {
    crate::product::env_offline()
}

fn repo() -> String {
    creds::env("SPANREED_REPO").unwrap_or_else(|| DEFAULT_REPO.into())
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// True when this process must not replace its own binary (Nix store / env).
pub fn install_is_nix_managed() -> bool {
    if std::env::var_os("SPANREED_NIX").is_some() {
        return true;
    }
    exe_is_under_nix_store(std::env::current_exe().ok().as_deref())
}

fn exe_is_under_nix_store(exe: Option<&Path>) -> bool {
    let Some(exe) = exe else {
        return false;
    };
    let resolved = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    resolved.starts_with("/nix/store")
}

/// Whether the tray/CLI should offer “Install update”.
///
/// Linux tray builds must not apply GH musl assets (those binaries have no tray).
/// Nix-managed installs are immutable.
pub fn can_apply_self_update() -> bool {
    if platform_apply_blocked_reason(cfg!(windows)).is_some() {
        return false;
    }
    if install_is_nix_managed() {
        return false;
    }
    if cfg!(all(feature = "tray", target_os = "linux")) {
        return false;
    }
    true
}

pub fn apply_blocked_reason() -> Option<&'static str> {
    if install_is_nix_managed() {
        return Some(
            "this install is Nix-managed — update with \
             `nix profile upgrade` or `nix flake update` the \
             github:grok-insider/spanreed input, then rebuild",
        );
    }
    if cfg!(all(feature = "tray", target_os = "linux")) {
        return Some(
            "Linux tray builds cannot self-replace with musl GH assets (those have no tray)",
        );
    }
    platform_apply_blocked_reason(cfg!(windows))
}

fn platform_apply_blocked_reason(windows: bool) -> Option<&'static str> {
    windows.then_some(
        "Windows self-update is disabled until candidate processes are assigned to a Job before execution and executable ACLs strictly exclude other writers; version checks remain available. No update has been downloaded or applied",
    )
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
    let mut parts = s.strip_prefix('v').unwrap_or(s).split('.');
    let number = |value: &str| {
        if value.is_empty()
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            None
        } else {
            value.parse().ok()
        }
    };
    let major = number(parts.next()?)?;
    let minor = number(parts.next()?)?;
    let patch = number(parts.next()?)?;
    parts.next().is_none().then_some((major, minor, patch))
}

/// Parse GitHub Releases API JSON into tag + version (no network).
pub fn parse_latest_tag(body: &str) -> Result<(String, String), String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid releases JSON: {e}"))?;
    if ["draft", "prerelease"]
        .iter()
        .any(|field| v.get(field).and_then(serde_json::Value::as_bool) != Some(false))
    {
        return Err("release must explicitly be published and stable".into());
    }
    let tag = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "releases JSON missing tag_name".to_string())?
        .to_string();
    if parse_semver(&tag).is_none() {
        return Err(format!("unparseable release version in tag: {tag}"));
    }
    let version = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    Ok((tag, version))
}

/// Parse a `.sha256` sidecar file (`hash  filename` or `hash`).
pub fn parse_sha256_file(text: &str) -> Result<String, String> {
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let line = lines
        .next()
        .ok_or_else(|| "empty sha256 file".to_string())?;
    if lines.next().is_some() || line.split_whitespace().count() > 2 {
        return Err("sha256 sidecar must contain exactly one entry".into());
    }
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

fn parse_asset_sha256(text: &str, asset_name: &str) -> Result<String, String> {
    let hash = parse_sha256_file(text)?;
    let fields: Vec<_> = text.split_whitespace().collect();
    if fields.len() != 2 || fields[1] != asset_name {
        return Err("sha256 sidecar does not identify the selected release asset".into());
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
    if let Some(why) = apply_blocked_reason() {
        return Err(why.into());
    }
    let _host_guard = if dry_run {
        None
    } else {
        let directory = fabrials_agent_host::state_dir::resolve_state_dirs()
            .map_err(|error| format!("cannot locate agent host state: {error}"))?
            .state_dir;
        Some(install::claim_host(&directory)?)
    };

    eprintln!("downloading {} …", r.asset_name);
    let archive = download_bytes(&r.asset_url)?;
    let sha_text = download_bytes(&r.sha_url)
        .and_then(|b| String::from_utf8(b).map_err(|e| format!("sha256 file not utf-8: {e}")))?;
    let expected = parse_asset_sha256(&sha_text, &r.asset_name)?;
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
    let candidate = install::Candidate::prepare(&dest, &bin_bytes, &r.latest)?;

    if dry_run {
        return Ok(format!(
            "dry-run ok: verified {} ({} bytes) would install to {}",
            r.asset_name,
            bin_bytes.len(),
            dest.display()
        ));
    }

    candidate.install(&r.latest)?;
    #[cfg(windows)]
    return Ok(format!(
        "update {} prepared; the verified helper completes after this process exits. Run self-update --status to check completion or --recover if interrupted. Restart running Spanreed processes after completion",
        r.latest
    ));
    #[cfg(not(windows))]
    Ok(format!(
        "installed {} → {} ({}); previous executable retained for self-update --rollback. Restart running Spanreed processes when idle",
        r.latest,
        dest.display(),
        r.tag
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

pub fn recover_update(rollback: bool) -> Result<String, String> {
    if install_is_nix_managed() {
        return Err(
            "this installation is Nix-managed; use its profile generations for recovery".into(),
        );
    }
    let directory = fabrials_agent_host::state_dir::resolve_state_dirs()
        .map_err(|error| error.to_string())?
        .state_dir;
    let _host = install::claim_host(&directory)?;
    durable::recover(&setup::install_bin_path(), rollback)
}

pub fn update_status() -> Result<String, String> {
    durable::status(&install_path())
}

pub fn finish_update() -> Result<String, String> {
    #[cfg(windows)]
    {
        durable::finish()
    }
    #[cfg(not(windows))]
    {
        Err("update helpers are only used on Windows".into())
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
    fn windows_update_gate_is_closed_without_disabling_version_checks() {
        let why = platform_apply_blocked_reason(true).unwrap();
        assert!(why.contains("before execution"));
        assert!(why.contains("ACLs"));
        assert!(why.contains("version checks remain available"));
        assert!(platform_apply_blocked_reason(false).is_none());
        if cfg!(windows) {
            assert!(!can_apply_self_update());
            assert!(apply_blocked_reason().is_some());
        }
        assert!(
            check_from_release_json(
                r#"{"draft":false,"prerelease":false,"tag_name":"v9.9.9"}"#,
                "0.1.0",
                "x86_64-pc-windows-msvc",
                "grok-insider/spanreed",
            )
            .unwrap()
            .newer
        );
    }

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
        let body = r#"{"draft":false,"prerelease":false,"tag_name":"v0.0.2","name":"v0.0.2"}"#;
        let (tag, ver) = parse_latest_tag(body).unwrap();
        assert_eq!(tag, "v0.0.2");
        assert_eq!(ver, "0.0.2");
    }

    #[test]
    fn only_canonical_stable_release_versions_are_accepted() {
        for tag in [
            "v1.2.3.4",
            "vv1.2.3",
            "1.2.3-rc.1",
            "1.2.3+build",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            " 1.2.3",
            "1.2.3 ",
            "1.+2.3",
            "1.2",
            "18446744073709551616.2.3",
        ] {
            assert!(parse_semver(tag).is_none(), "{tag}");
            let body =
                serde_json::json!({"draft":false,"prerelease":false,"tag_name":tag}).to_string();
            assert!(parse_latest_tag(&body).is_err(), "{tag}");
        }
        for flags in [
            serde_json::json!({}),
            serde_json::json!({"draft":true,"prerelease":false}),
            serde_json::json!({"draft":false,"prerelease":true}),
            serde_json::json!({"draft":"false","prerelease":false}),
        ] {
            let mut body = flags;
            body["tag_name"] = serde_json::json!("v1.2.3");
            assert!(parse_latest_tag(&body.to_string()).is_err());
        }
        assert_eq!(parse_semver("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("0.0.0"), Some((0, 0, 0)));
    }

    #[test]
    fn sidecar_must_name_the_single_selected_asset() {
        let hash = "a".repeat(64);
        let expected = "spanreed-1.2.3-x86_64-unknown-linux-musl.tar.gz";
        assert_eq!(
            parse_asset_sha256(&format!("{hash}  {expected}\n"), expected).unwrap(),
            hash
        );
        for content in [
            hash.clone(),
            format!("{hash} different.tar.gz\n"),
            format!("{hash} {expected}\n{hash} other.tar.gz\n"),
            format!("{hash} {expected} extra\n"),
            format!("{hash} ../{expected}\n"),
            format!("{hash} *{expected}\n"),
        ] {
            assert!(parse_asset_sha256(&content, expected).is_err());
        }
        assert!(parse_sha256_file(&format!("{hash}\n{hash}\n")).is_err());
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
        let body = r#"{"draft":false,"prerelease":false,"tag_name":"v0.0.9","name":"v0.0.9"}"#;
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
        let body = r#"{"draft":false,"prerelease":false,"tag_name":"v0.0.2"}"#;
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

    #[test]
    fn nix_store_path_is_managed() {
        assert!(exe_is_under_nix_store(Some(Path::new(
            "/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-spanreed-0.0.4/bin/spanreed"
        ))));
        let fixture = std::env::temp_dir().join(format!(
            "spanreed-unmanaged-{}",
            fabrials_fabric::accounting::new_request_id()
        ));
        std::fs::write(&fixture, b"fixture").unwrap();
        assert!(!exe_is_under_nix_store(Some(&fixture)));
        std::fs::remove_file(fixture).unwrap();
        assert!(!exe_is_under_nix_store(None));
    }

    #[test]
    fn spanreed_nix_env_blocks_apply() {
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("SPANREED_NIX", "1") };
        assert!(install_is_nix_managed());
        assert!(!can_apply_self_update());
        let why = apply_blocked_reason().unwrap();
        assert!(why.contains("Nix-managed"));
        assert!(why.contains("github:grok-insider/spanreed"));
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("SPANREED_NIX") };
    }
}
