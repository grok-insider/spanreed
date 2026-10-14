import assert from "node:assert/strict";
import {spawn, execFileSync, type ChildProcess} from "node:child_process";
import {createHash} from "node:crypto";
import {mkdir, readFile, writeFile, rm, readdir} from "node:fs/promises";
import {closeSync, openSync, realpathSync} from "node:fs";
import {createInterface} from "node:readline";
import {resolve, join} from "node:path";

const [outputArgument, portableArgument, artifactArgument] = process.argv.slice(2);
assert.ok(outputArgument && portableArgument && artifactArgument, "Usage: bun scripts/qualify-direct-vision.ts <new-private-output> <portable-checkout> <frozen-vercel-output>");
const directory = resolve(outputArgument), portable = resolve(portableArgument), artifact = resolve(artifactArgument);
const binary = process.env.DIRECT_VISION_SPANREED ?? "/tmp/fabrials-remote-integration/target/debug/spanreed";
const program = process.env.DIRECT_VISION_GROK ?? "/nix/store/h9ds4ma518gvp6qjhihncs3sjs4b3lpx-grok-build-unwrapped-1.0.52/libexec/grok/grok";
const chrome = process.env.REMOTE_QA_CHROME;
assert.ok(chrome?.startsWith("/nix/store/") && chrome.includes("google-chrome-"), "An unwrapped store Chrome binary is required");
assert.ok(program.startsWith("/nix/store/") && process.env.FABRIALS_LOCAL_DEV_KEY, "Official binary and local-dev key required");
const key = process.env.FABRIALS_LOCAL_DEV_KEY!;
const python = realpathSync("/run/current-system/sw/bin/python3");
await mkdir(directory, {mode: 0o700});
const native = join(directory, "native"), state = join(native, "state"), grok = join(native, "grok"), project = join(native, "vision-project");
for (const path of [native, grok, project, join(native, "home")]) await mkdir(path, {mode: 0o700});
const report: any = {schema: 1, startedAt: new Date().toISOString(), scope: "pwa-noise-spanreed-official-acp-provider-history", model: "grok-4.7-build", checks: [], errors: [], prompts: 0, privateProfileMounted: false, credentialRole: "local-dev", providerEndpoint: "https://ai.fabrials.com/v1", notQualified: ["Production TLS trust: isolated laboratory certificate", "Tailscale", "Real Android/Windows/Apple", "Long-running battery, suspend and load", "Publications and push"]};
const processes: ChildProcess[] = [];
let browser: any, page: any, server: any, syl: ChildProcess | undefined;
let nativePort = 0;
const upstreams = new Set<WebSocket>();
const check = (name: string, value: unknown) => {assert.ok(value, name); report.checks.push(name);};
const hash = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex");
const sanitized = (value: string) => value.replaceAll(key, "[redacted]");
const inspectHistory = async () => {
  const rows: any[] = [];
  const scan = async (base: string) => {for (const entry of await readdir(base, {withFileTypes:true}).catch(()=>[])) {
    const path = join(base,entry.name);
    if(entry.isDirectory()) await scan(path);
    else if(entry.name === "updates.jsonl") {
      const bytes=await readFile(path);assert.ok(bytes.length < 4 * 1024 * 1024,"synthetic transcript size");
      const updates=bytes.toString().trim().split("\n").filter(Boolean).map(line=>JSON.parse(line));
      await writeFile(join(directory,`synthetic-updates-${rows.length}.jsonl`),bytes,{mode:0o600});
      rows.push({sha256:hash(bytes),updates:updates.map(value=>{const update=value.params?.update??value.update??{};return {keys:Object.keys(value),kind:update.sessionUpdate,contentType:update.content?.type,contentKeys:Object.keys(update.content??{}),imageBytes:update.content?.data?.length,imageSha256:update.content?.type==="image"?hash(Buffer.from(update.content.data,"base64")):undefined,hidden:update._meta?.hideFromScrollback};})});
    }
  }};
  await scan(join(grok,"sessions"));report.historyEvidence=rows;
};
const waitFor = async (ready: () => Promise<boolean>, timeout = 20000) => {const stop = Date.now() + timeout; while (!await ready()) {assert.ok(Date.now() < stop, "qualification readiness deadline"); await Bun.sleep(100);}};
const nativeEnv: Record<string, string> = {PATH: process.env.PATH ?? "", LANG: "C.UTF-8", HOME: join(native, "home"), USER: "synthetic", GROK_HOME: grok, FABRIALS_AGENT_STATE_DIR: state, FABRIALS_AGENT_PROGRAM: join(directory, "agent-launcher"), FABRIALS_LOCAL_DEV_KEY: key};
const cleanEnv = {...process.env};
for (const name of Object.keys(cleanEnv)) if (/KEY|TOKEN|SECRET|PASSWORD|AUTH|CREDENTIAL/i.test(name)) delete cleanEnv[name];
const start = (name: string, command: string, args: string[], env: Record<string,string | undefined>, cwd?: string) => {
  const fd = openSync(join(directory, `${name}.log`), "wx", 0o600);
  const child = spawn(command, args, {env, cwd, detached: true, stdio: ["ignore", fd, fd]});
  closeSync(fd); processes.push(child); return child;
};
const cli = (...args: string[]) => execFileSync(binary, args, {env: nativeEnv, cwd: project, encoding: "utf8", timeout: 15000, stdio: ["ignore", "pipe", "pipe"]});
let nextRpc = 0;
const pending = new Map<number, {resolve(value: any): void; reject(error: Error): void}>();
const rpc = (method: string, params: object) => new Promise<any>((resolveReply, reject) => {
  const id = ++nextRpc;
  const timer = setTimeout(() => {pending.delete(id); reject(new Error(`Syl ${method} deadline`));}, 15000);
  pending.set(id, {resolve: value => {clearTimeout(timer); resolveReply(value);}, reject: error => {clearTimeout(timer); reject(error);}});
  syl!.stdin!.write(JSON.stringify({jsonrpc: "2.0", id, method, params}) + "\n");
});
try {
  report.spanreedSha256 = hash(await readFile(binary));
  report.grokSha256 = hash(await readFile(program));
  check("official_grok_1_0_52_hash", report.grokSha256 === "1668414b047e1050a96d25fe17dfc3efe63190fbf66d8cc1c3de8633caf50d11");
  const manifest: Array<{path: string; sha256: string}> = [];
  const scan = async (base: string, prefix = "") => {for (const entry of await readdir(base, {withFileTypes: true})) {const path = join(base, entry.name), name = prefix + entry.name; if (entry.isDirectory()) await scan(path, name + "/"); else if (entry.isFile()) manifest.push({path: name, sha256: hash(await readFile(path))});}};
  await scan(join(artifact, "static"));
  report.artifactFiles = manifest.sort((a,b) => a.path.localeCompare(b.path));
  report.artifactManifestSha256 = hash(Buffer.from(JSON.stringify(report.artifactFiles)));
  const deployment = JSON.parse(await readFile(join(artifact, "config.json"), "utf8"));
  const fixture = JSON.parse(execFileSync(python, ["-c", "import importlib.util,json,sys; from pathlib import Path; spec=importlib.util.spec_from_file_location('vision',sys.argv[1]); module=importlib.util.module_from_spec(spec); spec.loader.exec_module(module); png,expected=module.fixture(); Path(sys.argv[2]).write_bytes(png); print(json.dumps({'expected':expected}))", join(portable, "scripts/qualify-acp-vision.py"), join(directory, "fixture.png")], {encoding: "utf8", env: cleanEnv}));
  report.imageSha256 = hash(await readFile(join(directory, "fixture.png")));
  report.expectedDigits = fixture.expected;
  const launcher = `#!${python}\nimport os,sys\nroot=${JSON.stringify(native)}\nprogram=${JSON.stringify(program)}\nargs=sys.argv[1:]\nif args == ['agent','--leader','stdio']: args=['agent','--leader','--model','grok-4.7-build','stdio']\nelif args != ['--version']: raise SystemExit('Unexpected qualification agent argv')\nenv={'PATH':'/nix/var/nix/profiles/default/bin','LANG':'C.UTF-8','HOME':root+'/home','USER':'synthetic','GROK_HOME':root+'/grok','XAI_API_KEY':os.environ['FABRIALS_LOCAL_DEV_KEY'],'GROK_XAI_API_BASE_URL':'https://ai.fabrials.com/v1','GROK_CLI_CHAT_PROXY_BASE_URL':'https://ai.fabrials.com/v1'}\ncommand=['/run/current-system/sw/bin/bwrap','--unshare-user','--unshare-pid','--unshare-ipc','--unshare-uts','--die-with-parent','--new-session','--ro-bind','/nix','/nix','--proc','/proc','--dev','/dev','--tmpfs','/tmp','--dir','/etc','--ro-bind','/etc/resolv.conf','/etc/resolv.conf','--ro-bind','/etc/ssl','/etc/ssl','--bind',root,root,'--chdir',root+'/vision-project',program,*args]\nos.execve(command[0],command,env)\n`;
  await writeFile(nativeEnv.FABRIALS_AGENT_PROGRAM, launcher, {mode: 0o700});
  check("expected_fact_and_image_are_outside_agent_mount", !join(directory, "fixture.png").startsWith(native + "/") && !launcher.includes(fixture.expected));
  const openssl = process.env.REMOTE_QA_OPENSSL ?? "/nix/store/44gxi8li8y8wrxrb93sya9hljkpigsqw-openssl-3.6.4-bin/bin/openssl";
  execFileSync(openssl, ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1", "-keyout", join(directory, "key.pem"), "-out", join(directory, "cert.pem"), "-subj", "/CN=desktop.grok.me", "-addext", "subjectAltName=DNS:desktop.grok.me,DNS:vision-fixture.test"], {stdio: "ignore", env: cleanEnv});
  cli("agent", "workspace", "add", project);
  start("native", binary, ["remote", "private-serve", "https://vision-fixture.test"], nativeEnv, project);
  await waitFor(async () => {const text = await readFile(join(directory, "native.log"), "utf8"); nativePort = Number(text.match(/127\.0\.0\.1:(\d+)/)?.[1]); return nativePort > 0;});
  cli("remote", "roots", "authorize", project);
  const pairing = cli("remote", "private-pair", "https://vision-fixture.test").match(/https:\/\/desktop\.grok\.me\/#directPair=[^\s]+/)?.[0];
  assert.ok(pairing, "direct pairing produced a fragment URL");
  server = Bun.serve<{upstream?: WebSocket; queued: string[]}>({hostname: "127.0.0.1", port: 0, tls: {key: Bun.file(join(directory,"key.pem")), cert: Bun.file(join(directory,"cert.pem"))}, async fetch(request, serving) {
    const url = new URL(request.url);
    if (url.hostname === "vision-fixture.test" && url.pathname === "/remote") {if (serving.upgrade(request, {data: {queued: []}, headers: {"sec-websocket-protocol": "fabrials.remote.v1"}})) return; return new Response(null,{status:400});}
    if (url.hostname !== "desktop.grok.me" || url.pathname.includes("..")) return new Response(null,{status:404});
    const path = url.pathname === "/" ? "/index.html" : url.pathname;
    const file = Bun.file(join(artifact,"static",path));
    if (!await file.exists()) return new Response(null,{status:404});
    const headers = new Headers({"cache-control":"no-store",...(path.endsWith(".js") ? {"content-type":"text/javascript"} : {})});
    for (const route of deployment.routes ?? []) if (route.src && !route.has && route.headers && new RegExp(`^${route.src}$`).test(url.pathname)) for (const [name,value] of Object.entries(route.headers)) headers.set(name,String(value));
    return new Response(file,{headers});
  }, websocket: {
    open(socket) {const upstream = new WebSocket(`ws://127.0.0.1:${nativePort}/remote`,{headers:{Host:"vision-fixture.test",Origin:"https://desktop.grok.me","Sec-WebSocket-Protocol":"fabrials.remote.v1"}});socket.data.upstream=upstream;upstreams.add(upstream);upstream.onopen=()=>{for(const message of socket.data.queued)upstream.send(message);socket.data.queued=[];};upstream.onmessage=event=>socket.send(event.data);upstream.onclose=()=>{upstreams.delete(upstream);socket.close();};upstream.onerror=()=>socket.close();},
    message(socket,message) {const text=typeof message === "string"?message:message.toString();if(socket.data.upstream?.readyState===1)socket.data.upstream.send(text);else if(socket.data.queued.length<16)socket.data.queued.push(text);else socket.close();},
    close(socket) {socket.data.upstream?.close();},
  }});
  const sylLog = openSync(join(directory,"syl.log"),"wx",0o600);
  syl = spawn("syl",["mcp"],{env:{...cleanEnv,SYL_MCP_PROFILE:"visual",SYL_NESTED_RES:"2560x1440"},stdio:["pipe","pipe",sylLog]});closeSync(sylLog);
  createInterface({input:syl.stdout!}).on("line",line=>{const value=JSON.parse(line),entry=pending.get(value.id);if(entry){pending.delete(value.id);if(value.error)entry.reject(new Error(value.error.message));else entry.resolve(value.result);}});
  await rpc("initialize",{protocolVersion:"2024-11-05",capabilities:{},clientInfo:{name:"direct-provider-vision",version:"1"}});
  const nested = await rpc("tools/call",{name:"gui_open",arguments:{target:"nested",app:"foot"}});
  const display=nested.content?.map((item:any)=>item.text??"").join(" ").match(/display=(wayland-\d+)/)?.[1];assert.ok(display,"real hidden Syl compositor required");
  const profile=join(directory,"chrome-profile");await mkdir(profile,{mode:0o700});
  start("chrome",chrome!,[`--user-data-dir=${profile}`,"--remote-debugging-address=127.0.0.1","--remote-debugging-port=0","--ozone-platform=wayland","--password-store=basic","--no-first-run","--no-default-browser-check","--disable-background-networking","--disable-component-update","--disable-sync","--no-proxy-server","--ignore-certificate-errors",`--host-resolver-rules=MAP desktop.grok.me 127.0.0.1:${server.port}, MAP vision-fixture.test 127.0.0.1:${server.port}`,"--start-fullscreen","--window-size=2560,1440","about:blank"],{...cleanEnv,WAYLAND_DISPLAY:display});
  let cdpPort=0;await waitFor(async()=>{try{cdpPort=Number((await readFile(join(profile,"DevToolsActivePort"),"utf8")).split("\n")[0]);return cdpPort>0;}catch{return false;}});
  const {chromium}=await import(process.env.REMOTE_QA_PLAYWRIGHT??"/tmp/xt-work/qa/node_modules/playwright-core/index.mjs");
  browser=await chromium.connectOverCDP(`http://127.0.0.1:${cdpPort}`);report.browser=browser.version();
  const context=browser.contexts()[0];page=await context.newPage();page.setDefaultTimeout(30000);await page.setViewportSize({width:2560,height:1440});
  page.on("pageerror",(error:Error)=>report.errors.push(sanitized(error.message)));
  const forbiddenRequests:string[]=[];
  await context.route(/https:\/\/(?:[^/]+\.)?fabrials\.com\//,(route:any)=>{forbiddenRequests.push(new URL(route.request().url()).hostname);return route.abort();});
  await page.goto(pairing);await page.getByRole("button",{name:"Pair and connect",exact:true}).click();
  await page.getByRole("button",{name:"Open project vision-project",exact:true}).click();
  await page.getByRole("button",{name:"New session",exact:true}).first().click();
  const input=page.getByRole("textbox",{name:"Message the agent",exact:true});await input.waitFor();
  check("paired_real_noise_and_created_official_acp_session",true);
  await page.locator('input[type="file"][accept*="image"]').setInputFiles(join(directory,"fixture.png"));
  await page.getByRole("button",{name:"Preview fixture.png",exact:true}).waitFor();
  const prompt="Read the eight digits in the attached image: top row left to right, then bottom row left to right. Reply with exactly those eight digits and nothing else. Do not use tools or files.";
  check("prompt_does_not_include_expected_answer",!prompt.includes(fixture.expected));
  await input.fill(prompt);report.prompts++;const started=Date.now();await input.press("Enter");
  await page.getByText(fixture.expected,{exact:true}).waitFor({timeout:120000});report.durationMs=Date.now()-started;
  check("provider_read_random_fact_visible_only_in_image",true);
  await page.getByText("Connected to host. Ready.",{exact:true}).waitFor();
  await page.getByRole("img",{name:"Image 1",exact:true}).waitFor();
  report.liveImages=await page.getByRole("img",{name:"Image 1",exact:true}).count();
  check("typed_image_visible_after_prompt",report.liveImages===1);
  await inspectHistory();
  const updates=report.historyEvidence.flatMap((row:any)=>row.updates);
  check("official_history_persists_typed_image",updates.some((update:any)=>update.kind==="user_message_chunk"&&update.contentType==="image"&&update.imageBytes>0));
  check("official_history_contains_no_tool_calls_or_dropped_images",updates.length>0&&!updates.some((update:any)=>["tool_call","tool_call_update","image_dropped"].includes(update.kind)));
  await page.screenshot({path:join(directory,"answer.png")});
  check("no_permission_or_tool_execution_requested",await page.getByRole("dialog").count()===0 && await page.getByText("image_dropped",{exact:false}).count()===0);
  await page.reload();await page.getByRole("button",{name:"Connect",exact:true}).click();
  await page.getByText(fixture.expected,{exact:true}).waitFor({timeout:30000});
  check("answer_survives_real_host_history_reload",true);
  const images=page.getByRole("img",{name:"Image 1",exact:true});await images.first().waitFor();
  await page.waitForFunction(()=>{const image=document.querySelector<HTMLImageElement>('img[src^="blob:"]');return Boolean(image?.complete&&image.naturalWidth);});
  check("typed_image_survives_history_reload",await images.first().evaluate((image:HTMLImageElement)=>image.complete && image.naturalWidth===512 && image.naturalHeight===320));
  check("private_browser_did_not_call_fabrials",forbiddenRequests.length===0);
  check("no_uncaught_browser_errors",report.errors.length===0);
  report.passed=true;
} catch(error) {
  report.passed=false;report.failure=sanitized(error instanceof Error?error.message:String(error));process.exitCode=1;
  await inspectHistory().catch((error:Error)=>{report.historyEvidenceFailure=sanitized(error.message);});
  if(page){await page.screenshot({path:join(directory,"failure.png"),timeout:5000}).catch((error:Error)=>{report.screenshotFailure=sanitized(error.message);});await writeFile(join(directory,"failure-page.txt"),sanitized(await page.locator("body").innerText({timeout:5000}).catch((error:Error)=>`unavailable: ${error.message}`)),{mode:0o600});}
} finally {
  await browser?.close().catch(()=>{});
  try {cli("remote","private-stop");} catch {}
  for(const child of [...processes].reverse())if(child.exitCode===null&&child.pid){try{process.kill(-child.pid,"SIGTERM");}catch{}}
  for(const socket of upstreams)socket.close();server?.stop(true);
  if(syl){await rpc("tools/call",{name:"gui_close",arguments:{}}).catch(()=>{});syl.kill("SIGTERM");}
  for(const child of processes)if(child.exitCode===null)await Promise.race([new Promise<void>(resolveDone=>child.once("exit",()=>resolveDone())),Bun.sleep(3000)]);
  for(const child of processes)if(child.exitCode===null&&child.pid){try{process.kill(-child.pid,"SIGKILL");}catch{}}
  await rm(native,{recursive:true,force:true});await rm(join(directory,"chrome-profile"),{recursive:true,force:true});await rm(join(directory,"agent-launcher"),{force:true});await rm(join(directory,"key.pem"),{force:true});
  report.temporaryStateRemoved=true;report.finishedAt=new Date().toISOString();report.scriptSha256=hash(await readFile(new URL(import.meta.url)));
  await writeFile(join(directory,"report.json"),JSON.stringify(report,null,2)+"\n",{mode:0o600});
  console.log(JSON.stringify({passed:report.passed,checks:report.checks.length,failure:report.failure,receipt:join(directory,"report.json")}));
}
