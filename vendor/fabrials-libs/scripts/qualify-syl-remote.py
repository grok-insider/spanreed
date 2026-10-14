import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import threading
import time


ALIASES = ["XAI_API_KEY", "GROK_CODE_XAI_API_KEY", "AI_RELAY_VIRTUAL_KEY", "FABRIALS_LOCAL_DEV_KEY", "GROK_API_KEY"]
ALLOWED = {"status", "browser_open", "browser_snapshot", "browser_click", "browser_close"}


def proxy(output, url, original_home):
    environment = dict(os.environ, HOME=original_home, SYL_MCP_PROFILE="browser", SYL_MCP_PANEL="0", SYL_BROWSER_ALLOWED_HOSTS="127.0.0.1")
    for name in ALIASES + ["SYL_MCP_ALLOW_REAL", "SYL_MCP_ALLOW_VM", "SYL_MCP_ALLOW_GUEST_EXEC", "SYL_BROWSER_CDP", "SYL_BROWSER_SHARE_AUTH", "SYL_BROWSER_HEADED", "XDG_CONFIG_HOME"]:
        environment.pop(name, None)
    output = Path(output)
    log = (output / "syl-stderr.log").open("a")
    child = subprocess.Popen(["syl", "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True, bufsize=1, env=environment, start_new_session=True)
    selector = selectors.DefaultSelector()
    selector.register(sys.stdin, selectors.EVENT_READ, "input")
    selector.register(child.stdout, selectors.EVENT_READ, "child")
    pending = {}
    buffers = {"input":b"", "child":b""}
    closed = False

    def record(value):
        with (output / "mcp-trace.jsonl").open("a") as target:
            target.write(json.dumps(value) + "\n")

    def send(value):
        child.stdin.write(json.dumps(value) + "\n")
        child.stdin.flush()

    try:
        while child.poll() is None:
            for key, _ in selector.select(.2):
                chunk = os.read(key.fileobj.fileno(), 65_536)
                if not chunk:
                    return
                buffers[key.data] += chunk
                while b"\n" in buffers[key.data]:
                    line, buffers[key.data] = buffers[key.data].split(b"\n", 1)
                    value = json.loads(line)
                    if key.data == "input":
                        if value.get("method") == "tools/call":
                            params = value.get("params", {})
                            name = params.get("name")
                            arguments = params.get("arguments", {})
                            allowed = name in ALLOWED and (name != "browser_open" or arguments == {"url":url})
                            if not allowed:
                                record({"direction":"refused","name":name})
                                print(json.dumps({"jsonrpc":"2.0","id":value["id"],"error":{"code":-32602,"message":"qualification permits only the synthetic browser fixture"}}), flush=True)
                                continue
                            pending[str(value["id"])] = {"name":name,"arguments":arguments,"started":time.monotonic()}
                            record({"direction":"request","id":value["id"],"name":name,"arguments":arguments})
                        send(value)
                    else:
                        request = pending.pop(str(value.get("id")), None)
                        if request is not None:
                            closed = request["name"] == "browser_close" and not value.get("result", {}).get("isError")
                            record({"direction":"response","id":value["id"],"name":request["name"],"arguments":request["arguments"],"result":value.get("result"),"error":value.get("error"),"durationMs":(time.monotonic()-request["started"])*1000})
                        print(json.dumps(value), flush=True)
    finally:
        selector.close()
        if child.poll() is None:
            if not closed:
                try:
                    send({"jsonrpc":"2.0","id":"qualification-cleanup","method":"tools/call","params":{"name":"browser_close","arguments":{}}})
                    time.sleep(.5)
                except (BrokenPipeError, OSError):
                    pass
            child.stdin.close()
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=3)
        log.close()


def run(output, binary, program):
    output = Path(output).resolve()
    output.mkdir(mode=0o700)
    os.chmod(output, 0o700)
    state = {"counter":0,"choice":"blue","effects":[]}
    lock = threading.Lock()

    def save():
        temporary = output / "fixture-state.tmp"
        temporary.write_text(json.dumps(state))
        temporary.replace(output / "fixture-state.json")

    save()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            if self.path != "/":
                self.send_error(404)
                return
            with lock:
                body = ('<!doctype html><title>Syl Remote qualification</title><main><h1>Counter ' + str(state["counter"]) + '</h1><h2>Choice ' + state["choice"] + '</h2><form action="/increment" method="post"><button>Increment</button></form><form action="/green" method="post"><button>Choose green</button></form></main>').encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            if self.path not in ("/increment", "/green"):
                self.send_error(404)
                return
            with lock:
                if self.path == "/increment":
                    state["counter"] += 1
                else:
                    state["choice"] = "green"
                state["effects"].append(self.path)
                save()
            if self.path == "/increment":
                time.sleep(3)
            self.send_response(303)
            self.send_header("Location", "/")
            self.send_header("Content-Length", "0")
            self.end_headers()

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    secret = os.environ.get("FABRIALS_LOCAL_DEV_KEY") or Path("/home/friend/.ai-relay-local-dev").read_text().strip()
    environment = dict(os.environ, FABRIALS_AGENT_PROGRAM=str(Path(program).resolve()), QUALIFY_SYL_OUTPUT=str(output), QUALIFY_SYL_URL=f"http://127.0.0.1:{server.server_port}/", QUALIFY_SYL_PROXY=str(Path(__file__).resolve()), QUALIFY_SYL_HOME=os.environ["HOME"])
    for name in ALIASES:
        environment[name] = secret
    started = time.monotonic()
    child = subprocess.Popen([str(Path(binary).resolve())], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, env=environment, start_new_session=True)
    timed_out = False
    try:
        try:
            content, _ = child.communicate(timeout=300)
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(child.pid, signal.SIGTERM)
            try:
                content, _ = child.communicate(timeout=6)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                content, _ = child.communicate(timeout=3)
        (output / "native.log").write_text(content.replace(secret, "<redacted>"))
    finally:
        server.shutdown()
        server.server_close()
        worker.join(timeout=3)
    trace_file = output / "mcp-trace.jsonl"
    trace = [json.loads(line) for line in trace_file.read_text().splitlines()] if trace_file.exists() else []
    responses = [item for item in trace if item.get("direction") == "response"]
    clicks = [item for item in responses if item["name"] == "browser_click"]
    stale = any(item.get("result", {}).get("isError") is True and "stale" in json.dumps(item.get("result", {})).lower() and any(previous["arguments"] == item["arguments"] and not previous.get("result", {}).get("isError") for previous in clicks[:index]) for index, item in enumerate(clicks))
    report = {"schema":1,"scope":"native-RemoteAccess-official-shared-ACP-real-Syl-MCP-synthetic-browser","exit":child.returncode,"timeout":timed_out,"durationSeconds":time.monotonic()-started,"checks":{"native_acceptance":child.returncode == 0,"exact_two_effects":state["counter"] == 1 and state["choice"] == "green" and state["effects"] == ["/increment","/green"],"actual_stale_repeat_refused":stale,"no_out_of_scope_tools":not any(item.get("direction") == "refused" for item in trace)},"programSha256":hashlib.sha256(Path(program).read_bytes()).hexdigest(),"binarySha256":hashlib.sha256(Path(binary).read_bytes()).hexdigest(),"scriptSha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),"notQualified":["Noise relay and PWA UI","real Android or desktop control","host restart after uncertain effect","readable tool output bodies in Remote"]}
    (output / "result.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report))
    return 0 if all(report["checks"].values()) and not timed_out else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("output")
    parser.add_argument("--proxy", action="store_true")
    parser.add_argument("--url")
    parser.add_argument("--home")
    parser.add_argument("--binary")
    parser.add_argument("--program")
    options = parser.parse_args()
    if options.proxy:
        proxy(options.output, options.url, options.home)
    else:
        sys.exit(run(options.output, options.binary, options.program))
