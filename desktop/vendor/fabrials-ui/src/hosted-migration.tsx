"use client";
import * as React from "react";
import { MigrationReviewDetails } from "./migration-review";
import type { MigrationDirection, MigrationInvitation, MigrationSessionView } from "./contracts";
export interface HostedMigrationApi {
  sessions(): Promise<{sessions: MigrationSessionView[]}>;
  create(request: {direction: MigrationDirection}): Promise<MigrationInvitation>;
  status(request: {id: string}): Promise<MigrationSessionView>;
  approve(request: {id: string; revision: string}): Promise<unknown>;
  authorizations(request: {id: string}): Promise<string[]>;
  forget(request: {id: string}): Promise<unknown>;
  cancel(request: {id: string}): Promise<unknown>;
}
export type MigrationAuthorization = {provider: "grok" | "nous"; alias: string; migrationId: string; sourceId: string; onConnected: () => Promise<void>};

export function HostedMigration({api: migrationApi, origin, authorize}: {api: HostedMigrationApi; origin: string; authorize: (request: MigrationAuthorization) => React.ReactNode}) {
  const [authorized, setAuthorized] = React.useState<string[]>([]);
  const [direction, setDirection] = React.useState<MigrationDirection>("hostedToLocal");
  const [invitation, setInvitation] = React.useState<MigrationInvitation | null>(null);
  const [sessions, setSessions] = React.useState<MigrationSessionView[]>([]);
  const [selected, setSelected] = React.useState<MigrationSessionView | null>(null);
  const [forgetting, setForgetting] = React.useState(false);
  const [confirmed, setConfirmed] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const errorRef = React.useRef<HTMLParagraphElement>(null);
  React.useEffect(() => {
    if (error) errorRef.current?.focus();
  }, [error]);
  const [now, setNow] = React.useState(()=>Date.now());
  async function refresh() {
    const data = await migrationApi.sessions();
    setSessions(data.sessions);
  }
  React.useEffect(()=>{
    const controller = new AbortController();
    migrationApi.sessions()
      .then(data=>setSessions(data.sessions))
      .catch(error=>{if(!controller.signal.aborted)setError(String(error));});
    const timer=setInterval(()=>setNow(Date.now()),1000);
    return ()=>{controller.abort();clearInterval(timer);};
  },[migrationApi]);
  async function run(action:()=>Promise<void>) {if(busy)return;setBusy(true);setError(null);try{await action();}catch(error){setError(String(error));}finally{setBusy(false);}}
  async function open(id:string) {
    const view = await migrationApi.status({id});
    setSessions(current=>current.map(session=>session.id === view.id ? view : session));
    setAuthorized([]);setSelected(view);setConfirmed(false);setForgetting(false);
    if(view.phase === "completed" && view.direction === "localToHosted")setAuthorized(await migrationApi.authorizations({id}));
    if(view.phase !== "pairing")setInvitation(null);
  }
  return <div className="fb-form">
    <h1 >Account migration</h1>
    <p className="fb-muted">Pair Spanreed with this hosted workspace. Review the destination and selected accounts before approving a transfer.</p>
    <section className="fb-form" aria-label="Create migration invitation">
      <label>Direction<select value={direction} disabled={busy} onChange={event=>setDirection(event.target.value as MigrationDirection)}><option value="hostedToLocal">ai-relay → Spanreed</option><option value="localToHosted">Spanreed → ai-relay</option></select></label>
      <button className="fb-button" disabled={busy} onClick={()=>void run(async()=>{const result=await migrationApi.create({direction});setInvitation(result);setSelected(null);setConfirmed(false);await refresh();})}>Create invitation</button>
      {invitation && <div className="fb-form">
        <p>Enter these details in Spanreed → Migration. The invitation expires {new Date(invitation.expiresAtMs).toLocaleTimeString()}.</p>
        <label>Hosted origin<input readOnly value={origin} /></label>
        <label>Session ID<input readOnly value={invitation.id} /></label>
        {now < invitation.expiresAtMs ? <label>Invitation secret<input readOnly type="password" autoComplete="off" value={invitation.secret} onFocus={event=>event.currentTarget.select()} /></label> : <p role="alert">Invitation expired. Create another invitation.</p>}
        {now < invitation.expiresAtMs && <button className="fb-button" disabled={busy} onClick={()=>void run(async()=>{await navigator.clipboard.writeText(invitation.secret);})}>Copy invitation secret</button>}
        <p className="fb-muted">The secret is shown only in this page session. Copy it into Spanreed before closing this page.</p>
        <button className="fb-button" disabled={busy} onClick={()=>void run(async()=>open(invitation.id))}>Check pairing</button>
        <button className="fb-button" onClick={()=>setInvitation(null)}>Hide invitation</button>
      </div>}
    </section>
    <section className="fb-form" aria-label="Migration sessions">
      <h2 >Sessions</h2>
      <p className="fb-muted">Completed receipts remain available for 30 days after the transfer window closes.</p>
      <button className="fb-button" disabled={busy} onClick={()=>void run(refresh)}>Refresh sessions</button>
      {!sessions.length && <p className="fb-muted">No migration sessions.</p>}
      {sessions.map(session=><button className="fb-button" key={session.id} disabled={busy} onClick={()=>void run(async()=>open(session.id))}><span style={{overflowWrap:"anywhere"}}>{session.direction === "hostedToLocal" ? "ai-relay → Spanreed" : "Spanreed → ai-relay"} · {session.phase}<br />{session.id}</span></button>)}
    </section>
    {selected && <section className="fb-form" aria-label="Selected migration">
      <h2 >Review transfer</h2>
      <p style={{overflowWrap:"anywhere"}}>Session: <code>{selected.id}</code><br />Local installation: <code>{selected.localEnvironment || "Waiting for pairing"}</code><br />Status: {selected.phase}</p>
      {selected.review && <MigrationReviewDetails review={selected.review} />}
      {selected.phase === "paired" && <p>Select accounts and create a review in Spanreed, then refresh this session.</p>}
      {selected.phase === "reviewing" && <>
        <label><input type="checkbox" checked={confirmed} disabled={busy || now >= selected.expiresAtMs} onChange={event=>setConfirmed(event.target.checked)} /> I recognize this installation and approve the exact accounts and destination above.</label>
        <button className="fb-button fb-button-primary" disabled={busy || !confirmed || !selected.revision || now >= selected.expiresAtMs} onClick={()=>void run(async()=>{if(!selected.revision)throw new Error("Review unavailable. Refresh this session.");await migrationApi.approve({id:selected.id,revision:selected.revision});await open(selected.id);await refresh();})}>Approve review</button>
      </>}
      {selected.phase === "approved" && <p role="status">Approved. Confirm and execute the transfer in Spanreed.</p>}
      {selected.phase === "completed" && <p role="status">API-key transfer completed. OAuth entries require independent authorization in the destination.</p>}
      {selected.phase === "completed" && selected.direction === "localToHosted" && selected.review?.items.filter(item=>item.action === "authorizeOAuth" && !authorized.includes(`${item.provider}/${item.targetAlias}`)).map(item=>(item.provider === "grok" || item.provider === "nous") && <div key={`${selected.id}/${item.sourceId}`}>{authorize({provider:item.provider,alias:item.targetAlias,migrationId:selected.id,sourceId:item.sourceId,onConnected:async()=>{setAuthorized(await migrationApi.authorizations({id:selected.id}));}})}</div>)}
      {authorized.map(id=><p role="status" key={id}>Connected {id} with a new hosted authorization.</p>)}
      {now >= selected.expiresAtMs && selected.phase !== "completed" && <p role="alert">This session expired.</p>}
      {selected.phase === "completed" && <div className="fb-form">
        {!forgetting ? <button className="fb-button" disabled={busy} onClick={()=>setForgetting(true)}>Forget receipt</button> : <>
          <p>Remove this migration receipt from ai-relay? Imported accounts remain connected. The reviewed aliases will no longer be available here for pending OAuth logins.</p>
          <div className="fb-row"><button className="fb-button" disabled={busy} onClick={()=>void run(async()=>{await migrationApi.forget({id:selected.id});setSelected(null);setForgetting(false);await refresh();})}>Forget this receipt</button><button className="fb-button" disabled={busy} onClick={()=>setForgetting(false)}>Keep receipt</button></div>
        </>}
      </div>}
      <div className="fb-row"><button className="fb-button" disabled={busy} onClick={()=>void run(async()=>open(selected.id))}>Refresh selected session</button>
      {selected.phase !== "completed" && selected.phase !== "cancelled" && <button className="fb-button" disabled={busy || now >= selected.expiresAtMs} onClick={()=>void run(async()=>{await migrationApi.cancel({id:selected.id});setInvitation(null);setSelected(null);setConfirmed(false);await refresh();})}>Cancel migration</button>}</div>
    </section>}
    {busy && <p role="status">Updating migration…</p>}
    {error && <p ref={errorRef} tabIndex={-1} className="fb-error" role="alert">{error}</p>}
  </div>;
}
