#![allow(missing_docs)]

use fabrials_agent_host::HostIdentity;
use fabrials_agent_host::cli::{CliDefaults, run};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

#[test]
fn command_in_isolated_process() {
    let Ok(command) = std::env::var("FABRIALS_HELP_TEST_ARGS") else {
        return;
    };
    let args: Vec<String> = serde_json::from_str(&command).unwrap();
    assert_eq!(
        run(
            &args,
            CliDefaults::new(HostIdentity::new("test-host", "0"), "test-host agent")
        ),
        ExitCode::SUCCESS
    );
}

#[test]
fn subcommand_help_exits_without_creating_state_or_starting_an_agent() {
    for args in [
        vec!["serve", "--help"],
        vec!["serve", "-h"],
        vec!["open", "--help"],
        vec!["status", "--help"],
        vec!["doctor", "--help"],
        vec!["stop", "--help"],
        vec!["repair", "--help"],
        vec!["workspace", "add", "--help"],
    ] {
        let directory = tempfile::tempdir().unwrap();
        let state = directory.path().join("state");
        let grok = directory.path().join("grok");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "command_in_isolated_process"])
            .env(
                "FABRIALS_HELP_TEST_ARGS",
                serde_json::to_string(&args).unwrap(),
            )
            .env("FABRIALS_AGENT_STATE_DIR", &state)
            .env("GROK_HOME", &grok)
            .env("FABRIALS_AGENT_PROGRAM", directory.path().join("no-agent"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() > Duration::from_secs(3) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("help must exit promptly: {args:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success(), "help failed: {args:?}");
        assert!(!state.exists(), "help created host state: {args:?}");
        assert!(!grok.exists(), "help started the CLI: {args:?}");
    }
}
