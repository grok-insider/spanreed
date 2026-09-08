import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
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
    void invoke<LinkView>("fabrials_status").then(next => { if (!stopped) update(next); }).catch(error => { if (!stopped) setError(String(error)); });
    return () => { stopped = true; };
  }, [update]);
  React.useEffect(() => {
    if (view?.state !== "pending") return;
    let stopped = false;
    const timer = setTimeout(() => {
      void invoke<LinkView>("fabrials_poll", { id: view.id }).then(next => { if (!stopped) update(next); }).catch(error => { if (!stopped) setError(String(error)); });
    }, Math.max(1, view.retryAfterSecs) * 1000);
    return () => { stopped = true; clearTimeout(timer); };
  }, [view, retry, update]);
  async function begin() {
    setBusy(true); setError(null);
    try {
      const next = await invoke<LinkView>("fabrials_begin");
      update(next);
      if (next.state === "pending") await invoke("fabrials_open", { id: next.id });
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  async function cancel() {
    if (view?.state !== "pending") return;
    setBusy(true);
    try { await invoke("fabrials_cancel", { id: view.id }); update({ state: "disconnected" }); }
    catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  async function disconnect() {
    setBusy(true); setError(null);
    try { await invoke("fabrials_disconnect"); update({ state: "disconnected" }); }
    catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  return <section className="fb-card"><header><h2>Fabrials account</h2><p className="fb-muted">Connect this installation to your account. Sharing and provider migration have separate controls.</p></header><div className="fb-card-body fb-form">
    {view?.state === "declined" && <p role="status">Connection declined. You can start a new connection.</p>}
    {view?.state === "expired" && <p role="status">This code expired. Start a new connection to continue.</p>}
    {error && <p role="alert" className="fb-error">{error}</p>}
    {view === null && !error ? <p role="status">Checking this installation’s connection…</p> : view?.state === "linked" ? <><p role="status">Connected as @{view.user.username}</p><button className="fb-button" disabled={busy} onClick={() => void disconnect()}>Disconnect this installation</button></> : view?.state === "pending" ? <>
      <p>Approve this code in your browser:</p><output aria-label="Fabrials authorization code"><strong>{view.userCode}</strong></output>
      <p className="fb-muted">Waiting for approval. Code expires at {new Date(view.expiresAtMs).toLocaleTimeString()}.</p>
      <div className="fb-row"><button className="fb-button fb-button-primary" disabled={busy} onClick={() => void invoke("fabrials_open", {id: view.id}).catch(error => setError(String(error)))}>Open authorization</button><button className="fb-button" disabled={busy} onClick={() => void cancel()}>Cancel</button></div>
      {error && <button className="fb-button" onClick={() => {setError(null); setRetry(value => value + 1);}}>Retry connection check</button>}
    </> : <button className="fb-button fb-button-primary" disabled={busy} onClick={() => void begin()}>{busy ? "Starting connection…" : "Connect to Fabrials"}</button>}
  </div></section>;
}
