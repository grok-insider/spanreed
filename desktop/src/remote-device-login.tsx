import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { ExternalLink } from "lucide-react";
import { Button, Input, Label } from "@fabrials/ui";
import { useRemote } from "./remote-api";
import { Done, ErrorAlert } from "./feedback";
import type { GrokDeviceStart, GrokDeviceProgress, LoginView, Progress } from "./relay-contracts";

type Flow = { id: string; uri: string; code: string; interval: number; expires: number };
export function RemoteDeviceLogin({ provider, initialAlias, reauthorize = false, migration, onConnected }: { provider: "grok" | "nous" | "codex"; initialAlias?: string; reauthorize?: boolean; migration?: { migrationId: string; sourceId: string }; onConnected: () => Promise<void> }) {
  const remote = useRemote();
  const [alias, setAlias] = React.useState(initialAlias ?? "personal");
  const [flow, setFlow] = React.useState<Flow | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [retry, setRetry] = React.useState(0);
  const [connected, setConnected] = React.useState(false);
  const callback = React.useRef(onConnected); callback.current = onConnected;
  const pending = React.useRef<Flow | null>(null); pending.current = flow;
  const name = provider === "grok" ? "SuperGrok" : provider === "codex" ? "Codex" : "Nous";
  async function cancelRemote(value: Flow) { await remote(provider === "grok" ? "cancelGrok" : provider === "codex" ? "cancelCodex" : "cancelNous", provider === "grok" ? { state: value.id } : { id: value.id }); }
  React.useEffect(() => () => { if (pending.current) void cancelRemote(pending.current).catch(() => undefined); }, [provider]);
  React.useEffect(() => {
    if (!flow) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        if (Date.now() >= flow!.expires) throw new Error("The sign-in code expired. Cancel and start again.");
        const result = provider === "grok" ? await remote<GrokDeviceProgress>("pollGrok", { state: flow!.id }) : await remote<Progress>(provider === "codex" ? "pollCodex" : "pollNous", { id: flow!.id });
        if (stopped) return;
        if (("status" in result && result.status === "linked") || ("state" in result && result.state === "connected")) {
          pending.current = null; setFlow(null); setConnected(true); await callback.current();
        } else {
          const seconds = "retry_after_secs" in result ? result.retry_after_secs : "retryAfterSecs" in result ? result.retryAfterSecs : flow!.interval;
          timer = setTimeout(() => void poll(), Math.max(2, seconds) * 1000);
        }
      } catch (error) { if (!stopped) setError(String(error)); }
    }
    timer = setTimeout(() => void poll(), Math.max(2, flow.interval) * 1000);
    return () => { stopped = true; clearTimeout(timer); };
  }, [flow, provider, retry]);
  async function begin(event: React.FormEvent) {
    event.preventDefault(); setBusy(true); setError(null); setConnected(false);
    try {
      let next: Flow;
      if (provider === "grok") {
        const value = await remote<GrokDeviceStart>("beginGrok", { alias, reauthorize, newOnly: !reauthorize, ...migration });
        next = { id: value.state, uri: value.verification_uri, code: value.user_code, interval: value.interval, expires: Date.now() + value.expires_in * 1000 };
      } else {
        const value = await remote<LoginView>(provider === "codex" ? (reauthorize ? "reauthorizeCodex" : "beginCodex") : (reauthorize ? "reauthorizeNous" : "beginNous"), { alias, ...migration });
        next = { id: value.id, uri: value.device.verificationUri, code: value.device.userCode, interval: value.device.interval, expires: Date.now() + value.device.expiresIn * 1000 };
      }
      setFlow(next);
    } catch (error) { setError(String(error)); } finally { setBusy(false); }
  }
  if (connected) return <Done>Signed in {alias} on the hosted relay.</Done>;
  return <div className="sr-form">
    <p className="fui-description">The hosted relay keeps this {name} sign-in on its server.</p>
    <ErrorAlert title="Sign-in didn't complete" error={error} action={flow ? <Button variant="outline" size="sm" onClick={() => { setError(null); setRetry((value) => value + 1); }}>Check again</Button> : undefined} />
    {!flow ? <form className="sr-form" onSubmit={(event) => void begin(event)}>
      <Label>Account name<Input required pattern="[A-Za-z0-9_\-]+" maxLength={40} disabled={busy || reauthorize || !!migration} value={alias} onChange={(event) => setAlias(event.target.value)} /></Label>
      <div className="fui-actions"><Button disabled={busy} type="submit">{busy ? "Starting…" : reauthorize ? "Sign in again" : `Sign in to ${name}`}</Button></div>
    </form> : <div className="sr-form">
      <p>Enter this code on the sign-in page:</p>
      <output className="sr-device-code" aria-label={`${name} sign-in code`}>{flow.code}</output>
      <p className="fui-description">Waiting for approval. The code expires at {new Date(flow.expires).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}.</p>
      <div className="fui-actions">
        <Button onClick={() => void invoke("remote_open_authorization", { url: flow.uri }).catch((error) => setError(String(error)))}><ExternalLink aria-hidden size={16} />Open sign-in page</Button>
        <Button variant="outline" disabled={busy} onClick={async () => { setBusy(true); try { await cancelRemote(flow); pending.current = null; setFlow(null); setError(null); } catch (error) { setError(String(error)); } finally { setBusy(false); } }}>Cancel</Button>
      </div>
    </div>}
  </div>;
}
