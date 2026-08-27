//! Integration tests for the `spanreed` binary.
//!
//! Cargo provides the built binary path via `CARGO_BIN_EXE_spanreed`.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_spanreed")
}

/// Run the binary with args in a clean, isolated HOME so it never touches the
/// developer's real credentials/logs, and capture stdout.
fn run(args: &[&str]) -> (String, std::process::ExitStatus) {
    let (stdout, _stderr, status) = run_full(args, true);
    (stdout, status)
}

/// Full capture (stdout+stderr). When `offline`, sets SPANREED_OFFLINE=1.
fn run_full(args: &[&str], offline: bool) -> (String, String, std::process::ExitStatus) {
    let tmp = std::env::temp_dir().join(format!(
        "spanreed-it-{}-{}",
        std::process::id(),
        args.join("_").replace(['/', ' '], "_")
    ));
    let _ = std::fs::create_dir_all(&tmp);

    let mut cmd = Command::new(bin());
    // Isolate all OS profile dirs the `dirs` crate may consult (Windows uses
    // APPDATA/LOCALAPPDATA, not XDG_* alone).
    cmd.args(args)
        .env("HOME", &tmp)
        .env("USERPROFILE", &tmp)
        .env("APPDATA", tmp.join("appdata"))
        .env("LOCALAPPDATA", tmp.join("localappdata"))
        .env("XDG_CONFIG_HOME", tmp.join("config"))
        .env("XDG_DATA_HOME", tmp.join("data"))
        .env("XDG_CACHE_HOME", tmp.join("cache"))
        .env_remove("ZAI_API_KEY")
        .env_remove("GLM_API_KEY")
        .env_remove("MINIMAX_API_KEY")
        .env_remove("MINIMAX_CN_API_KEY")
        .env_remove("MINIMAX_API_TOKEN")
        .env_remove("SYNTHETIC_API_KEY")
        .env_remove("CLAUDE_CODE_OAUTH_TOKEN")
        .env_remove("CODEX_HOME")
        .env_remove("CLAUDE_CONFIG_DIR")
        // Host machine may have Grok capture wired in the parent environment;
        // that would make `setup status` exit 1 when ports are down.
        .env_remove("GROK_CLI_CHAT_PROXY_BASE_URL");
    if offline {
        cmd.env("SPANREED_OFFLINE", "1");
    } else {
        cmd.env_remove("SPANREED_OFFLINE");
    }
    let out = cmd.output().expect("run spanreed");
    let _ = std::fs::remove_dir_all(&tmp);
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status,
    )
}

#[test]
fn list_shows_all_providers() {
    let (stdout, status) = run(&["list"]);
    assert!(status.success(), "list should exit 0");
    for id in [
        "codex",
        "cursor",
        "grok",
        "opencode-go",
        "amp",
        "zai",
        "minimax",
        "synthetic",
        "kimi",
        "copilot",
        "factory",
        "devin",
        "jetbrains-ai-assistant",
        "kiro",
        "antigravity",
        "perplexity",
    ] {
        assert!(
            stdout.contains(id),
            "list missing provider '{id}'\n{stdout}"
        );
    }
}

#[test]
fn json_is_valid_array_when_nothing_detected() {
    let (stdout, status) = run(&["json"]);
    assert!(status.success());
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert!(v.is_array(), "json output should be an array");
}

#[test]
fn waybar_emits_object_with_required_keys() {
    let (stdout, status) = run(&["waybar"]);
    assert!(status.success());
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert!(v.get("text").is_some());
    assert!(v.get("tooltip").is_some());
    assert!(v.get("class").is_some());
}

#[test]
fn help_lists_subcommands() {
    let (stdout, status) = run(&["help"]);
    assert!(status.success());
    for word in [
        "list",
        "probe",
        "waybar",
        "json",
        "serve",
        "history",
        "capture",
        "ensure",
        "setup",
        "auth",
        "share",
        "sync",
        "self-update",
        "tray",
    ] {
        assert!(stdout.contains(word), "help missing '{word}'");
    }
    assert!(
        stdout.contains("share login") || stdout.contains("login"),
        "help should mention share login\n{stdout}"
    );
}

#[test]
fn share_help_documents_auth_subcommands() {
    let (stdout, status) = run(&["share", "--help"]);
    assert!(status.success(), "share --help failed\n{stdout}");
    for word in ["login", "logout", "status"] {
        assert!(
            stdout.contains(word),
            "share help missing '{word}'\n{stdout}"
        );
    }
    assert!(
        stdout.contains("SPANREED_API_BASE") || stdout.contains("authenticated"),
        "share help should mention auth/API base\n{stdout}"
    );
}

