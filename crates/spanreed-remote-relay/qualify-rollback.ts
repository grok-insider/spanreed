import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { openSync, closeSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createServer, createConnection, type Socket } from "node:net";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

const baseline = process.env.RELAY_QA_BASELINE_BINARY;
const candidate = process.env.RELAY_QA_CANDIDATE_BINARY;
const root = process.env.RELAY_QA_ROOT;
assert.ok(baseline && candidate && root && resolve(root) === root, "explicit pinned binaries and new absolute evidence directory required");
await mkdir(root, { recursive: false, mode: 0o700 });
const report: any = { startedAt: new Date().toISOString(), status: "running", phases: [], limits: ["Synthetic fixed control plane, manual client reconnect and loopback TCP ingress", "No public TLS, real OAuth, native host persistence, deployed ingress or web database rollback qualification"] };
const secret = randomBytes(32).toString("hex");
const tickets = new Map<string, any>();
const hashes = new Map<string, string>();
for (const file of [baseline, candidate]) hashes.set(file, createHash("sha256").update(await readFile(file)).digest("hex"));
assert.notEqual(hashes.get(baseline), hashes.get(candidate), "rollback uses different executables");
const control = Bun.serve({ hostname: "127.0.0.1", port: 0, async fetch(request) {
  if (request.headers.get("authorization") !== `Bearer ${secret}`) return new Response(null, { status: 401 });
  const path = new URL(request.url).pathname;
  if (path.endsWith("/ready")) return Response.json({ ready: true });
  const input = await request.json() as any;
  if (path.endsWith("/consume")) { const identity = tickets.get(input.ticket); tickets.delete(input.ticket); return identity ? Response.json(identity) : new Response(null, { status: 401 }); }
  if (path.endsWith("/introspect")) return Response.json({ active: input.accountId === "rollback-account" && input.hostId === "rollback-host" && input.generation === 1 });
  return new Response(null, { status: 404 });
} });
let target: URL | undefined;
const sockets = new Set<Socket>();
const ingress = createServer(downstream => {
  if (!target) { downstream.destroy(); return; }
  const upstream = createConnection({ host: target.hostname, port: Number(target.port) });
  for (const socket of [downstream, upstream]) { sockets.add(socket); socket.once("close", () => sockets.delete(socket)); }
  downstream.on("error", () => upstream.destroy());
  upstream.on("error", () => downstream.destroy());
  downstream.pipe(upstream).pipe(downstream);
});
await new Promise<void>(resolveReady => ingress.listen(0, "127.0.0.1", resolveReady));
const address = ingress.address();
assert.ok(address && typeof address === "object");
const publicUrl = `ws://127.0.0.1:${address.port}/connect`;
let child: ReturnType<typeof spawn> | undefined;
let exited: Promise<{ code: number | null; signal: string | null }> | undefined;
let directory = "";
const peers: any[] = [];

async function bounded<T>(work: Promise<T>, milliseconds: number, name: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout>;
  try { return await Promise.race([work, new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error(name)), milliseconds); })]); }
  finally { clearTimeout(timer!); }
}

async function connect(role: "host" | "browser") {
  const identity = { accountId: "rollback-account", hostId: "rollback-host", generation: 1, role };
  const ticket = randomBytes(24).toString("base64url");
  tickets.set(ticket, identity);
  const socket = new WebSocket(publicUrl, role === "browser" ? { headers: { Origin: "https://remote.fabrials.com" } } : undefined);
  const peer: any = { socket, closed: false, channel: null, receive: undefined };
  peers.push(peer);
  let resolveReady: () => void;
  let rejectReady: (error: Error) => void;
  const ready = new Promise<void>((done, reject) => { resolveReady = done; rejectReady = reject; });
  socket.onopen = () => socket.send(JSON.stringify({ type: "authenticate", ticket }));
  socket.onmessage = event => {
    const frame = JSON.parse(String(event.data));
    if (frame.type === "ready") { peer.channel = frame.channelId ?? null; resolveReady(); }
    else if (frame.type === "data" && role === "host") socket.send(JSON.stringify(frame));
    else if (frame.type === "data") peer.receive?.(frame);
  };
  socket.onclose = () => { peer.closed = true; rejectReady(new Error("closed before admission")); };
  socket.onerror = () => rejectReady(new Error("connection failed"));
  await bounded(ready, 7000, "admission timeout");
  return peer;
}

try {
  for (const [name, binary] of [["baseline", baseline], ["candidate", candidate], ["rollback", baseline]]) {
    directory = resolve(root, name!);
    await mkdir(directory, { mode: 0o700 });
    const fd = openSync(resolve(directory, "relay.log"), "w", 0o600);
    child = spawn(binary!, ["tests::managed_relay_qualification_fixture", "--exact", "--nocapture"], { env: { ...process.env, FABRIALS_REMOTE_RELAY_QA: directory, FABRIALS_REMOTE_RELAY_QA_CONTROL_URL: control.url.origin, FABRIALS_REMOTE_RELAY_QA_SECRET: secret }, stdio: ["ignore", fd, fd] });
    closeSync(fd);
    exited = new Promise(resolveExit => child!.once("exit", (code, signal) => resolveExit({ code, signal })));
    for (let attempt = 0; ; attempt++) {
      assert.equal(child.exitCode, null, "fixture remains alive");
      try { target = new URL(JSON.parse(await readFile(resolve(directory, "relay-ready.json"), "utf8")).url); break; } catch {}
      assert.ok(attempt < 200, "fixture listener ready");
      await delay(25);
    }
    const host = await connect("host");
    const browser = await connect("browser");
    for (let index = 0; index < 20; index++) {
      const payload = Buffer.from(`synthetic-${name}-${index}`).toString("base64");
      const received = new Promise<any>(resolveFrame => { browser.receive = resolveFrame; });
      browser.socket.send(JSON.stringify({ type: "data", channelId: browser.channel, payload }));
      const frame = await bounded(received, 5000, "echo timeout");
      assert.ok(frame.channelId === browser.channel && frame.payload === payload, "exact unchanged protocol echo");
    }
    const stoppedAt = performance.now();
    await writeFile(resolve(directory, "stop"), "stop\n");
    const exit = await bounded(exited, 10_000, "process drain exceeds ten seconds");
    await delay(50);
    assert.ok(exit.code === 0 && exit.signal === null && host.closed && browser.closed, "phase drains its active clients");
    assert.equal(createHash("sha256").update(await readFile(binary!)).digest("hex"), hashes.get(binary!), "pinned executable unchanged");
    report.phases.push({ name, binarySha256: hashes.get(binary!), echoes: 20, drainMs: performance.now() - stoppedAt, activeClientsClosed: true, exit });
    child = undefined;
    exited = undefined;
    target = undefined;
  }
  report.status = "passed";
} catch (error) { report.status = "failed"; report.failure = { name: (error as Error).name, message: (error as Error).message }; process.exitCode = 1; }
finally {
  for (const peer of peers) peer.socket.close();
  if (child?.exitCode === null && exited) {
    await writeFile(resolve(directory, "stop"), "stop\n");
    try { await bounded(exited, 12_000, "cleanup timeout"); } catch { child.kill("SIGKILL"); await exited; report.forcedKill = true; }
  }
  for (const socket of sockets) socket.destroy();
  await new Promise<void>(resolveClosed => ingress.close(() => resolveClosed()));
  control.stop(true);
  report.cleanup = { listenerClosed: true, tcpSockets: sockets.size, pendingTickets: tickets.size };
  report.finishedAt = new Date().toISOString();
  await writeFile(resolve(root, "report.json"), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report));
}
