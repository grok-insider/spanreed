import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { openSync, closeSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createConnection } from "node:net";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

const binary = process.env.RELAY_QA_BINARY;
const root = process.env.RELAY_QA_ROOT;
assert.ok(binary && root && resolve(root) === root, "explicit test binary and absolute private evidence directory required");
const duration = Number(process.env.RELAY_QA_DURATION_SEC ?? 3600);
assert.ok(Number.isInteger(duration) && duration >= 1 && duration <= 5400, "bounded duration required");
await mkdir(root, { recursive: false, mode: 0o700 });
const report: any = { startedAt: new Date().toISOString(), status: "running", durationSec: duration, limits: ["Synthetic control plane and opaque base64 frames, not real account, HTTPS, Noise or inference qualification", "No multi-replica support", "Short smoke runs cannot qualify the one-hour memory gate"], checks: [], samples: [], control: { consumes: 0, probes: 0, introspections: 0 }, cleanup: {} };
const secret = randomBytes(32).toString("hex");
const tickets = new Map<string, any>();
const revokedAccounts = new Set<string>();
let allowed = true;
let ready = true;
let control: ReturnType<typeof Bun.serve>;
let failure: Error | undefined;
const interrupted = () => { failure = new Error("qualification interrupted"); };
process.once("SIGINT", interrupted);
process.once("SIGTERM", interrupted);
const peers: any[] = [];
const check = (name: string, value: unknown) => { assert.ok(value, name); report.checks.push(name); };
const fetchControl = async (request: Request) => {
  if (request.headers.get("authorization") !== `Bearer ${secret}`) return Response.json({ error: "unauthorized" }, { status: 401 });
  const path = new URL(request.url).pathname;
  if (path.endsWith("/ready")) { report.control.probes++; return Response.json({ ready }, { status: ready ? 200 : 503 }); }
  const input = await request.json() as any;
  if (path.endsWith("/consume")) {
    report.control.consumes++;
    const identity = tickets.get(input.ticket);
    tickets.delete(input.ticket);
    return identity && allowed && !revokedAccounts.has(identity.accountId) ? Response.json(identity) : Response.json({ error: "invalid_ticket" }, { status: 401 });
  }
  if (path.endsWith("/introspect")) { report.control.introspections++; return Response.json({ active: allowed && !revokedAccounts.has(input.accountId) && typeof input.accountId === "string" && input.hostId?.startsWith(`${input.accountId}-`) && input.generation === 1 }); }
  return new Response(null, { status: 404 });
};
control = Bun.serve({ hostname: "127.0.0.1", port: 0, fetch: fetchControl });
const controlPort = control.port;
const log = openSync(resolve(root, "relay.log"), "w", 0o600);
const child = spawn(binary, ["tests::managed_relay_qualification_fixture", "--exact", "--nocapture"], { env: { ...process.env, FABRIALS_REMOTE_RELAY_QA: root, FABRIALS_REMOTE_RELAY_QA_CONTROL_URL: control.url.origin, FABRIALS_REMOTE_RELAY_QA_SECRET: secret, FABRIALS_REMOTE_RELAY_QA_DURATION_SEC: String(duration + 300) }, stdio: ["ignore", log, log] });
closeSync(log);
const exited = new Promise<{ code: number | null; signal: string | null }>(resolveExit => child.once("exit", (code, signal) => resolveExit({ code, signal })));
let url = "";
let base = "";
let sequence = 0;
let delivered = 0;
let sampler: ReturnType<typeof setInterval> | undefined;
let sampling = false;
let slow: ReturnType<typeof createConnection> | undefined;

async function bounded<T>(work: Promise<T>, milliseconds: number, name: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout>;
  try {
    return await Promise.race([work, new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error(name)), milliseconds); })]);
  } finally { clearTimeout(timer!); }
}

async function until(predicate: () => boolean | Promise<boolean>, name: string, timeout = 20_000) {
  const end = performance.now() + timeout;
  while (!await predicate()) {
    if (failure) throw failure;
    assert.equal(child.exitCode, null, "relay process remains alive");
    assert.ok(performance.now() < end, name);
    await delay(25);
  }
}

function ticket(identity: any) {
  const value = randomBytes(24).toString("base64url");
  tickets.set(value, identity);
  return value;
}

