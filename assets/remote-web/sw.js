const SHELL = ["index.html","manifest.webmanifest","theme-init.js","icon-192.png","icon-512.png","third-party-notices.txt","crypto/f585e4ce86fe11dca440/fabrials_remote_noise.js","crypto/f585e4ce86fe11dca440/fabrials_remote_noise_bg.wasm","assets/index-jrE2fKnr.js","assets/ibm-plex-mono-cyrillic-400-normal-BSMlKf0J.woff2","assets/ibm-plex-mono-cyrillic-400-normal-CEL4l2ZJ.woff","assets/ibm-plex-mono-cyrillic-500-normal-Ael50iVv.woff","assets/ibm-plex-mono-cyrillic-500-normal-Bq9vWWag.woff2","assets/ibm-plex-mono-cyrillic-600-normal-CTOM6hUh.woff2","assets/ibm-plex-mono-cyrillic-600-normal-fLZuRloM.woff","assets/ibm-plex-mono-cyrillic-ext-400-normal-DMdlQ8Kv.woff","assets/ibm-plex-mono-cyrillic-ext-400-normal-xuaO2J-f.woff2","assets/ibm-plex-mono-cyrillic-ext-500-normal-BIfNGwUT.woff","assets/ibm-plex-mono-cyrillic-ext-500-normal-BqneJy0T.woff2","assets/ibm-plex-mono-cyrillic-ext-600-normal-9HEixskS.woff","assets/ibm-plex-mono-cyrillic-ext-600-normal-V-xxqcpd.woff2","assets/ibm-plex-mono-latin-400-normal-CvHOgSBP.woff","assets/ibm-plex-mono-latin-400-normal-DMJ8VG8y.woff2","assets/ibm-plex-mono-latin-500-normal-CB9ihrfo.woff","assets/ibm-plex-mono-latin-500-normal-DSY6xOcd.woff2","assets/ibm-plex-mono-latin-600-normal-BgSNZQsw.woff2","assets/ibm-plex-mono-latin-600-normal-DWFSQ4vo.woff","assets/ibm-plex-mono-latin-ext-400-normal-BmRBH3aV.woff2","assets/ibm-plex-mono-latin-ext-400-normal-D3D2R8hC.woff","assets/ibm-plex-mono-latin-ext-500-normal-CAhNIIs5.woff2","assets/ibm-plex-mono-latin-ext-500-normal-CZ70TYgx.woff","assets/ibm-plex-mono-latin-ext-600-normal-D38SheWl.woff2","assets/ibm-plex-mono-latin-ext-600-normal-DmB0ttJJ.woff","assets/ibm-plex-mono-vietnamese-400-normal-BulugwFq.woff2","assets/ibm-plex-mono-vietnamese-400-normal-DDuiU_S-.woff","assets/ibm-plex-mono-vietnamese-500-normal-C8zxqsMH.woff","assets/ibm-plex-mono-vietnamese-500-normal-DZ4AoWbu.woff2","assets/ibm-plex-mono-vietnamese-600-normal-D2EvbN8M.woff2","assets/ibm-plex-mono-vietnamese-600-normal-iLQfcSjf.woff","assets/ibm-plex-sans-cyrillic-ext-wght-normal-d45eAU9y.woff2","assets/ibm-plex-sans-cyrillic-wght-normal-BAAhND-U.woff2","assets/ibm-plex-sans-greek-wght-normal-CmyJS8uq.woff2","assets/ibm-plex-sans-latin-ext-wght-normal-CIII54If.woff2","assets/ibm-plex-sans-latin-wght-normal-IvpUvPa2.woff2","assets/ibm-plex-sans-vietnamese-wght-normal-Dg1JeJN0.woff2","assets/index-2oFlYdY5.css"];
const PUSH_ONLY = new URL(self.registration.scope).pathname.startsWith("/push/");
const CACHE = "grok-shell-a3612b47d5994d28";
const SHARE_TARGET = "disabled" === "enabled";
const shareInboxApi = null;
const pushWorkerApi = (() => {const PUSH_ACCOUNT_ORIGIN = "https://fabrials.com";
const PUSH_DATABASE = "grok-remote-push.v1";
const PUSH_EVENT_LIMIT = 256;

function parsePushNotice(value, now = Date.now()) {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).sort().join(",") !== "eventId,expiresAt,kind" || value.kind !== "remote_attention" || typeof value.eventId !== "string" || !/^[a-f0-9]{32}$/.test(value.eventId) || !Number.isSafeInteger(value.expiresAt) || value.expiresAt <= now || value.expiresAt > now + 60_000) return null;
  return {kind:"remote_attention",eventId:value.eventId,expiresAt:value.expiresAt};
}

