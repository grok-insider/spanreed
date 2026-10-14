#!/usr/bin/env python3
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--web-origin", choices=["https://desktop.grok.me", "https://grok.fabrials.com"], default="https://desktop.grok.me")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    digest = hashlib.sha256()
    with binary.open("rb") as source:
        for part in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(part)
    report = {"schema": 1, "scope": "isolated-linux-direct-cli-lifecycle", "webOrigin": args.web_origin, "binarySha256": digest.hexdigest(), "checks": [], "notQualified": ["Windows", "service manager installation", "TLS/Tailscale", "browser pairing", "Grok inference", "release archive update and rollback"]}
    process = None

    def check(name, condition):
        if not condition:
            raise RuntimeError(name)
        report["checks"].append(name)

    with tempfile.TemporaryDirectory(prefix="spanreed-direct-lifecycle-") as temporary:
        root = Path(temporary)
        state = root / "state"
        grok = root / "grok"
        grok.mkdir(mode=0o700)
        env = {**os.environ, "FABRIALS_AGENT_STATE_DIR": str(state), "GROK_HOME": str(grok), "FABRIALS_AGENT_PROGRAM": "/run/current-system/sw/bin/false"}
        origin = "https://synthetic-direct.test"
        web_args = ["--web-origin", args.web_origin]
        other_web = "https://grok.fabrials.com" if args.web_origin == "https://desktop.grok.me" else "https://desktop.grok.me"

        def run(*commands, environment=env):
            return subprocess.run([str(binary), "remote", *commands], env=environment, capture_output=True, text=True, timeout=10)

        def start():
            child = subprocess.Popen([str(binary), "remote", "private-serve", origin, *web_args], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
            deadline = time.monotonic() + 15
            try:
                while time.monotonic() < deadline:
                    if child.poll() is not None:
                        raise RuntimeError("host terminated before becoming ready")
                    status = run("status")
                    if status.returncode == 0:
                        try:
                            value = json.loads(status.stdout)
                            if value.get("agentRunning"):
                                return child, value
                        except json.JSONDecodeError:
                            pass
                    time.sleep(0.1)
                raise RuntimeError("host readiness deadline")
            except BaseException:
                if child.poll() is None:
                    os.killpg(child.pid, signal.SIGTERM)
                    child.wait(timeout=5)
                raise

        try:
            invalid = run("private-serve", "http://synthetic-direct.test")
            check("invalid TLS origin fails before creating state", invalid.returncode != 0 and not state.exists())
            long_state = root / ("x" * 120)
            invalid_socket = run("private-serve", origin, environment={**env, "FABRIALS_AGENT_STATE_DIR": str(long_state)})
            check("overlong control socket fails before creating state", invalid_socket.returncode != 0 and not long_state.exists())
            service_env = {key: value for key, value in env.items() if key not in {"FABRIALS_AGENT_STATE_DIR", "GROK_BRIDGE_STATE_DIR", "GROK_LIGHT_STATE_DIR"}}
            printed = run("service", "print", "--private-origin", origin, *web_args, environment=service_env)
            check("private service preview uses the direct command and selected web origin without installation", printed.returncode == 0 and "private-serve" in printed.stdout and origin in printed.stdout and args.web_origin in printed.stdout and not state.exists())
            process, first = start()
            check("host starts with the configured direct endpoint and web origin", first.get("transport") == "direct" and first.get("configured") is True and first.get("endpoint") == origin and first.get("webOrigin") == args.web_origin)
            check("state directory is private", stat.S_IMODE(state.stat().st_mode) == 0o700)
            pairing = run("private-pair", origin, *web_args)
            match = re.search(re.escape(args.web_origin) + r"/#directPair=([^\s]+)", pairing.stdout)
            check("pairing uses the selected web origin and a fragment", pairing.returncode == 0 and bool(match))
            value = json.loads(base64.urlsafe_b64decode(match.group(1) + "=" * (-len(match.group(1)) % 4)))
            check("pairing binds the same endpoint installation and host key", value["endpoint"] == origin and value["installationId"] == first["installationId"] and value["hostPublicKey"] == first["publicKey"] and bool(value["nonce"]))
            check("pairing rejects a different endpoint", run("private-pair", "https://another-synthetic.test", *web_args).returncode != 0)
            check("pairing rejects a different web origin", run("private-pair", origin, "--web-origin", other_web).returncode != 0)
            stopped = run("private-stop")
            check("private stop exits the foreground host cleanly", stopped.returncode == 0 and process.wait(timeout=10) == 0)
            process = None
            status = run("status")
            check("stopped status preserves configuration without claiming a live host", status.returncode == 0 and json.loads(status.stdout)["agentRunning"] is False)
            process, second = start()
            check("restart preserves endpoint web origin installation and pinned host key", all(first[name] == second[name] for name in ["endpoint", "webOrigin", "installationId", "publicKey"]))
            check("competing foreground host is rejected", run("private-serve", origin, *web_args).returncode != 0)
            check("second stop completes", run("private-stop").returncode == 0 and process.wait(timeout=10) == 0)
            process = None
            check("endpoint changes require explicit migration", run("private-serve", "https://another-synthetic.test", *web_args).returncode != 0)
            check("web origin changes require explicit migration", run("private-serve", origin, "--web-origin", other_web).returncode != 0)
            check("private state survives failed endpoint migration", json.loads(run("status").stdout)["publicKey"] == first["publicKey"])
            check("no Grok session or credential file was created", not list(grok.iterdir()))
            report["passed"] = True
        finally:
            if process and process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=10)
            report["ownedChildStopped"] = process is None or process.poll() is not None
    report["temporaryStateRemoved"] = not Path(temporary).exists()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"passed": report["passed"], "checks": len(report["checks"]), "receipt": str(args.output)}))


if __name__ == "__main__":
    main()
