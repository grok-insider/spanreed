import assert from "node:assert/strict";
import {mkdtemp, mkdir, readFile, writeFile, rm, chmod, access} from "node:fs/promises";
import {createWriteStream} from "node:fs";
import {tmpdir} from "node:os";
import {resolve, join} from "node:path";
import {fileURLToPath, pathToFileURL} from "node:url";
import {spawn} from "node:child_process";
import {randomBytes, createHash} from "node:crypto";
import {mock} from "bun:test";

const [spanreedPath, portablePath, cryptoPath] = process.argv.slice(2).map(value => resolve(value));
if (!spanreedPath || !portablePath || !cryptoPath) throw new Error("Usage: bun scripts/qualify-managed.ts <spanreed> <portable> <crypto-dist>");
const libs = resolve(fileURLToPath(new URL("..", import.meta.url)));
const root = await mkdtemp(join(tmpdir(), "remote-managed-qualification-"));
await chmod(root, 0o700);
const relayRoot = join(root, "relay");
const nativeRoot = join(root, "native");
for (const path of [relayRoot, nativeRoot, join(root, "grok")]) await mkdir(path, {mode: 0o700});
const secret = randomBytes(32).toString("hex");
const tickets = new Map<string, {accountId: string; hostId: string; role: string; generation: number; expires: number}>();
let active = true;
let account = "account-a";
let consumed = 0;
let consumedBrowserTicket = "";
const children: ReturnType<typeof spawn>[] = [];
const logs: ReturnType<typeof createWriteStream>[] = [];
const transports: {dispose?(): void}[] = [];
const report: Record<string, unknown> = {root, scriptSha256: createHash("sha256").update(await readFile(fileURLToPath(import.meta.url))).digest("hex"), realRelay: true, realNativeHost: true, realWasm: true, realPortableConnector: true, simulated: ["account login and ticket store", "browser IndexedDB through an isolated memory store", "test-only loopback transport boundaries"], checks: []};
const frozenRelay = Bun.env.FABRIALS_REMOTE_QA_RELAY_BINARY;
const frozenNative = Bun.env.FABRIALS_REMOTE_QA_NATIVE_BINARY;
assert.equal(Boolean(frozenRelay), Boolean(frozenNative), "Both frozen executables must be supplied together");
if (frozenRelay && frozenNative) report.binaries = await Promise.all([frozenRelay, frozenNative].map(async path => ({path: resolve(path), sha256: createHash("sha256").update(await readFile(path)).digest("hex")})));
const passed = (message: string) => { (report.checks as string[]).push(message); console.log(`PASS: ${message}`); };
function issue(role: string, owner = "account-a", expires = Date.now() + 30_000): string {
  const ticket = randomBytes(32).toString("hex");
  tickets.set(ticket, {accountId: owner, hostId: "qa-host", role, generation: 1, expires});
  return ticket;
}
const control = Bun.serve({hostname: "127.0.0.1", port: 0, async fetch(request) {
  const path = new URL(request.url).pathname;
  if (request.method !== "POST") return new Response(null, {status: 405});
  const body = await request.json() as Record<string, unknown>;
  if (path === "/test/ticket") {
    if (!active || request.headers.get("x-test-account") !== "account-a" || body.hostId !== "qa-host") return new Response(null, {status: 403});
    return Response.json({ticket: issue(String(body.role)), relayUrl: "wss://remote-relay.fabrials.com/connect"});
  }
  if (request.headers.get("authorization") !== `Bearer ${secret}`) return new Response(null, {status: 403});
  if (path === "/api/spanreed/remote/ready") return Response.json({ready: true});
  if (path === "/api/spanreed/remote/tickets/consume") {
    const ticket = typeof body.ticket === "string" ? tickets.get(body.ticket) : undefined;
    if (typeof body.ticket === "string") tickets.delete(body.ticket);
    if (!ticket || ticket.expires <= Date.now()) return new Response(null, {status: 403});
    consumed++;
    if (ticket.role === "browser") consumedBrowserTicket = String(body.ticket);
    const {expires, ...identity} = ticket;
    return Response.json(identity);
  }
  if (path === "/api/spanreed/remote/tickets/introspect") return Response.json({active: active && body.hostId === "qa-host" && body.generation === 1});
  return new Response(null, {status: 404});
}});
function child(name: string, cwd: string, args: string[], env: Record<string, string>) {
  const log = createWriteStream(join(root, `${name}.log`), {mode: 0o600}); logs.push(log);
  const environment = {...Bun.env, GROK_HOME: join(root, "grok"), ...env};
  for (const key of ["XAI_API_KEY", "GROK_API_KEY", "FABRIALS_LOCAL_DEV_KEY", "AI_RELAY_VIRTUAL_KEY", "GROK_CODE_XAI_API_KEY"]) delete environment[key];
  const frozen = name === "relay" ? frozenRelay : frozenNative;
  const selected = frozen ? name === "relay" ? ["tests::managed_relay_qualification_fixture", "--exact", "--nocapture"] : ["network_fixture::managed_network_fixture", "--ignored", "--exact", "--nocapture"] : args;
  const process = spawn(frozen ?? "cargo", selected, {cwd, env: environment, stdio: ["ignore", "pipe", "pipe"], detached: Bun.env.OS !== "Windows_NT"});
  process.stdout!.pipe(log, {end: false}); process.stderr!.pipe(log, {end: false});
  children.push(process); return process;
}
async function until(predicate: () => boolean | Promise<boolean>, label: string, timeout = 20_000) {
  const deadline = Date.now() + timeout;
  while (!(await predicate())) { if (Date.now() > deadline) throw new Error(`Timed out: ${label}`); await Bun.sleep(20); }
}
async function exists(path: string) { try {await access(path); return true;} catch {return false;} }
async function stop(process: ReturnType<typeof spawn>, directory: string) {
  await writeFile(join(directory, "stop"), "stop", {mode: 0o600});
  await until(() => process.exitCode !== null || process.signalCode !== null, "fixture shutdown");
  assert.equal(process.exitCode, 0, "fixture passed");
}
const nativeArgs = ["test", "-p", "fabrials-remote-host", "--lib", "--locked", "network_fixture::managed_network_fixture", "--", "--ignored", "--exact", "--nocapture"];
let nativeCount = 0;
async function startNative(relayUrl: string) {
  await rm(join(nativeRoot, "stop"), {force: true});
  await rm(join(nativeRoot, "host-info.json"), {force: true});
  await rm(join(nativeRoot, "remote-status.json"), {force: true});
  await writeFile(join(nativeRoot, "host-input.json"), JSON.stringify({relayUrl, ticket: issue("host")}), {mode: 0o600});
  const native = child(`native-${++nativeCount}`, libs, nativeArgs, {FABRIALS_REMOTE_HOST_QA: nativeRoot});
  await until(() => exists(join(nativeRoot, "host-info.json")), "native identity");
  await until(async () => {
    try {return JSON.parse(await readFile(join(nativeRoot, "remote-status.json"), "utf8")).connection === "connected";} catch {return false;}
  }, "native relay connection");
  return {process: native, info: JSON.parse(await readFile(join(nativeRoot, "host-info.json"), "utf8"))};
}
const originalFetch = globalThis.fetch;
const OriginalWebSocket = globalThis.WebSocket;
let relayUrl = "";
try {
  const relay = child("relay", spanreedPath, ["test", "--manifest-path", "crates/spanreed-remote-relay/Cargo.toml", "--locked", "tests::managed_relay_qualification_fixture", "--", "--exact", "--nocapture"], {FABRIALS_REMOTE_RELAY_QA: relayRoot, FABRIALS_REMOTE_RELAY_QA_CONTROL_URL: control.url.origin, FABRIALS_REMOTE_RELAY_QA_SECRET: secret});
  await until(() => exists(join(relayRoot, "relay-ready.json")), "relay ready", 60_000);
  relayUrl = JSON.parse(await readFile(join(relayRoot, "relay-ready.json"), "utf8")).url;
  let native = await startNative(relayUrl);
  const wasm = await import(pathToFileURL(join(cryptoPath, "fabrials_remote_noise.js")).href);
  await wasm.default({module_or_path: await readFile(join(cryptoPath, "fabrials_remote_noise_bg.wasm"))});
  const grants = new Map<string, any>();
  const serviceDirectory = join(portablePath, "apps/web/src/services");
  mock.module(join(serviceDirectory, "remoteCrypto.ts"), () => ({loadNoiseModule: async () => wasm}));
  mock.module(join(serviceDirectory, "remoteDevice.ts"), () => ({
    readGrant: async (id: string) => structuredClone(grants.get(id)),
    writeGrant: async (id: string, grant: unknown) => {grants.set(id, structuredClone(grant));},
    deleteGrant: async (id: string) => {grants.delete(id);},
  }));
  globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
    assert.equal(String(input), "https://fabrials.com/api/spanreed/remote/tickets", "only test ticket HTTP may be intercepted");
    const headers = new Headers(init?.headers); headers.set("x-test-account", account);
    return originalFetch(`${control.url.origin}/test/ticket`, {...init, headers});
  }) as typeof fetch;
  globalThis.WebSocket = class extends OriginalWebSocket {
    constructor(url: string | URL) {assert.equal(String(url), "wss://remote-relay.fabrials.com/connect"); super(relayUrl);}
  } as typeof WebSocket;
  const {createRemoteTransport} = await import(pathToFileURL(join(serviceDirectory, "remoteConnector.ts")).href);
  async function connected(pairing?: {secret: string; hostPublicKey: string}) {
    const transport = await createRemoteTransport({hostId: "qa-host", pairing}); transports.push(transport);
    const observed: any[] = []; let closed = false; let failure: unknown;
    const events = transport.openEvents({onEvent: (event: unknown) => observed.push(event), onOpen() {}, onClose() {closed = true;}, onFailure(value: unknown) {failure = value;}}, 0);
    await until(() => observed.some(value => value.event.kind === "hostStatus"), "native controller status");
    const status = observed.find(value => value.event.kind === "hostStatus").event.state as string;
    const epoch = status.startsWith("controlling:") ? Number(status.slice(12)) : 0;
    const heartbeat = setInterval(() => events.send(JSON.stringify({kind: "controllerHeartbeat"})), 5000);
    const dispose = transport.dispose.bind(transport); transport.dispose = () => {clearInterval(heartbeat); dispose();};
    return {transport, epoch, observed, pulse: () => events.send(JSON.stringify({kind: "controllerHeartbeat"})), closed: () => closed, failure: () => failure};
  }
  async function command(peer: Awaited<ReturnType<typeof connected>>, kind: string, epoch = peer.epoch, fields: Record<string, unknown> = {}) {
    const id = crypto.randomUUID();
    return peer.transport.request("/command", {method: "POST", body: JSON.stringify({protocolVersion: 2, requestId: id, idempotencyKey: id, controllerEpoch: epoch, operation: {kind, ...fields}})});
  }
  let owner = await connected({secret: native.info.nonce, hostPublicKey: native.info.hostPublicKey});
  assert.ok(owner.epoch > 0);
  const bootstrap = await command(owner, "bootstrap"); assert.equal(bootstrap.status, 202); assert.equal((await bootstrap.json()).result.outcome, "workspaces");
  passed("Portable connector pairs through real relay, WASM and native host and dispatches bootstrap");
  const observer = await connected(); assert.equal(observer.epoch, 0);
  assert.equal((await command(observer, "openWorkspacePicker", owner.epoch)).status, 409);
  assert.equal((await command(observer, "listWorkspaces")).status, 202);
  assert.equal((await command(owner, "openWorkspacePicker")).status, 202);
  passed("same-grant observer can read but cannot borrow owner epoch for mutations");
  observer.transport.dispose(); owner.transport.dispose(); await Bun.sleep(100);
  owner = await connected(); assert.ok(owner.epoch > 1);
  assert.equal((await command(owner, "openWorkspacePicker", owner.epoch - 1)).status, 409);
  passed("browser reconnect resumes persisted grant and invalidates stale epoch");
  const originalGrant = structuredClone(grants.get("qa-host"));
  grants.set("qa-host", {...originalGrant, privateKey: wasm.generate_private_key()});
  await assert.rejects(createRemoteTransport({hostId: "qa-host"}), /revoked/);
  grants.set("qa-host", originalGrant);
  passed("resumed session is bound to its original Noise device key");
  account = "account-b";
  await assert.rejects(createRemoteTransport({hostId: "qa-host"}), /revoked/);
  account = "account-a";
  async function rejectedTicket(ticket: string) {
    const socket = new OriginalWebSocket(relayUrl); let closed = false; let routed = false;
    socket.onopen = () => socket.send(JSON.stringify({type: "authenticate", ticket}));
    socket.onmessage = event => {if (JSON.parse(String(event.data)).type === "ready") routed = true;};
    socket.onclose = () => {closed = true;};
    await until(() => closed, "relay rejects ticket"); assert.equal(routed, false);
  }
  const spentBrowserTicket = consumedBrowserTicket;
  await rejectedTicket(issue("browser", "account-b"));
  await rejectedTicket(issue("browser", "account-a", Date.now() - 1));
  assert.ok(spentBrowserTicket);
  await rejectedTicket(spentBrowserTicket);
  await rejectedTicket("unknown-ticket");
  passed("foreign account, expired ticket and consumed/unknown ticket cannot route to this host");
  owner.transport.dispose();
  await stop(native.process, nativeRoot);
  native = await startNative(relayUrl);
  owner = await connected();
  assert.equal((await command(owner, "listWorkspaces")).status, 202);
  assert.equal(grants.get("qa-host").hostPublicKey, originalGrant.hostPublicKey);
  assert.equal(grants.get("qa-host").pair.sessionId, originalGrant.pair.sessionId);
  passed("native host restart preserves identity, device binding and browser grant");
  const revocationHeartbeat = setInterval(() => owner.pulse(), 1);
  try {
    assert.equal((await command(owner, "forgetBrowserPairing")).status, 202);
    await until(() => owner.closed(), "revoked channel closes");
  } finally {clearInterval(revocationHeartbeat);}
  assert.equal((owner.failure() as {kind?: string})?.kind, "not_paired");
  await assert.rejects(createRemoteTransport({hostId: "qa-host"}), /revoked/);
  passed("revocation acknowledges, closes the active encrypted channel and rejects resume");
  owner.transport.dispose();
  await stop(native.process, nativeRoot);
  await stop(relay, relayRoot);
  report.result = "passed"; report.consumedTickets = consumed;
} catch (error) {
  report.result = "failed"; report.error = error instanceof Error ? error.message : String(error);
  throw error;
} finally {
  globalThis.fetch = originalFetch; globalThis.WebSocket = OriginalWebSocket;
  for (const transport of transports) transport.dispose?.();
  for (const child of children) if (child.exitCode === null && child.signalCode === null) {
    if (Bun.env.OS === "Windows_NT") child.kill("SIGTERM");
    else try {globalThis.process.kill(-child.pid!, "SIGTERM");} catch {}
  }
  await Promise.all(children.map(async process => {
    try {await until(() => process.exitCode !== null || process.signalCode !== null, "owned process shutdown", 5000);} catch {
      if (Bun.env.OS === "Windows_NT") process.kill("SIGKILL");
      else try {globalThis.process.kill(-process.pid!, "SIGKILL");} catch {}
    }
  }));
  control.stop(true);
  await Promise.all([relayRoot, nativeRoot, join(root, "grok")].map(path => rm(path, {recursive: true, force: true})));
  for (const log of logs) log.end();
  await writeFile(join(root, "report.json"), JSON.stringify(report, null, 2) + "\n", {mode: 0o600});
  console.log(`Evidence: ${join(root, "report.json")}`);
}