async function connect(identity: any, name: string) {
  const websocket = new WebSocket(url, identity.role === "browser" ? { headers: { Origin: "https://remote.fabrials.com" } } : undefined);
  const peer: any = { websocket, identity, name, channel: null, closed: false, received: 0, pending: new Map(), channels: new Set() };
  peers.push(peer);
  let readyResolve: () => void;
  let readyReject: (error: Error) => void;
  const established = new Promise<void>((resolveReady, rejectReady) => { readyResolve = resolveReady; readyReject = rejectReady; });
  websocket.onopen = () => websocket.send(JSON.stringify({ type: "authenticate", ticket: ticket(identity) }));
  websocket.onmessage = event => {
    try {
      const frame = JSON.parse(String(event.data));
      if (frame.type === "ready") { peer.channel = frame.channelId ?? null; readyResolve(); return; }
      if (frame.type === "open") { peer.channels.add(frame.channelId); return; }
      if (frame.type === "closed") { peer.channels.delete(frame.channelId); return; }
      assert.equal(frame.type, "data");
      const content = JSON.parse(Buffer.from(frame.payload, "base64").toString());
      assert.ok(content.accountId === identity.accountId && content.hostId === identity.hostId, "no account or host crossing");
      if (identity.role === "host") {
        assert.ok(peer.channels.has(frame.channelId), "host sees only its open channels");
        websocket.send(JSON.stringify(frame));
      } else {
        assert.ok(content.browser === name && frame.channelId === peer.channel, "no browser channel crossing");
        const resolvePending = peer.pending.get(content.sequence);
        assert.ok(resolvePending, "no duplicate or unsolicited data frame");
        peer.pending.delete(content.sequence);
        peer.received++;
        delivered++;
        resolvePending();
      }
    } catch (error) { failure = error as Error; }
  };
  websocket.onclose = () => { peer.closed = true; readyReject(new Error("closed before admission")); };
  websocket.onerror = () => { peer.closed = true; readyReject(new Error("websocket connection failed")); };
  await bounded(established, 7000, "socket admission timeout");
  return peer;
}

async function fleet(onlyAccount?: string) {
  const hosts = [];
  const browsers = [];
  for (let hostIndex = 0; hostIndex < 10; hostIndex++) {
    const accountId = `account-${hostIndex % 2}`;
    if (onlyAccount && accountId !== onlyAccount) continue;
    const hostId = `${accountId}-host-${hostIndex}`;
    hosts.push(await connect({ accountId, hostId, generation: 1, role: "host" }, `host-${hostIndex}`));
    for (let browser = 0; browser < 10; browser++) browsers.push(await connect({ accountId, hostId, generation: 1, role: "browser" }, `browser-${hostIndex}-${browser}`));
  }
  return { hosts, browsers };
}

async function echo(browsers: any[]) {
  await Promise.all(browsers.map(async peer => {
    const current = sequence++;
    const payload = Buffer.from(JSON.stringify({ accountId: peer.identity.accountId, hostId: peer.identity.hostId, browser: peer.name, sequence: current })).toString("base64");
    const received = new Promise<void>(resolvePending => peer.pending.set(current, resolvePending));
    peer.websocket.send(JSON.stringify({ type: "data", channelId: peer.channel, payload }));
    try { await bounded(received, 5500, "echo acknowledgement timeout"); } finally { peer.pending.delete(current); }
  }));
  if (failure) throw failure;
}

async function stats() {
  const response = await fetch(`${base}/qa/stats`, { signal: AbortSignal.timeout(1500) });
  assert.equal(response.status, 200);
  return response.json() as Promise<any>;
}

function maskedFrame(value: unknown) {
  const payload = Buffer.from(JSON.stringify(value));
  assert.ok(payload.length < 126, "small authentication frame");
  const mask = randomBytes(4);
  const output = Buffer.alloc(6 + payload.length);
  output[0] = 0x81;
  output[1] = 0x80 | payload.length;
  mask.copy(output, 2);
  for (let index = 0; index < payload.length; index++) output[index + 6] = payload[index] ^ mask[index % 4];
  return output;
}

