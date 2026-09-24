import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { ExternalLink } from "lucide-react";
import { Badge, Button } from "@fabrials/ui";
import { ErrorAlert } from "./feedback";
import type { LinkView } from "./contracts";

export function FabrialsLink({ onChange }: { onChange?: (view: LinkView) => void }) {
  const [view, setView] = React.useState<LinkView | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [retry, setRetry] = React.useState(0);
  const changed = React.useRef(onChange);
  changed.current = onChange;
  const update = React.useCallback((next: LinkView) => { setView(next); setError(null); changed.current?.(next); }, []);
  React.useEffect(() => {
    let stopped = false;
    void invoke<LinkView>("fabrials_status").then((next) => { if (!stopped) update(next); }).catch((error) => { if (!stopped) setError(String(error)); });
    return () => { stopped = true; };
  }, [update]);
  React.useEffect(() => {
    if (view?.state !== "pending") return;
    let stopped = false;
    const timer = setTimeout(() => {
      void invoke<LinkView>("fabrials_poll", { id: view.id }).then((next) => { if (!stopped) update(next); }).catch((error) => { if (!stopped) setError(String(error)); });
    }, Math.max(1, view.retryAfterSecs) * 1000);
    return () => { stopped = true; clearTimeout(timer); };
  }, [view, retry, update]);
  async function run(action: () => Promise<unknown>) {
    setBusy(true); setError(null);
    try { await action(); } catch (error) { setError(String(error)); } finally { setBusy(false); }
  }
  const begin = () => run(async () => {
    const next = await invoke<LinkView>("fabrials_begin");
    update(next);
    if (next.state === "pending") await invoke("fabrials_open", { id: next.id });
  });
  return <div className="sr-form">
    {view?.state === "declined" && <p role="status">The connection was declined. You can start a new one.</p>}
    {view?.state === "expired" && <p role="status">That code expired. Start a new connection to continue.</p>}
    <ErrorAlert title="Couldn't connect to Fabrials" error={error} action={view?.state === "pending" ? <Button variant="outline" size="sm" onClick={() => { setError(null); setRetry((value) => value + 1); }}>Check again</Button> : undefined} />
    {view === null && !error ? <p role="status" className="fui-description">Checking this computer's connection…</p>
      : view?.state === "linked" ? <div className="sr-setting">
        <div className="sr-setting-text"><p className="sr-setting-title">Connected as @{view.user.username}</p><p className="fui-description">Sharing and sync each still need to be turned on separately.</p></div>
        <div className="fui-actions"><Badge tone="success">Connected</Badge><Button variant="outline" size="sm" disabled={busy} onClick={() => void run(async () => { await invoke("fabrials_disconnect"); update({ state: "disconnected" }); })}>Disconnect</Button></div>
      </div>
      : view?.state === "pending" ? <div className="sr-form">
        <p>Approve this code in your browser:</p>
        <output className="sr-device-code" aria-label="Fabrials connection code">{view.userCode}</output>
        <p className="fui-description">Waiting for approval. The code expires at {new Date(view.expiresAtMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}.</p>
        <div className="fui-actions">
          <Button disabled={busy} onClick={() => void run(() => invoke("fabrials_open", { id: view.id }))}><ExternalLink aria-hidden size={16} />Open in browser</Button>
          <Button variant="outline" disabled={busy} onClick={() => void run(async () => { await invoke("fabrials_cancel", { id: view.id }); update({ state: "disconnected" }); })}>Cancel</Button>
        </div>
      </div>
      : <div className="fui-actions"><Button disabled={busy} onClick={() => void begin()}>{busy ? "Starting…" : "Connect to Fabrials"}</Button></div>}
  </div>;
}
