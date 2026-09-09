import { ProviderIcon } from "@fabrials/ui";
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";

import type { LoginView as Login, Progress } from "./contracts";

export function DeviceLogin({ provider, accountId, initialAlias, lockedAlias = false, inactive = false, migration, onConnected }: { provider: "nous" | "grok" | "codex"; accountId?: string; initialAlias?: string; lockedAlias?: boolean; inactive?: boolean; migration?: {id: string; sourceId: string}; onConnected: () => void }) {
  const name = provider === "grok" ? "SuperGrok" : provider === "codex" ? "Codex" : "Nous";
  const portal = provider === "grok" ? "Grok authorization" : provider === "codex" ? "OpenAI authorization" : "Nous Portal";
  const [alias, setAlias] = React.useState(initialAlias ?? (provider === "nous" ? "portal" : "personal"));
  const [login, setLogin] = React.useState<Login | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [retry, setRetry] = React.useState(0);
  const connected = React.useRef(onConnected);
  connected.current = onConnected;
  const mounted = React.useRef(false);
  React.useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  React.useEffect(() => {
    if (!login) return;
    return () => { void invoke("cancel_device_login", {id: login.id}).catch(() => undefined); };
  }, [login]);
  React.useEffect(() => {
    if (!login) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const progress = await invoke<Progress>("poll_device_login", {id: login!.id});
        if (stopped) return;
        if (progress.state === "connected") {
          setLogin(null); setNotice(`Connected ${provider}/${login!.alias}.`); connected.current();
        } else {
          timer = setTimeout(() => void poll(), Math.max(1, progress.retryAfterSecs) * 1000);
        }
      } catch (error) { if (!stopped) setError(String(error)); }
    }
    timer = setTimeout(() => void poll(), login.device.interval * 1000);
    return () => { stopped = true; clearTimeout(timer); };
  }, [login, retry, provider]);
  async function begin(event: React.FormEvent) {
    event.preventDefault(); setBusy(true); setError(null); setNotice(null);
    try {
      const flow = migration ? await invoke<Login>("begin_migration_authorization", migration) : accountId ? await invoke<Login>("reauthorize_account", {id: accountId}) : await invoke<Login>(inactive ? "begin_inactive_device_login" : "begin_device_login", {provider, alias: alias.trim()});
      if (mounted.current) setLogin(flow);
      else await invoke("cancel_device_login", {id: flow.id});
    } catch (error) { if (mounted.current) setError(String(error)); }
    finally { if (mounted.current) setBusy(false); }
  }
  async function cancel() {
    if (!login) return;
    setBusy(true);
    try { await invoke("cancel_device_login", {id: login.id}); setLogin(null); setError(null); }
    catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  return <section className="fb-card">
    <header><div className="fb-row"><h2>{accountId ? `Authorize ${initialAlias} again` : `Connect ${name}`}</h2><ProviderIcon provider={provider}/></div><p className="fb-muted">Authorize a new local connection through {portal}.</p></header>
    <div className="fb-card-body">
      {error && <p className="fb-error" role="alert">{error}</p>}
      {notice && <p role="status">{notice}</p>}
      {!login ? <form className="fb-form" onSubmit={event => void begin(event)}>
        <label>Account name<input required pattern="[A-Za-z0-9_-]+" maxLength={40} value={alias} disabled={busy || Boolean(accountId) || lockedAlias} onChange={event => setAlias(event.target.value)} /></label>
        <button type="submit" className="fb-button fb-button-primary" disabled={busy}>{busy ? "Starting authorization…" : (accountId ? "Authorize again" : `Connect ${name} account`)}</button>
      </form> : <div className="fb-form">
        <p>Enter this code in {portal}:</p><output aria-label={`${name} authorization code`}><strong>{login.device.userCode}</strong></output>
        <p className="fb-muted">This code expires after {Math.ceil(login.device.expiresIn / 60)} minutes. Waiting for your approval.</p>
        <div className="fb-row"><button className="fb-button fb-button-primary" disabled={busy} onClick={() => void invoke("open_device_login", {id: login.id}).catch(error => setError(String(error)))}>Open {portal}</button>
        <button className="fb-button" disabled={busy} onClick={() => void cancel()}>Cancel</button></div>
        {error && <button className="fb-button" onClick={() => {setError(null); setRetry(retry + 1);}}>Retry authorization check</button>}
      </div>}
    </div>
  </section>;
}