async function rawUpgrade(forwardedFor: string, identity?: any) {
  const target = new URL(url);
  const socket = createConnection({ host: target.hostname, port: Number(target.port) });
  let buffer = Buffer.alloc(0);
  let upgraded = false;
  let done = false;
  const result = new Promise<{ socket: typeof socket; status: number; channel?: string }>((resolveRaw, rejectRaw) => {
    socket.on("error", () => rejectRaw(new Error("raw websocket connection failed")));
    socket.on("close", () => { if (!done) rejectRaw(new Error("raw websocket closed before result")); });
    socket.on("connect", () => socket.write(`GET /connect HTTP/1.1\r\nHost: ${target.host}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: ${randomBytes(16).toString("base64")}\r\nOrigin: https://remote.fabrials.com\r\nX-Forwarded-For: ${forwardedFor}\r\n\r\n`));
    socket.on("data", data => {
      if (done) return;
      buffer = Buffer.concat([buffer, data]);
      if (!upgraded) {
        const end = buffer.indexOf("\r\n\r\n");
        if (end < 0) return;
        const status = Number(/^HTTP\/1\.1 (\d+)/.exec(buffer.subarray(0, end).toString())?.[1]);
        if (status !== 101 || !identity) { done = true; socket.destroy(); resolveRaw({ socket, status }); return; }
        upgraded = true;
        buffer = buffer.subarray(end + 4);
        socket.write(maskedFrame({ type: "authenticate", ticket: ticket(identity) }));
      }
      if (buffer.length < 2) return;
      const length = buffer[1] & 0x7f;
      if (length >= 126 || (buffer[0] & 0x0f) !== 1 || (buffer[1] & 0x80)) { rejectRaw(new Error("unexpected admission frame")); return; }
      if (buffer.length < 2 + length) return;
      try {
        const frame = JSON.parse(buffer.subarray(2, length + 2).toString());
        assert.ok(frame.type === "ready" && typeof frame.channelId === "string", "raw browser ready frame");
        done = true;
        socket.pause();
        resolveRaw({ socket, status: 101, channel: frame.channelId });
      } catch { rejectRaw(new Error("raw browser admission invalid")); }
    });
  });
  try { return await bounded(result, 7000, "raw websocket timeout"); }
  catch (error) { socket.destroy(); throw error; }
}

async function backpressure(host: any, healthyBrowsers: any[]) {
  const raw = await rawUpgrade("203.0.113.1", { ...host.identity, role: "browser" });
  slow = raw.socket;
  await until(() => host.channels.has(raw.channel), "slow browser admitted");
  const started = performance.now();
  const payload = randomBytes(65_535).toString("base64");
  for (let index = 0; index < 400; index++) {
    host.websocket.send(JSON.stringify({ type: "data", channelId: raw.channel, payload }));
    if (index % 10 === 0) await delay(1);
  }
  let sawFullQueue = false;
  await until(async () => {
    const state = await stats();
    assert.ok(state.maxQueuedFrames <= 4, "bounded queue under real TCP backpressure");
    sawFullQueue ||= state.maxQueuedFrames === 4;
    return !host.channels.has(raw.channel);
  }, "slow browser is disconnected independently", 12_000);
  check("real paused TCP receiver fills the bounded queue", sawFullQueue);
  check("slow channel teardown preserves its host", !host.closed);
  await echo(healthyBrowsers);
  report.slowReceiver = { sentFrames: 400, decodedBytesPerFrame: 65_535, closedAfterMs: performance.now() - started };
  slow.destroy();
  slow = undefined;
}

async function quota(host: any) {
  const existing = host.channels.size;
  const extras = [];
  for (let index = existing; index < 64; index++) extras.push(await connect({ ...host.identity, role: "browser" }, `quota-${index}`));
  let refused = false;
  try { await connect({ ...host.identity, role: "browser" }, "quota-denied"); } catch { refused = true; }
  check("sixty-fifth channel is refused without replacing host", refused && !host.closed);
  for (const peer of extras) peer.websocket.close();
  await until(() => host.channels.size === existing, "quota test releases extra channels");
}

async function admissionStorm() {
  const before = report.control.consumes;
  const statuses: number[] = [];
  for (let offset = 0; offset < 1100; offset += 25) {
    const batch = await Promise.all(Array.from({ length: 25 }, (_, index) => rawUpgrade(`198.51.${Math.floor((offset + index) / 250)}.${(offset + index) % 250 + 1}`)));
    statuses.push(...batch.map(result => result.status));
  }
  check("storm returns only bounded admission outcomes", statuses.every(status => status === 101 || status === 429 || status === 503));
  check("forwarded-IP changes do not bypass connection admission", statuses.slice(-100).every(status => status === 429));
  check("unauthenticated storm never consumes a ticket", report.control.consumes === before);
  await until(async () => (await stats()).allocatedSockets === 110, "storm releases unauthenticated permits");
  report.admissionStorm = { requests: statuses.length, upgraded: statuses.filter(status => status === 101).length, rateLimited: statuses.filter(status => status === 429).length, unavailable: statuses.filter(status => status === 503).length };
}