async function claimPushNotice(notice, indexed = indexedDB, now = Date.now()) {
  const database = await new Promise((resolve,reject) => {
    const request = indexed.open(PUSH_DATABASE,1);
    let abandoned = false;
    const fail = () => {abandoned=true;clearTimeout(timer);reject(new Error("push_storage_unavailable"));};
    const timer = setTimeout(fail,2000);
    request.onupgradeneeded = () => request.result.createObjectStore("events",{keyPath:"eventId"});
    request.onsuccess = () => {clearTimeout(timer);if(abandoned)request.result.close();else resolve(request.result);};
    request.onerror = request.onblocked = fail;
  });
  try {
    return await new Promise((resolve,reject) => {
      const transaction = database.transaction("events","readwrite");
      const timer = setTimeout(() => {try {transaction.abort();} catch {}reject(new Error("push_storage_unavailable"));},2000);
      const store = transaction.objectStore("events");
      const request = store.getAll();
      let accepted = false;
      request.onsuccess = () => {
        let live = 0;
        let duplicate = false;
        for (const row of request.result) {
          if (!Number.isSafeInteger(row.expiresAt) || row.expiresAt <= now || row.expiresAt > now + 60_000) store.delete(row.eventId);
          else {live++;if (row.eventId === notice.eventId) duplicate = true;}
        }
        if (!duplicate && live < PUSH_EVENT_LIMIT && notice.expiresAt > now) {store.put(notice);accepted=true;}
      };
      transaction.oncomplete = () => {clearTimeout(timer);resolve(accepted);};
      transaction.onerror = transaction.onabort = () => {clearTimeout(timer);reject(new Error("push_storage_unavailable"));};
    });
  } finally {database.close();}
}

async function resolvePushNotice(notice, scope) {
  if (!parsePushNotice(notice)) return false;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(),5000);
  const run = async () => {
    const response = await scope.fetch(`${PUSH_ACCOUNT_ORIGIN}/api/spanreed/remote/push/resolve`,{method:"POST",credentials:"include",cache:"no-store",redirect:"error",signal:controller.signal,headers:{"content-type":"application/json"},body:JSON.stringify({eventId:notice.eventId})});
    if (!response.ok) return false;
    const result = await response.json();
    return result?.active === true && Boolean(parsePushNotice(notice));
  };
  try {
    return await (scope.navigator?.locks ? scope.navigator.locks.request(`grok-remote-account:${PUSH_ACCOUNT_ORIGIN}`,{mode:"exclusive",signal:controller.signal},run) : run());
  } catch {return false;}
  finally {clearTimeout(timer);}
}

function installRemotePushWorker(scope) {
  scope.addEventListener("message",event => {
    if (event.data?.kind === "remotePushCapabilities") event.ports?.[0]?.postMessage({kind:"remotePushCapabilities",version:1,directVersion:1,enabled:true});
  });
  scope.addEventListener("push",event => {
    event.waitUntil((async () => {
      try {
        const text = event.data?.text();
        if (typeof text !== "string" || text.length > 1024 || new TextEncoder().encode(text).length > 1024) return;
        const data = JSON.parse(text);
        if (data && typeof data === "object" && Object.keys(data).sort().join(",") === "kind,version" && data.version === 1 && data.kind === "remoteActivity") {
          await scope.registration.showNotification("Grok", {body:"There is activity on your computer.",tag:"grok-direct-activity",data:{version:1,kind:"remoteActivity"},renotify:false});
          return;
        }
        const notice = parsePushNotice(data);
        if (!notice || !await claimPushNotice(notice,scope.indexedDB) || !await resolvePushNotice(notice,scope)) return;
        await scope.registration.showNotification("Grok Remote",{body:"There is activity in your sessions.",tag:`grok-remote-${notice.eventId}`,data:notice,renotify:false});
      } catch {}
    })());
  });
  scope.addEventListener("notificationclick",event => {
    event.notification.close();
    event.waitUntil((async () => {
      try {
        const direct = event.notification.data;
        if (direct?.version === 1 && direct.kind === "remoteActivity") {
          const destination = new URL("/", scope.registration.scope);
          const windows = await scope.clients.matchAll({type:"window",includeUncontrolled:true});
          const existing = windows.find(client => new URL(client.url).origin === destination.origin);
          if (existing) await existing.focus();
          else await scope.clients.openWindow(destination.href);
          return;
        }
        const notice = parsePushNotice(event.notification.data);
        if (!notice || !await resolvePushNotice(notice,scope)) return;
        const destination = new URL("/",scope.registration.scope);
        if (destination.origin !== scope.location.origin) return;
        await scope.clients.openWindow(destination.href);
      } catch {}
    })());
  });
}

return {installRemotePushWorker};})();
if (pushWorkerApi) pushWorkerApi.installRemotePushWorker(self);
const STATE_CACHE = "grok-pwa-state-v1";
const STATE_URL = new URL("__shell_state__", self.registration.scope).href;
const MANIFEST_URL = new URL("__shell_manifest__", self.registration.scope).href;
const urls = new Set(SHELL.map((path) => new URL(path, self.registration.scope).href));

