//! `spanreed agent` runs the absorbed grok-bridge host from the real binary.
#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const INSTALL_ID: &str = "0123456789abcdef0123456789abcdef";

struct Host {
    child: Child,
    root: PathBuf,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn command(program: &Path, root: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .env("XDG_STATE_HOME", root.join("state"))
        .env("GROK_HOME", root.join("grok-home"))
        .env("GROK_BRIDGE_AGENT", root.join("agent.sh"))
        .env_remove("FABRIALS_AGENT_STATE_DIR")
        .env_remove("GROK_BRIDGE_STATE_DIR")
        .env_remove("GROK_LIGHT_STATE_DIR")
        .env_remove("FABRIALS_AGENT_PROGRAM");
    command
}

/// A free port in the range the host allocates from (below the ephemeral range).
fn allocatable_port() -> u16 {
    let seed = std::process::id() as u16;
    (0..2_000u16)
        .map(|offset| 20_000 + (seed.wrapping_add(offset.wrapping_mul(7)) % 12_000))
        .find(|port| TcpListener::bind(("127.0.0.1", *port)).is_ok())
        .expect("free port")
}

fn get(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
}

fn output(mut command: Command) -> String {
    let out = command.output().expect("run spanreed");
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
fn agent_serve_takes_over_grok_bridge_state_and_reports_spanreed() {
    let root = std::env::temp_dir().join(format!("spanreed-agent-host-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let legacy = root.join("state/grok-bridge");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::create_dir_all(root.join("grok-home")).unwrap();
    let port = allocatable_port();
    std::fs::write(
        legacy.join("origin.json"),
        format!("{{\n  \"installId\": \"{INSTALL_ID}\",\n  \"port\": {port}\n}}\n"),
    )
    .unwrap();
    // A minimal ACP agent: answers `initialize` like the Grok Build CLI and
    // acknowledges anything else, so the host reaches its ready state.
    let agent = root.join("agent.sh");
    std::fs::write(
        &agent,
        r#"#!/usr/bin/env python3
import json, sys
if "--version" in sys.argv:
    print("grok 1.0.41 (stub)")
    sys.exit(0)
for line in sys.stdin:
    try:
        message = json.loads(line)
    except ValueError:
        continue
    if "id" not in message or "method" not in message:
        continue
    if message["method"] == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {"loadSession": True},
                  "authMethods": [{"id": "cached_token", "name": "cached_token"}],
                  "_meta": {"agentVersion": "stub-1.0.0"}}
    else:
        result = {}
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}) + "\n")
    sys.stdout.flush()
"#,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();

    let bin = Path::new(env!("CARGO_BIN_EXE_spanreed"));
    let mut serve = command(bin, &root);
    serve
        .args(["agent", "serve"])
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(root.join("serve.out")).unwrap())
        .stderr(std::fs::File::create(root.join("serve.err")).unwrap());
    let host = Host {
        child: serve.spawn().expect("spawn agent serve"),
        root: root.clone(),
    };

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut host = host;
    let health = loop {
        if let Some(body) = get(port, "/healthz") {
            break body;
        }
        if let Some(status) = host.child.try_wait().unwrap() {
            panic!(
                "agent host exited with {status}: {}",
                std::fs::read_to_string(root.join("serve.err")).unwrap_or_default()
            );
        }
        assert!(
            Instant::now() < deadline,
            "agent host never listened on {port}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let health: serde_json::Value = serde_json::from_str(&health).expect("healthz json");
    assert_eq!(health["ok"], true);
    assert_eq!(health["protocolVersion"], 2);
    assert_eq!(health["host"], "spanreed");
    assert_eq!(health["hostVersion"], env!("CARGO_PKG_VERSION"));

    let migrated: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("state/spanreed/agent/origin.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(migrated["installId"], INSTALL_ID);
    assert_eq!(migrated["port"], port);

    let mut status = command(bin, &root);
    status.args(["agent", "status"]);
    let status = output(status);
    assert!(status.starts_with("running"), "{status}");
    assert!(status.contains(&format!(":{port}")), "{status}");

    let links = root.join("bin");
    std::fs::create_dir_all(&links).unwrap();
    std::os::unix::fs::symlink(bin, links.join("grok-bridge")).unwrap();
    let mut legacy_status = command(&links.join("grok-bridge"), &root);
    legacy_status.arg("status");
    let legacy_status = output(legacy_status);
    assert!(legacy_status.starts_with("running"), "{legacy_status}");

    let mut stop = command(bin, &root);
    stop.args(["agent", "stop"]);
    let _ = output(stop);
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "agent host ignored stop");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn agent_service_prints_a_launch_agent_for_this_binary() {
    let bin = env!("CARGO_BIN_EXE_spanreed");
    let out = Command::new(bin)
        .args(["agent", "service", "print"])
        .env("HOME", "/Users/fixture")
        .output()
        .expect("run spanreed");
    assert!(out.status.success());
    let plist = String::from_utf8(out.stdout).unwrap();
    assert!(plist.starts_with("<?xml"));
    assert!(plist.contains("<string>com.fabrials.spanreed.agent</string>"));
    assert!(plist.contains(&format!("<string>{bin}</string>")));
    assert!(plist.contains("<string>/Users/fixture/Library/Logs/spanreed/agent.log</string>"));
    let refused = Command::new(bin)
        .args(["agent", "service", "install"])
        .env("HOME", "/nonexistent-home")
        .output()
        .expect("run spanreed");
    assert!(!refused.status.success(), "install is macOS-only");
}