async function reconnectStorm(active: { hosts: any[]; browsers: any[] }) {
  const started = performance.now();
  for (let round = 0; round < 2; round++) {
    for (const browser of active.browsers) browser.websocket.close();
    await until(async () => (await stats()).browsers === 0, "concurrent browser departure settles");
    check(`reconnect round ${round} preserves all ten hosts`, active.hosts.every(peer => !peer.closed));
    active.browsers = await Promise.all(active.browsers.map(peer => connect(peer.identity, peer.name)));
    await echo(active.browsers);
    check(`reconnect round ${round} authenticates all one hundred browsers`, (await stats()).browsers === 100 && active.hosts.every(peer => !peer.closed));
  }
  report.reconnectStorm = { rounds: 2, simultaneousBrowsersPerRound: 100, elapsedMs: performance.now() - started };
}

async function sample() {
  if (sampling) return;
  sampling = true;
  try {
    const text = await readFile(`/proc/${child.pid}/status`, "utf8");
    const row = { atMs: performance.now(), rssKiB: Number(/^VmRSS:\s+(\d+)/m.exec(text)?.[1]), ...await stats() };
    assert.ok(Number.isFinite(row.rssKiB) && row.rssKiB > 0);
    assert.ok(row.queueCapacity === 4 && row.maxQueuedFrames <= 4 && row.allocatedSockets <= 1024);
    report.samples.push(row);
  } catch (error) { failure = error as Error; }
  finally { sampling = false; }
}