#[test]
fn share_status_without_session_instructs_login() {
    let (stdout, stderr, status) = run_full(&["share", "status"], true);
    assert!(!status.success(), "status should fail without session");
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("not logged in") || combined.contains("share login"),
        "expected login instruction\n{combined}"
    );
}

#[test]
fn share_without_session_instructs_login() {
    // Online path (no OFFLINE) so share attempts auth gate before probe skip.
    let (stdout, stderr, status) = run_full(&["share"], false);
    let combined = format!("{stdout}{stderr}");
    assert!(
        !status.success(),
        "share without session must fail\n{combined}"
    );
    assert!(
        combined.contains("not logged in") || combined.contains("share login"),
        "expected login instruction\n{combined}"
    );
}

#[test]
fn self_update_offline_fails_clearly() {
    let (stdout, stderr, status) = run_full(&["self-update", "--check"], true);
    assert!(!status.success(), "self-update --check must fail offline");
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("SPANREED_OFFLINE"),
        "expected offline message\n{combined}"
    );
}

#[test]
fn setup_status_exits_zero_in_isolated_home() {
    let (stdout, stderr, status) = run_full(&["setup", "status"], true);
    assert!(
        stdout.contains("spanreed setup status"),
        "unexpected status output\n{stdout}"
    );
    assert!(
        stdout.contains("Capture service:"),
        "missing capture service line\n{stdout}"
    );
    assert!(
        stdout.contains("Tray autostart:"),
        "missing tray autostart line\n{stdout}"
    );
    // Prefer exit 0 in a fully isolated profile. On Windows the `dirs` crate
    // uses known folders (not APPDATA env), so host Grok/OpenCode wire can
    // leak and yield exit 1 with the ports-DOWN error — still a valid status.
    if !status.success() {
        let combined = format!("{stdout}{stderr}");
        assert!(
            combined.contains("ports are DOWN") || combined.contains("wired"),
            "setup status failed unexpectedly\n{combined}"
        );
    }
}

#[test]
fn capture_status_reports_state() {
    let (stdout, status) = run(&["capture", "status"]);
    // Ports may or may not be up on the host; just require a clear line and no panic.
    assert!(
        stdout.contains("capture:"),
        "capture status missing line\n{stdout}"
    );
    let _ = status; // 0 if up, 1 if down — both valid
}

#[test]
fn capture_ensure_dry_run_ok() {
    let (stdout, status) = run(&["capture", "ensure", "--dry-run"]);
    assert!(status.success(), "ensure --dry-run failed\n{stdout}");
    assert!(
        stdout.contains("listening") || stdout.contains("would start"),
        "unexpected ensure output\n{stdout}"
    );
}

#[test]
fn setup_dry_run_yes_exits_zero() {
    let (stdout, status) = run(&["setup", "--yes", "--dry-run", "--no-wire"]);
    assert!(status.success(), "setup --yes --dry-run failed\n{stdout}");
    assert!(
        stdout.contains("Dry run") || stdout.contains("Done") || stdout.contains("Non-interactive"),
        "unexpected setup output\n{stdout}"
    );
}

#[test]
fn auth_without_target_fails_cleanly() {
    let (stdout, status) = run(&["auth"]);
    // help/usage on stderr; exit non-zero
    assert!(
        !status.success(),
        "auth with no target should fail\n{stdout}"
    );
}

#[test]
fn copilot_not_detected_without_auth() {
    let (stdout, status) = run(&["list"]);
    assert!(status.success());
    // "copilot" line should show em-dash (not detected), not "detected"
    let line = stdout
        .lines()
        .find(|l| l.starts_with("copilot"))
        .expect("copilot row");
    assert!(
        !line.contains("detected"),
        "copilot must not auto-detect without auth\n{line}"
    );
}

/// The SIGPIPE fix: piping output into a reader that closes early must not
/// panic; the process should terminate cleanly (killed by SIGPIPE or exit 0),
/// never with a Rust panic (exit code 101). Unix-only: Windows has no SIGPIPE.
#[cfg(unix)]
#[test]
fn does_not_panic_on_broken_pipe() {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = Command::new(bin())
        .arg("list")
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");

    // Read a single byte, then drop the pipe to close the read end.
    {
        let mut stdout = child.stdout.take().unwrap();
        let mut one = [0u8; 1];
        let _ = stdout.read(&mut one);
        // stdout dropped here -> downstream pipe closed.
    }

    let status = child.wait().expect("wait");
    // 101 is the Rust panic exit code; we must never see it.
    assert_ne!(status.code(), Some(101), "binary panicked on broken pipe");
}
