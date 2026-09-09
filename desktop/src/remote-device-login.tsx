import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRemote } from "./remote-api";
import type { GrokDeviceStart, GrokDeviceProgress, LoginView, Progress } from "./relay-contracts";

type Flow = { id: string; uri: string; code: string; interval: number; expires: number };
export function RemoteDeviceLogin({ provider, initialAlias, reauthorize = false, migration, onConnected }: {provider:"grok" | "nous" | "codex"; initialAlias?:string; reauthorize?:boolean; migration?:{migrationId:string;sourceId:string}; onConnected:() => Promise<void>}) {
  const remote = useRemote();
  const [alias,setAlias] = React.useState(initialAlias ?? "personal");
  const [flow,setFlow] = React.useState<Flow | null>(null);
  const [error,setError] = React.useState<string | null>(null);
  const [busy,setBusy] = React.useState(false);
  const [retry,setRetry] = React.useState(0);
  const [connected,setConnected] = React.useState(false);
  const callback = React.useRef(onConnected); callback.current = onConnected;
  const pending = React.useRef<Flow | null>(null); pending.current = flow;
  async function cancelRemote(flow:Flow) { await remote(provider === "grok" ? "cancelGrok" : provider === "codex" ? "cancelCodex" : "cancelNous", provider === "grok" ? {state:flow.id} : {id:flow.id}); }
  React.useEffect(() => () => {if (pending.current) void cancelRemote(pending.current).catch(() => undefined);}, [provider]);
  React.useEffect(() => {
    if (!flow) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        if (Date.now() >= flow!.expires) throw new Error("Authorization code expired. Cancel and start again.");
        const result = provider === "grok" ? await remote<GrokDeviceProgress>("pollGrok",{state:flow!.id}) : await remote<Progress>(provider === "codex" ? "pollCodex" : "pollNous",{id:flow!.id});
        if (stopped) return;
        if (("status" in result && result.status === "linked") || ("state" in result && result.state === "connected")) {
          pending.current = null; setFlow(null); setConnected(true); await callback.current();
        } else {
          const seconds = "retry_after_secs" in result ? result.retry_after_secs : "retryAfterSecs" in result ? result.retryAfterSecs : flow!.interval;
          timer = setTimeout(() => void poll(), Math.max(2,seconds) * 1000);
        }
      } catch (error) { if (!stopped) setError(String(error)); }
    }
    timer = setTimeout(() => void poll(), Math.max(2,flow.interval) * 1000);
    return () => {stopped = true; clearTimeout(timer);};
  }, [flow,provider,retry]);
  async function begin(event:React.FormEvent) {
    event.preventDefault();setBusy(true);setError(null);setConnected(false);
    try {
      let next:Flow;
      if (provider === "grok") {
        const value = await remote<GrokDeviceStart>("beginGrok",{alias,reauthorize,newOnly:!reauthorize,...migration});
        next = {id:value.state,uri:value.verification_uri,code:value.user_code,interval:value.interval,expires:Date.now()+value.expires_in*1000};
      } else {
        const value = await remote<LoginView>(provider === "codex" ? (reauthorize ? "reauthorizeCodex" : "beginCodex") : (reauthorize ? "reauthorizeNous" : "beginNous"),{alias,...migration});
        next = {id:value.id,uri:value.device.verificationUri,code:value.device.userCode,interval:value.device.interval,expires:Date.now()+value.device.expiresIn*1000};
      }
      setFlow(next);
    } catch(error) {setError(String(error));} finally {setBusy(false);}
  }
  return <section className="fb-card"><header><h2>{reauthorize ? "Renew" : "Connect"} {provider === "grok" ? "SuperGrok" : provider === "codex" ? "Codex" : "Nous"} on ai-relay</h2><p className="fb-muted">Provider credentials will be held by your hosted workspace.</p></header><div className="fb-card-body fb-form">
    {error && <p className="fb-error" role="alert">{error}</p>}{connected && <p role="status">Connected {alias} to ai-relay.</p>}
    {!flow ? <form className="fb-form" onSubmit={event => void begin(event)}><label>Account name<input required pattern="[A-Za-z0-9_-]+" maxLength={40} disabled={busy || reauthorize || !!migration} value={alias} onChange={event => setAlias(event.target.value)}/></label><button className="fb-button fb-button-primary" disabled={busy} type="submit">{busy ? "Starting…" : "Start authorization"}</button></form> : <>
      <output aria-label={`${provider} authorization code`}><strong>{flow.code}</strong></output><p>Waiting for approval. Expires at {new Date(flow.expires).toLocaleTimeString()}.</p>
      <button className="fb-button" onClick={() => void invoke("remote_open_authorization",{url:flow.uri}).catch(error => setError(String(error)))}>Open provider authorization</button>
      <button className="fb-button" disabled={busy} onClick={async () => {setBusy(true);try {await cancelRemote(flow);pending.current=null;setFlow(null);setError(null);} catch(error){setError(String(error));}finally{setBusy(false);}}}>Cancel authorization</button>
      {error && <button className="fb-button" onClick={() => {setError(null);setRetry(value => value+1);}}>Retry check</button>}
    </>}
  </div></section>;
}