try {
  report.binarySha256 = createHash("sha256").update(await readFile(binary)).digest("hex");
  report.sourceHashes = {};
  for (const path of ["qualify.ts", "Cargo.toml", "Cargo.lock", "src/main.rs", "src/routing.rs", "src/control.rs"]) report.sourceHashes[path] = createHash("sha256").update(await readFile(new URL(path, import.meta.url))).digest("hex");
  await until(async () => { try { url = JSON.parse(await readFile(resolve(root, "relay-ready.json"), "utf8")).url; return true; } catch { return false; } }, "relay listener ready");
  base = url.replace(/^ws:/, "http:").replace(/\/connect$/, "");
  await until(async () => (await fetch(`${base}/ready`)).status === 200, "control-plane readiness becomes ready");
  check("liveness while healthy", (await fetch(`${base}/health`)).status === 200);
  ready = false;
  await until(async () => (await fetch(`${base}/ready`)).status === 503, "readiness reflects dependency failure", 9000);
  check("liveness survives dependency failure", (await fetch(`${base}/health`)).status === 200);
  ready = true;
  await until(async () => (await fetch(`${base}/ready`)).status === 200, "readiness recovers", 9000);
  let active = await fleet();
  await echo(active.browsers);
  check("ten hosts and one hundred browsers admitted", (await stats()).browsers === 100 && (await stats()).hosts === 10);
  await quota(active.hosts[0]);
  await backpressure(active.hosts[0], active.browsers);
  revokedAccounts.add("account-0");
  const revoked = [...active.hosts, ...active.browsers].filter(peer => peer.identity.accountId === "account-0");
  await until(() => revoked.every(peer => peer.closed), "account-specific revocation closes only its peers", 22_000);
  const preserved = { hosts: active.hosts.filter(peer => peer.identity.accountId === "account-1"), browsers: active.browsers.filter(peer => peer.identity.accountId === "account-1") };
  await echo(preserved.browsers);
  check("unrelated account remains connected after revocation", preserved.hosts.every(peer => !peer.closed) && preserved.browsers.every(peer => !peer.closed));
  revokedAccounts.clear();
  const restored = await fleet("account-0");
  active = { hosts: [...preserved.hosts, ...restored.hosts], browsers: [...preserved.browsers, ...restored.browsers] };
  allowed = false;
  await until(() => active.hosts.every(peer => peer.closed) && active.browsers.every(peer => peer.closed), "revocation closes all sockets", 22_000);
  check("revocation returns all socket permits", (await stats()).allocatedSockets === 0);
  allowed = true;
  active = await fleet();
  await echo(active.browsers);
  control.stop(true);
  await until(async () => (await fetch(`${base}/ready`)).status === 503, "TCP outage invalidates readiness", 9000);
  await until(() => active.hosts.every(peer => peer.closed) && active.browsers.every(peer => peer.closed), "TCP outage closes authenticated sockets", 22_000);
  control = Bun.serve({ hostname: "127.0.0.1", port: controlPort, fetch: fetchControl });
  await until(async () => (await fetch(`${base}/ready`)).status === 200, "control plane reconnects", 9000);
  active = await fleet();
  await echo(active.browsers);
  await reconnectStorm(active);
  await sample();
  sampler = setInterval(() => { void sample(); }, 1000);
  const warmUntil = performance.now() + (duration >= 1200 ? 60_000 : 1000);
  while (performance.now() < warmUntil) { await echo(active.browsers); await delay(1000); }
  const start = performance.now();
  let loops = 0;
  while (performance.now() - start < duration * 1000) {
    await echo(active.browsers);
    loops++;
    if (loops % 60 === 0) console.log(JSON.stringify({ event: "relay.soak.progress", elapsedSec: (performance.now() - start) / 1000, loops, delivered }));
    await delay(1000);
  }
  const end = performance.now();
  clearInterval(sampler);
  sampler = undefined;
  await until(() => !sampling, "last sample completed");
  report.soak = { startedAtMs: start, finishedAtMs: end, measuredSec: (end - start) / 1000, loops, delivered, sockets: 110 };
  if (duration >= 3600) {
    const first = report.samples.filter((row: any) => row.atMs >= start && row.atMs < start + 600_000);
    const last = report.samples.filter((row: any) => row.atMs >= end - 600_000 && row.atMs <= end);
    const mean = (rows: any[]) => rows.reduce((sum, row) => sum + row.rssKiB, 0) / rows.length;
    report.memory = { firstSamples: first.length, lastSamples: last.length, firstMeanKiB: mean(first), lastMeanKiB: mean(last), growthPercent: (mean(last) / mean(first) - 1) * 100 };
    check("memory coverage has at least 570 samples per ten-minute window", first.length >= 570 && last.length >= 570);
    check("relay RSS growth stays at or below ten percent", report.memory.growthPercent <= 10);
  }
  check("no echo mismatch or duplicate", !failure);
  check("all channels remain live at soak end", (await stats()).browsers === 100 && (await stats()).hosts === 10);
  await admissionStorm();
  await echo(active.browsers);
  check("authenticated fleet survives connection storm", !failure && active.hosts.every(peer => !peer.closed) && active.browsers.every(peer => !peer.closed));
  check("test executable is unchanged during qualification", report.binarySha256 === createHash("sha256").update(await readFile(binary)).digest("hex"));
  for (const [path, digest] of Object.entries(report.sourceHashes)) check(`source unchanged: ${path}`, digest === createHash("sha256").update(await readFile(new URL(path, import.meta.url))).digest("hex"));
  const stoppedAt = performance.now();
  await writeFile(resolve(root, "stop"), "stop\n");
  const exit = await bounded(exited, 10_000, "drain exceeded ten seconds");
  check("owner-controlled drain exits successfully", exit.code === 0 && exit.signal === null);
  await delay(100);
  check("drain closes every authenticated peer", active.hosts.every(peer => peer.closed) && active.browsers.every(peer => peer.closed));
  report.drainMs = performance.now() - stoppedAt;
  report.status = duration >= 3600 ? "passed" : "smoke_passed_not_qualified";
} catch (error) {
  report.status = "failed";
  report.failure = { name: (error as Error).name, message: (error as Error).message };
  process.exitCode = 1;
} finally {
  if (sampler) clearInterval(sampler);
  slow?.destroy();
  for (const peer of peers) peer.websocket.close();
  if (child.exitCode === null) {
    await writeFile(resolve(root, "stop"), "stop\n");
    const clean = await Promise.race([exited.then(() => true), delay(12_000).then(() => false)]);
    if (!clean) { child.kill("SIGKILL"); await exited; report.cleanup.forcedKill = true; }
  }
  control.stop(true);
  report.cleanup.relayExit = await exited;
  report.cleanup.relayExited = report.cleanup.relayExit.code !== null || report.cleanup.relayExit.signal !== null;
  report.cleanup.pendingTickets = tickets.size;
  report.finishedAt = new Date().toISOString();
  await writeFile(resolve(root, "report.json"), JSON.stringify(report, null, 2));
  process.removeListener("SIGINT", interrupted);
  process.removeListener("SIGTERM", interrupted);
  console.log(JSON.stringify({ status: report.status, checks: report.checks.length, failure: report.failure, soak: report.soak, memory: report.memory, cleanup: report.cleanup }));
}
