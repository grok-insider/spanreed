import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Input, Label, NativeCheckbox } from "@fabrials/ui";
import { ErrorAlert } from "./feedback";
import type { SessionMoveView } from "./contracts";

export function CodexSessionMove({ owner, onChanged }: { owner: string; onChanged: () => Promise<void> }) {
  const [view, setView] = React.useState<SessionMoveView | null>(null);
  const [alias, setAlias] = React.useState("codex-desktop");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [reviewed, setReviewed] = React.useState(false);
  const sequence = React.useRef(0);
  React.useEffect(() => {
    let active = true;
    void invoke<SessionMoveView | null>("codex_session_move", { owner, operation: "current" }).then((value) => { if (active) setView(value); }).catch((error) => { if (active) setError(String(error)); });
    return () => { active = false; sequence.current++; };
  }, [owner]);
  async function run(operation: string) {
    const current = ++sequence.current;
    setBusy(true); setError(null);
    try {
      const value = await invoke<SessionMoveView | null>("codex_session_move", { owner, operation, id: view?.id ?? null, alias });
      if (current !== sequence.current) return;
      setView(value); setReviewed(false);
      if (value?.state === "completed") await onChanged();
    } catch (error) {
      if (current !== sequence.current) return;
      setError(String(error));
      try { const value = await invoke<SessionMoveView | null>("codex_session_move", { owner, operation: "current" }); if (current === sequence.current) setView(value); } catch { /* Keep the last recoverable view. */ }
    } finally { if (current === sequence.current) setBusy(false); }
  }
  const terminal = view?.state === "completed" || view?.state === "cancelled";
  return <Card>
    <CardHeader><CardTitle>Move a local Codex session to the hosted relay</CardTitle><CardDescription>An alternative to signing in again on the relay. The relay takes over refreshing the session, and Codex on this computer uses a private, account-scoped proxy key.</CardDescription></CardHeader>
    <CardContent className="sr-form">
      <p className="fui-description">Close Codex, OpenCode and other Spanreed windows first. Only file-based sessions with a single local owner can move. Sessions in a system credential store, or with other copies, need a new sign-in instead.</p>
      {!view ? <>
        <Label>Name on the hosted relay<Input value={alias} maxLength={40} onChange={(event) => setAlias(event.target.value)} disabled={busy} /></Label>
        <div className="fui-actions"><Button variant="outline" disabled={busy || !alias.trim()} onClick={() => void run("preview")}>Review the move</Button></div>
      </> : <>
        <p role="status">{view.state === "completed" ? "Done. The hosted relay owns the session, and Codex now uses it." : view.state === "cancelled" ? "Move cancelled. The original local files were restored." : `Saved move: ${view.state}`}</p>
        <dl className="sr-facts">
          <dt>Sign-in to retire</dt><dd><code>{view.source_path}</code></dd>
          <dt>Settings to update</dt><dd><code>{view.config_path}</code></dd>
          <dt>New Responses endpoint</dt><dd><code>{view.endpoint}</code></dd>
        </dl>
        {view.state === "prepared" && <>
          <label className="sr-check"><NativeCheckbox checked={reviewed} onChange={(event) => setReviewed(event.target.checked)} disabled={busy} /><span>I closed the tools and removed any other copies of this sign-in. Move it to the hosted relay and update my Codex settings.</span></label>
          <p className="fui-description">Your model and unrelated settings are kept. Profile, project or command-line provider overrides must be updated separately. The proxy key stays in the private local settings.</p>
          <div className="fui-actions"><Button disabled={busy || !reviewed} onClick={() => void run("apply")}>Move and update Codex</Button></div>
        </>}
        {!terminal && <div className="fui-actions"><Button variant="outline" size="sm" disabled={busy} onClick={() => void run("recover")}>Check the hosted receipt</Button><Button variant="ghost" size="sm" disabled={busy} onClick={() => void run("cancel")}>Cancel and restore</Button></div>}
        {terminal && <div className="fui-actions"><Button variant="outline" size="sm" disabled={busy} onClick={() => void run("dismiss")}>Dismiss</Button></div>}
        {!terminal && <p className="fui-description">If a step is interrupted, keep this record. Restoring waits until the server confirms no delayed import can still claim the session.</p>}
      </>}
      <ErrorAlert title="The move didn't complete" error={error} />
    </CardContent>
  </Card>;
}
