import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("output")
    parser.add_argument("binary")
    parser.add_argument("--case", action="append", required=True)
    args = parser.parse_args()
    cases = [(version, Path(program).resolve(strict=True)) for version, program in (case.split("=", 1) for case in args.case)]
    root = Path(args.output).resolve()
    root.mkdir(mode=0o700)
    source = Path(__file__).resolve().parent.parent / "crates/fabrials-agent-host/examples/qualify_shared_matrix.rs"
    shutil.copy2(args.binary, root / "qualify_shared_matrix")
    shutil.copy2(source, root / "qualify_shared_matrix.rs")
    shutil.copy2(__file__, root / "owner.py")
    key = os.environ.get("FABRIALS_LOCAL_DEV_KEY") or Path("/home/friend/.ai-relay-local-dev").read_text().strip()
    if not key:
        raise RuntimeError("local-dev credential unavailable")
    receipt = {"schema":1,"scope":"native RemoteAccess and two official shared ACP clients; no TUI, Noise or browser","ownerPid":os.getpid(),"startedUnix":time.time(),"binarySha256":hashlib.sha256((root / "qualify_shared_matrix").read_bytes()).hexdigest(),"sourceSha256":hashlib.sha256(source.read_bytes()).hexdigest(),"credentialRole":"local-dev","credentialAliasesMapped":["XAI_API_KEY","GROK_CODE_XAI_API_KEY","AI_RELAY_VIRTUAL_KEY","FABRIALS_LOCAL_DEV_KEY","GROK_API_KEY"],"runs":[]}
    for expected, path in cases:
        environment = dict(os.environ, FABRIALS_AGENT_PROGRAM=str(path))
        for name in receipt["credentialAliasesMapped"]:
            environment[name] = key
        for name in ["PROJECTS", "CONTROL_TRANSFER", "DEVICE_PANEL"]:
            environment["SPANREED_REMOTE_EXPERIMENT_" + name] = "false"
        environment["SPANREED_REMOTE_EXPERIMENT_PUSH"] = "0"
        run = {"expectedVersion":expected,"program":str(path),"programSha256":hashlib.sha256(path.read_bytes()).hexdigest(),"startedUnix":time.time()}
        receipt["runs"].append(run)
        child = subprocess.Popen([str(root / "qualify_shared_matrix")], env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True, text=True)
        run["pid"] = child.pid
        (root / "status.json").write_text(json.dumps(receipt, indent=2))
        try:
            output, _ = child.communicate(timeout=230)
        except subprocess.TimeoutExpired:
            run["timedOut"] = True
            os.killpg(child.pid, signal.SIGTERM)
            try:
                output, _ = child.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                output, _ = child.communicate(timeout=3)
        run["exitCode"] = child.returncode
        run["finishedUnix"] = time.time()
        clean = output.replace(key, "<redacted>")
        (root / ("grok-" + expected + ".log")).write_text(clean)
        run["passMarker"] = "PASS: official shared ACP matrix" in clean
        run["reportedExpectedVersion"] = "Official Grok " + expected in clean
        (root / "status.json").write_text(json.dumps(receipt, indent=2))
    receipt["finishedUnix"] = time.time()
    (root / "receipt.json").write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt), flush=True)
    return 0 if all(run["exitCode"] == 0 and run["passMarker"] and run["reportedExpectedVersion"] for run in receipt["runs"]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
