import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { ExternalLink } from "lucide-react";
import { Badge, Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Input, Label, NativeCheckbox } from "@fabrials/ui";
import { MigrationReviewDetails, type MigrationCandidate, type MigrationSessionView } from "@fabrials/ai-ui";
import { DeviceLogin } from "./device-login";
import { Done, ErrorAlert } from "./feedback";
import { absoluteTime } from "./format";
import type { SavedSession } from "./contracts";

const phaseLabel: Record<string, string> = { paired: "Paired", reviewing: "Waiting for approval in the hosted relay", approved: "Approved", completed: "Completed" };

export function MigrationPage() {
  const [saved, setSaved] = React.useState<SavedSession[]>([]);
  const [forgetting, setForgetting] = React.useState(false);
  const [cached, setCached] = React.useState(false);
  const [authorizing, setAuthorizing] = React.useState<{ provider: "grok" | "nous" | "codex"; alias: string; sourceId: string } | null>(null);
  const [authorized, setAuthorized] = React.useState<string[]>([]);
  const [origin, setOrigin] = React.useState("https://ai.fabrials.com");
  const [id, setId] = React.useState("");
  const [invitation, setInvitation] = React.useState("");
  const [session, setSession] = React.useState<MigrationSessionView | null>(null);
  const [accounts, setAccounts] = React.useState<MigrationCandidate[]>([]);
  const [selection, setSelection] = React.useState<Record<string, string>>({});
  const [confirmed, setConfirmed] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [receipt, setReceipt] = React.useState<string[] | null>(null);
  React.useEffect(() => {
    let stopped = false;
    void invoke<SavedSession[]>("saved_migrations").then((list) => { if (!stopped) setSaved(list); }).catch((error) => { if (!stopped) setError(String(error)); });
    return () => { stopped = true; };
  }, []);
  React.useEffect(() => {
    if (!receipt || !session) return;
    let stopped = false;
    void invoke<string[]>("migration_authorizations", { id: session.id })
      .then((ids) => { if (!stopped) setAuthorized(ids); })
      .catch((error) => { if (!stopped) setError(String(error)); });
    return () => { stopped = true; };
  }, [receipt, session?.id]);
  async function run(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true); setError(null);
    try { await action(); } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  async function update(view: MigrationSessionView) {
    setCached(false); setConfirmed(false); setSession(view);
    if (view.phase === "paired") setAccounts(await invoke<MigrationCandidate[]>("migration_inventory", { id: view.id }));
    else setAccounts([]);
  }
  async function transfer(sessionId: string, revision: string) {
    const imported = await invoke<string[]>("execute_migration", { id: sessionId, revision });
    setReceipt(imported); setConfirmed(false); setCached(true);
    await update(await invoke<MigrationSessionView>("migration_status", { id: sessionId }));
  }
  function close() { setSession(null); setInvitation(""); setReceipt(null); setSelection({}); setAuthorizing(null); setAuthorized([]); }
  const expired = session ? Date.now() >= session.expiresAtMs : false;
  return <section className="sr-stack" aria-label="Transfer accounts">
    <p className="fui-description">Move added accounts between this computer and a hosted relay. API keys are copied after you approve on both sides. Sign-in accounts (OAuth) need a fresh sign-in at the destination.</p>
    <ErrorAlert title="Transfer didn't work" error={error} />
    {!session ? <>
      <Card>
        <CardHeader><CardTitle>Pair with a hosted relay</CardTitle><CardDescription>In the hosted relay, open <strong>Migration</strong> and create an invitation. Paste its session ID and secret here.</CardDescription></CardHeader>
        <CardContent>
          <form className="sr-form" onSubmit={(event) => {
            event.preventDefault();
            void run(async () => {
              const view = await invoke<MigrationSessionView>("pair_migration", { origin: origin.trim(), id: id.trim(), invitation: invitation.trim() });
              setInvitation(""); setSelection({}); setReceipt(null); await update(view);
            });
          }}>
            <Label>Hosted relay address<Input type="url" required value={origin} disabled={busy} onChange={(event) => setOrigin(event.target.value)} /></Label>
            <div className="sr-field-row">
              <Label>Session ID<Input required value={id} disabled={busy} autoComplete="off" onChange={(event) => setId(event.target.value)} /></Label>
              <Label>Invitation secret<Input type="password" value={invitation} disabled={busy} autoComplete="off" onChange={(event) => setInvitation(event.target.value)} /></Label>
            </div>
            <div className="fui-actions">
              <Button type="submit" disabled={busy || !id.trim() || !invitation.trim()}>Pair this computer</Button>
              <Button type="button" variant="outline" disabled={busy || !id.trim()} onClick={() => void run(async () => { setReceipt(null); setSelection({}); await update(await invoke<MigrationSessionView>("migration_status", { id: id.trim() })); })}>Resume a session by ID</Button>
              <Button type="button" variant="ghost" onClick={() => void invoke("open_hosted").catch((error) => setError(String(error)))}><ExternalLink aria-hidden size={16} />Open ai.fabrials.com</Button>
            </div>
          </form>
        </CardContent>
      </Card>
      {saved.length > 0 && <Card>
        <CardHeader><CardTitle>Saved transfers</CardTitle><CardDescription>Transfers this computer started earlier. Open one to continue or to see its receipt.</CardDescription></CardHeader>
        <CardContent>
          <ul className="sr-list">{saved.map((entry) => <li key={entry.id}>
            <span><span className="sr-choice-name">{entry.origin}</span><span className="fui-description"><code>{entry.id}</code> · {entry.imported ? "Receipt saved" : entry.view?.phase ? phaseLabel[entry.view.phase] ?? entry.view.phase : "Pairing interrupted"}</span></span>
            <Button variant="outline" size="sm" disabled={busy} onClick={() => void run(async () => {
              setForgetting(false); setId(entry.id); setOrigin(entry.origin); setReceipt(null); setSelection({}); setAuthorizing(null); setAuthorized([]);
              if (entry.imported && entry.view) { setSession(entry.view); setReceipt(entry.imported); setCached(true); setConfirmed(false); setAccounts([]); }
              else await update(await invoke<MigrationSessionView>("migration_status", { id: entry.id }));
            })}>Open</Button>
          </li>)}</ul>
          <div className="fui-actions"><Button variant="ghost" size="sm" disabled={busy} onClick={() => void run(async () => setSaved(await invoke<SavedSession[]>("saved_migrations")))}>Refresh list</Button></div>
        </CardContent>
      </Card>}
    </> : <Card>
      <CardHeader className="sr-card-header-row">
        <div>
          <CardTitle>{session.direction === "localToHosted" ? "This computer → hosted relay" : "Hosted relay → this computer"}</CardTitle>
          <CardDescription>Session <code>{session.id}</code> · owner {session.ownerId} · expires {absoluteTime(session.expiresAtMs)}</CardDescription>
        </div>
        <Badge tone={expired ? "danger" : session.phase === "completed" ? "success" : "neutral"}>{expired && !receipt ? "Expired" : phaseLabel[session.phase] ?? session.phase}</Badge>
      </CardHeader>
      <CardContent className="sr-form">
        {cached && <p role="status" className="fui-description">Showing the receipt saved on this computer. The hosted status wasn't refreshed.</p>}
        {expired && !receipt && <p role="alert" className="sr-warning">This session expired. Create a new invitation in the hosted relay.</p>}
        {session.phase === "paired" && <form className="sr-form" onSubmit={(event) => {
          event.preventDefault();
          void run(async () => { await update(await invoke<MigrationSessionView>("propose_migration", { id: session.id, selection: Object.entries(selection).map(([sourceId, targetAlias]) => ({ sourceId, targetAlias })) })); });
        }}>
          <h3 className="sr-subsection-title">1. Choose accounts</h3>
          {!accounts.length && <p className="fui-description">There are no accounts to transfer.</p>}
          {accounts.map((account) => <div className="sr-transfer-choice" key={account.id}>
            <label className="sr-check">
              <NativeCheckbox checked={account.id in selection} disabled={busy || expired || account.credentialKind === "unavailable"} onChange={(event) => setSelection((current) => { const next = { ...current }; if (event.target.checked) next[account.id] = account.alias; else delete next[account.id]; return next; })} />
              <span><span className="sr-choice-name">{account.id}</span><span className="fui-description">{account.credentialKind === "oauth" ? "Needs a new sign-in at the destination" : account.credentialKind === "apiKey" ? "API key is copied" : "Can't be transferred"}</span></span>
            </label>
            {account.id in selection && <Label>Name at the destination<Input required pattern="[A-Za-z0-9_\-]+" maxLength={128} value={selection[account.id]} disabled={busy || expired} onChange={(event) => setSelection((current) => ({ ...current, [account.id]: event.target.value }))} /></Label>}
          </div>)}
          <div className="fui-actions"><Button type="submit" disabled={busy || expired || !Object.keys(selection).length}>Continue to review</Button></div>
        </form>}
        {session.review && <MigrationReviewDetails review={session.review} />}
        {session.phase === "reviewing" && <p role="status">2. Approve this exact review in the hosted relay, then choose <strong>Refresh status</strong> here.</p>}
        {(session.phase === "approved" || session.phase === "completed") && !receipt && <div className="sr-form">
          <label className="sr-check"><NativeCheckbox checked={confirmed} disabled={busy || expired} onChange={(event) => setConfirmed(event.target.checked)} /><span>I checked the accounts and destination shown above.</span></label>
          <div className="fui-actions"><Button disabled={busy || expired || !confirmed || !session.revision} onClick={() => void run(() => transfer(session.id, session.revision!))}>{session.phase === "completed" ? "Recover the receipt" : "Transfer the API keys"}</Button></div>
        </div>}
        {receipt && <Done>Transfer complete: {receipt.length} account{receipt.length === 1 ? "" : "s"} imported{receipt.length ? ` (${receipt.join(", ")})` : ""}.{session.review?.items.some((item) => item.action === "authorizeOAuth") ? " Each sign-in account still needs its own sign-in, using the name you reviewed." : ""}</Done>}
        {receipt && session.direction === "hostedToLocal" && <section className="sr-form" aria-label="Sign-ins still needed">
          {session.review?.items.filter((item) => item.action === "authorizeOAuth").map((item) => <div key={item.sourceId} className="sr-list-row">
            {authorized.includes(`${item.provider}/${item.targetAlias}`) ? <Done>Signed in {item.provider}/{item.targetAlias}.</Done>
              : <Button variant="outline" size="sm" disabled={!!authorizing} onClick={() => { if (item.provider === "grok" || item.provider === "nous" || item.provider === "codex") setAuthorizing({ provider: item.provider, alias: item.targetAlias, sourceId: item.sourceId }); }}>Sign in {item.provider}/{item.targetAlias}</Button>}
          </div>)}
          {authorizing && <Card className="sr-inset"><CardContent className="sr-form">
            <DeviceLogin key={`${authorizing.provider}/${authorizing.alias}`} provider={authorizing.provider} initialAlias={authorizing.alias} lockedAlias inactive migration={{ id: session.id, sourceId: authorizing.sourceId }} onConnected={() => { setAuthorizing(null); void run(async () => setAuthorized(await invoke<string[]>("migration_authorizations", { id: session.id }))); }} />
            <div className="fui-actions"><Button variant="ghost" size="sm" onClick={() => setAuthorizing(null)}>Close sign-in</Button></div>
          </CardContent></Card>}
        </section>}
        {receipt && session.phase !== "completed" && !expired && session.revision && <div className="fui-actions"><Button variant="outline" disabled={busy} onClick={() => void run(() => transfer(session.id, session.revision!))}>Finish syncing the receipt</Button></div>}
        {receipt && (!forgetting ? <div className="fui-actions"><Button variant="ghost" size="sm" disabled={busy || !!authorizing} onClick={() => setForgetting(true)}>Forget this receipt…</Button></div> : <div className="sr-review">
          <p>Remove this receipt and its pairing proof from this computer? Imported accounts stay connected, but pending sign-in guidance won't be shown here anymore.</p>
          <div className="fui-actions">
            <Button variant="destructive" size="sm" disabled={busy || !!authorizing} onClick={() => void run(async () => { await invoke("forget_migration", { id: session.id }); setId(""); close(); setForgetting(false); setSaved(await invoke<SavedSession[]>("saved_migrations")); })}>Forget receipt</Button>
            <Button variant="outline" size="sm" disabled={busy} onClick={() => setForgetting(false)}>Keep it</Button>
          </div>
        </div>)}
        <div className="fui-actions sr-card-footer-actions">
          <Button variant="outline" disabled={busy || expired} onClick={() => void run(async () => update(await invoke<MigrationSessionView>("migration_status", { id: session.id })))}>Refresh status</Button>
          {!receipt && session.phase !== "completed" && <Button variant="ghost" disabled={busy || expired} onClick={() => void run(async () => { await invoke("cancel_migration", { id: session.id }); setSession(null); setInvitation(""); })}>Cancel transfer</Button>}
          <Button variant="ghost" disabled={busy} onClick={close}>Close</Button>
        </div>
        {busy && <p role="status" className="fui-description">Updating…</p>}
      </CardContent>
    </Card>}
  </section>;
}
