import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SessionMoveView } from "./contracts";

export function CodexSessionMove({owner,onChanged}:{owner:string;onChanged:()=>Promise<void>}) {
  const [view,setView]=React.useState<SessionMoveView|null>(null);
  const [alias,setAlias]=React.useState("codex-desktop");
  const [busy,setBusy]=React.useState(false);
  const [error,setError]=React.useState<string|null>(null);
  const [reviewed,setReviewed]=React.useState(false);
  const sequence=React.useRef(0);
  React.useEffect(()=>{let active=true;void invoke<SessionMoveView|null>("codex_session_move",{owner,operation:"current"}).then(value=>{if(active)setView(value);}).catch(error=>{if(active)setError(String(error));});return()=>{active=false;sequence.current++;};},[owner]);
  async function run(operation:string) {
    const current=++sequence.current;setBusy(true);setError(null);
    try {
      const value=await invoke<SessionMoveView|null>("codex_session_move",{owner,operation,id:view?.id??null,alias});
      if(current!==sequence.current)return;setView(value);setReviewed(false);
      if(value?.state==="completed")await onChanged();
    } catch(error) {
      if(current!==sequence.current)return;setError(String(error));
      try {const value=await invoke<SessionMoveView|null>("codex_session_move",{owner,operation:"current"});if(current===sequence.current)setView(value);}catch{/* Keep the last recoverable view. */}
    } finally {if(current===sequence.current)setBusy(false);}
  }
  const terminal=view?.state==="completed"||view?.state==="cancelled";
  return <section className="fb-card"><header><h2>Move a local Codex session</h2><p>Optional alternative to independent hosted authorization. Move refresh ownership to ai-relay and configure Codex to use a private, account-scoped proxy key.</p></header><div className="fb-card-body fb-form">
    <p>Close Codex, OpenCode and other Spanreed processes first. File-backed sessions with one known local owner can be moved. System credential stores and ambiguous copies require independent authorization.</p>
    {!view ? <><label>Hosted account name<input value={alias} maxLength={40} onChange={event=>setAlias(event.target.value)} disabled={busy}/></label><button className="fb-button" disabled={busy||!alias.trim()} onClick={()=>void run("preview")}>Review session move</button></> : <>
      <p role="status">{view.state==="completed"?"Hosted ownership confirmed. Codex now uses ai-relay.":view.state==="cancelled"?"Move cancelled. Original local files restored.":`Saved move: ${view.state}`}</p>
      <dl><dt>Authorization to retire</dt><dd style={{overflowWrap:"anywhere"}}>{view.source_path}</dd><dt>Configuration to update</dt><dd style={{overflowWrap:"anywhere"}}>{view.config_path}</dd><dt>New Responses endpoint</dt><dd style={{overflowWrap:"anywhere"}}>{view.endpoint}</dd></dl>
      {view.state==="prepared" && <><label className="fb-row"><input type="checkbox" checked={reviewed} onChange={event=>setReviewed(event.target.checked)} disabled={busy}/>I have closed the clients and removed any other copies of this OAuth session. Move it to ai-relay and update my Codex configuration.</label><p>Existing model and unrelated settings are preserved. Profile, project or command-line provider overrides must be updated separately. The proxy key stays in the private local configuration.</p><button className="fb-button fb-button-primary" disabled={busy||!reviewed} onClick={()=>void run("apply")}>Move session and configure Codex</button></>}
      {!terminal && <div className="fb-row"><button className="fb-button" disabled={busy} onClick={()=>void run("recover")}>Check hosted receipt</button><button className="fb-button" disabled={busy} onClick={()=>void run("cancel")}>Cancel and restore safely</button></div>}
      {terminal && <button className="fb-button" disabled={busy} onClick={()=>void run("dismiss")}>Dismiss receipt</button>}
      {!terminal && <p className="fb-muted">If a request is interrupted, keep this recovery record. Restoration waits for the server to confirm that no delayed import can claim the session.</p>}
    </>}
    {error&&<p role="alert" className="fb-error">{error}</p>}
  </div></section>;
}