self.addEventListener("install", (event) => {
  if (!PUSH_ONLY) event.waitUntil(installShell());
});

self.addEventListener("activate", (event) => {
  if (!PUSH_ONLY) event.waitUntil(activateShell());
});

self.addEventListener("message", (event) => {
  if (event.data?.kind === "activateUpdate") self.skipWaiting();
  if (event.data?.kind === "shellStatus") event.waitUntil(shellState().then(state => event.ports?.[0]?.postMessage({canRollback: Boolean(state.previous)})));
  if (event.data?.kind === "rollbackShell") event.waitUntil((async () => {
    const state = await shellState();
    if (!state.previous) {event.ports?.[0]?.postMessage({ok: false}); return;}
    const cache = await caches.open(state.previous);
    if (!await cache.match(new URL("index.html", self.registration.scope).href)) {event.ports?.[0]?.postMessage({ok: false}); return;}
    await writeState({active: state.previous, previous: null});
    event.ports?.[0]?.postMessage({ok: true});
  })());
});

self.addEventListener("fetch", (event) => {
  if (PUSH_ONLY) return;
  const request = event.request;
  const url = new URL(request.url);
  if (SHARE_TARGET && request.method === "POST" && url.origin === self.location.origin && url.pathname === "/share-inbox" && !url.search) {
    event.respondWith(receiveSharedContent(request));
    return;
  }
  if (request.method !== "GET") return;
  if (url.origin !== self.location.origin || url.search) return;
  if (request.mode === "navigate" && (/^\/(?:s\/[^/]+|setup)?$/.test(url.pathname))) {
    event.respondWith(shellState().then(async state => (await (await caches.open(state.active)).match(new URL("index.html", self.registration.scope).href)) || Response.error()));
    return;
  }
  if (!urls.has(url.href) && !/^\/assets\/[^/]+-[A-Za-z0-9_-]+\.(?:js|css|woff2?)$/.test(url.pathname) && !/^\/crypto\/[a-f0-9]+\/fabrials_remote_noise(?:_bg)?\.(?:js|wasm)$/.test(url.pathname)) return;
  event.respondWith(shellState().then(async state => {
    for (const name of [state.active, state.previous].filter(Boolean)) {
      const cache = await caches.open(name);
      const manifest = await cache.match(MANIFEST_URL);
      const allowed = manifest ? await manifest.json() : name === CACHE ? [...urls] : [];
      if (Array.isArray(allowed) && allowed.includes(url.href)) return (await cache.match(request)) || Response.error();
    }
    return fetch(request);
  }));
});


async function receiveSharedContent(request) {
  try {
    const content = await shareInboxApi.parseSharedRequest(request);
    const id = await shareInboxApi.enqueueSharedContent(content);
    return Response.redirect(new URL(`/#share=${encodeURIComponent(id)}`, self.registration.scope).href, 303);
  } catch (error) {
    const code = error instanceof Error ? error.message : "share_unavailable";
    const messages = {share_too_large: "The shared content is too large. Share up to three files of 200 KB each and less than 1 MiB in total.", share_inbox_full: "Three shared items are waiting. Open Grok Remote and remove or use one before sharing again.", empty_share: "Nothing was shared. Return to the source app and try again."};
    return new Response(messages[code] ?? "The shared content could not be saved. Open Grok Remote and check site storage before trying again.", {status: code === "share_too_large" ? 413 : code === "share_inbox_full" ? 409 : 400, headers: {"content-type": "text/plain; charset=utf-8", "cache-control": "no-store"}});
  }
}

async function installShell() {
  try {
    const cache = await caches.open(CACHE);
    await cache.addAll([...urls]);
    await cache.put(MANIFEST_URL, Response.json([...urls]));
  } catch (error) {
    await caches.delete(CACHE);
    throw error;
  }
}

async function shellState() {
  const response = await (await caches.open(STATE_CACHE)).match(STATE_URL);
  try {
    const state = response ? await response.json() : null;
    if (state && typeof state.active === "string" && state.active.startsWith("grok-shell-") && (state.previous === null || typeof state.previous === "string" && state.previous.startsWith("grok-shell-"))) return state;
  } catch {}
  return {active: CACHE, previous: null};
}

async function writeState(state) {
  await (await caches.open(STATE_CACHE)).put(STATE_URL, Response.json(state));
}

async function activateShell() {
  const old = await shellState();
  const previous = old.active !== CACHE ? old.active : old.previous;
  await writeState({active: CACHE, previous});
  const keep = new Set([CACHE, previous]);
  const keys = await caches.keys();
  await Promise.all(keys.filter(key => key.startsWith("grok-shell-") && !keep.has(key)).map(key => caches.delete(key)));
}
