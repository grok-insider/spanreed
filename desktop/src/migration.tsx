import * as React from "react";
import { DeviceLogin } from "./device-login";
import { invoke } from "@tauri-apps/api/core";
import { MigrationReviewDetails, type MigrationCandidate, type MigrationSessionView } from "@fabrials/ui";

import type { SavedSession } from "./contracts";

export function MigrationPage() {
  const [saved, setSaved] = React.useState<SavedSession[]>([]);
  const [forgetting, setForgetting] = React.useState(false);
  const [cached, setCached] = React.useState(false);
  React.useEffect(()=>{let stopped=false;void invoke<SavedSession[]>("saved_migrations").then(list=>{if(!stopped)setSaved(list);}).catch(error=>{if(!stopped)setError(String(error));});return ()=>{stopped=true;};},[]);
  const [authorizing, setAuthorizing] = React.useState<{provider:"grok" | "nous" | "codex";alias:string;sourceId:string} | null>(null);
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
  const errorRef = React.useRef<HTMLParagraphElement>(null);
  React.useEffect(() => {
    if (error) errorRef.current?.focus();
  }, [error]);
  const [receipt, setReceipt] = React.useState<string[] | null>(null);
  React.useEffect(() => {
    if (!receipt || !session) return;
    let stopped = false;
    void invoke<string[]>("migration_authorizations", {id:session.id})
      .then(ids => { if (!stopped) setAuthorized(ids); })
      .catch(error => { if (!stopped) setError(String(error)); });
    return () => { stopped = true; };
  }, [receipt, session?.id]);
  async function run(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true); setError(null);
    try { await action(); } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  async function update(view: MigrationSessionView) {
    setCached(false);setConfirmed(false); setSession(view);
    if (view.phase === "paired") setAccounts(await invoke<MigrationCandidate[]>("migration_inventory", {id:view.id}));
    else setAccounts([]);
  }
  async function transfer(id: string, revision: string) {
    const imported = await invoke<string[]>("execute_migration", {id, revision});
    setReceipt(imported); setConfirmed(false); setCached(true);
    await update(await invoke<MigrationSessionView>("migration_status", {id}));
  }
  const expired = session ? Date.now() >= session.expiresAtMs : false;
  return <section className="fb-form" aria-label="Account migration">
    <h1>Move accounts between environments</h1>
    <p className="fb-muted">Create an invitation in ai-relay, then pair this installation. Review the selected accounts in both environments before transferring.</p>
    {!session && <section className="fb-form" aria-label="Saved migrations">
      <h2>Saved sessions</h2>
      <button className="fb-button" disabled={busy} onClick={()=>void run(async()=>setSaved(await invoke<SavedSession[]>("saved_migrations")))}>Refresh saved sessions</button>
      {saved.map(entry=><button className="fb-button" disabled={busy} key={entry.id} onClick={()=>void run(async()=>{
        setForgetting(false);setId(entry.id);setOrigin(entry.origin);setReceipt(null);setSelection({});setAuthorizing(null);setAuthorized([]);
        if(entry.imported && entry.view){setSession(entry.view);setReceipt(entry.imported);setCached(true);setConfirmed(false);setAccounts([]);}
        else await update(await invoke<MigrationSessionView>("migration_status",{id:entry.id}));
      })}><span style={{overflowWrap:"anywhere"}}>{entry.origin} · {entry.imported ? "Saved receipt" : entry.view?.phase ?? "Pairing interrupted"}<br />{entry.id}</span></button>)}
    </section>}
    {!session ? <form className="fb-form" onSubmit={event => {event.preventDefault(); void run(async () => {
      const view = await invoke<MigrationSessionView>("pair_migration", {origin:origin.trim(), id:id.trim(), invitation:invitation.trim()});
      setInvitation(""); setSelection({}); setReceipt(null); await update(view);
    });}}>
      <label>Hosted origin<input type="url" required value={origin} disabled={busy} onChange={event=>setOrigin(event.target.value)} /></label>
      <label>Session ID<input required value={id} disabled={busy} autoComplete="off" onChange={event=>setId(event.target.value)} /></label>
      <label>Invitation secret<input type="password" value={invitation} disabled={busy} autoComplete="off" onChange={event=>setInvitation(event.target.value)} /></label>
      <div className="fb-row"><button className="fb-button fb-button-primary" disabled={busy || !invitation.trim()}>Pair installation</button>
      <button type="button" className="fb-button" disabled={busy || !id.trim()} onClick={()=>void run(async()=>{setReceipt(null);setSelection({});await update(await invoke<MigrationSessionView>("migration_status", {id:id.trim()}));})}>Resume saved session</button></div>
    </form> : <div className="fb-form">
      <p><strong>{session.direction === "localToHosted" ? "Spanreed → ai-relay" : "ai-relay → Spanreed"}</strong> · {session.phase}</p>
      <p className="fb-muted">Hosted owner: {session.ownerId}<br />Session: <code>{session.id}</code><br />Expires {new Date(session.expiresAtMs).toLocaleString()}</p>
      {cached && <p role="status">Showing the local receipt. Hosted status has not been refreshed.</p>}
      {expired && !receipt && <p role="alert">This session expired. Create a new invitation.</p>}
      {session.phase === "paired" && <form className="fb-form" onSubmit={event=>{event.preventDefault();void run(async()=>{
        await update(await invoke<MigrationSessionView>("propose_migration", {id:session.id, selection:Object.entries(selection).map(([sourceId,targetAlias])=>({sourceId,targetAlias}))}));
      });}}>
        <h2>Select accounts</h2>
        {!accounts.length && <p>No source accounts are available.</p>}
        {accounts.map(account=><div className="fb-form" key={account.id}>
          <label><input type="checkbox" checked={account.id in selection} disabled={busy || expired || account.credentialKind === "unavailable"} onChange={event=>setSelection(current=>{const next={...current};if(event.target.checked) next[account.id]=account.alias;else delete next[account.id];return next;})} /> {account.id} · {account.credentialKind === "oauth" ? "New OAuth authorization" : account.credentialKind === "apiKey" ? "API key" : "Unavailable"}</label>
          {account.id in selection && <label>Destination alias for {account.id}<input required pattern="[A-Za-z0-9_-]+" maxLength={128} value={selection[account.id]} disabled={busy || expired} onChange={event=>setSelection(current=>({...current,[account.id]:event.target.value}))} /></label>}
        </div>)}
        <button className="fb-button fb-button-primary" disabled={busy || expired || !Object.keys(selection).length}>Create review</button>
      </form>}
      {session.review && <MigrationReviewDetails review={session.review} />}
      {session.phase === "reviewing" && <p role="status">Approve this exact review in ai-relay, then refresh the session here.</p>}
      {(session.phase === "approved" || session.phase === "completed") && !receipt && <div className="fb-form">
        <label><input type="checkbox" checked={confirmed} disabled={busy || expired} onChange={event=>setConfirmed(event.target.checked)} /> I confirm the accounts and destination shown above.</label>
        <button className="fb-button fb-button-primary" disabled={busy || expired || !confirmed || !session.revision} onClick={()=>void run(()=>transfer(session.id, session.revision!))}>{session.phase === "completed" ? "Recover transfer receipt" : "Transfer selected API keys"}</button>
      </div>}
      {receipt && <div role="status"><p>API-key transfer completed. {receipt.length} account(s) imported.</p>{receipt.map(account=><p key={account}><code>{account}</code></p>)}{session.review?.items.some(item=>item.action === "authorizeOAuth") && <p>Each OAuth connection requires an independent login using its reviewed destination alias.</p>}</div>}
      {receipt && session.direction === "hostedToLocal" && <section className="fb-form" aria-label="Independent OAuth authorizations">
        {session.review?.items.filter(item=>item.action === "authorizeOAuth").map(item=><div key={item.sourceId}>
          {authorized.includes(`${item.provider}/${item.targetAlias}`) ? <p role="status">Connected {item.provider}/{item.targetAlias} with a new authorization.</p> : <button className="fb-button" disabled={!!authorizing} onClick={()=>{if(item.provider === "grok" || item.provider === "nous" || item.provider === "codex")setAuthorizing({provider:item.provider,alias:item.targetAlias,sourceId:item.sourceId});}}>Authorize {item.provider}/{item.targetAlias}</button>}
        </div>)}
        {authorizing && <div className="fb-form"><DeviceLogin key={`${authorizing.provider}/${authorizing.alias}`} provider={authorizing.provider} initialAlias={authorizing.alias} lockedAlias inactive migration={{id:session.id,sourceId:authorizing.sourceId}} onConnected={()=>{setAuthorizing(null);void run(async()=>setAuthorized(await invoke<string[]>("migration_authorizations", {id:session.id})));}} /><button className="fb-button" onClick={()=>setAuthorizing(null)}>Close authorization</button></div>}
      </section>}
      {receipt && session.phase !== "completed" && !expired && session.revision && <button className="fb-button" disabled={busy} onClick={()=>void run(()=>transfer(session.id, session.revision!))}>Finish receipt synchronization</button>}
      {receipt && <div className="fb-form">
        {!forgetting ? <button className="fb-button" disabled={busy || !!authorizing} onClick={()=>setForgetting(true)}>Forget local receipt</button> : <>
          <p>Remove this receipt and its pairing proof from this installation? Imported accounts remain connected. Pending OAuth guidance will no longer be available here.</p>
          <div className="fb-row"><button className="fb-button" disabled={busy || !!authorizing} onClick={()=>void run(async()=>{await invoke("forget_migration", {id:session.id});setId("");setInvitation("");setSession(null);setReceipt(null);setAuthorized([]);setForgetting(false);setSaved(await invoke<SavedSession[]>("saved_migrations"));})}>Forget this receipt</button><button className="fb-button" disabled={busy} onClick={()=>setForgetting(false)}>Keep receipt</button></div>
        </>}
      </div>}
      <div className="fb-row"><button className="fb-button" disabled={busy || expired} onClick={()=>void run(async()=>update(await invoke<MigrationSessionView>("migration_status", {id:session.id})))}>Refresh status</button>
      {!receipt && session.phase !== "completed" && <button className="fb-button" disabled={busy || expired} onClick={()=>void run(async()=>{await invoke("cancel_migration", {id:session.id});setSession(null);setInvitation("");})}>Cancel migration</button>}
      <button className="fb-button" disabled={busy} onClick={()=>{setSession(null);setInvitation("");setReceipt(null);setSelection({});setAuthorizing(null);setAuthorized([]);}}>Close</button></div>
    </div>}
    {busy && <p role="status">Updating migration…</p>}
    {error && <p ref={errorRef} tabIndex={-1} className="fb-error" role="alert">{error}</p>}
  </section>;
}
