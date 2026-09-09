import { PrivateHistory } from "./private-history";
import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PublicationStatus, SyncSettings, SyncStatus } from "./contracts";
export function SharingControls({sources,enabled,connected}:{sources:string[];enabled:boolean;connected:boolean}) {
  const [selection,setSelection]=React.useState<SyncSettings>({sources:[]});
  const [sync,setSync]=React.useState<SyncStatus|null>(null);
  const [publication,setPublication]=React.useState<PublicationStatus|null>(null);
  const [busy,setBusy]=React.useState(false);
  const [error,setError]=React.useState<string|null>(null);
  const [notice,setNotice]=React.useState<string|null>(null);
  const load=React.useCallback(async()=>{
    const [selection,status,publication]=await Promise.all([invoke<SyncSettings>("sync_settings"),invoke<SyncStatus>("sync_status"),invoke<PublicationStatus>("publication_status")]);
    setSelection(selection);setSync(status);setPublication(publication);
  },[]);
  React.useEffect(()=>{void load().catch(error=>setError(String(error)));},[load]);
  async function perform(command:string,args?:Record<string,unknown>){
    setBusy(true);setError(null);setNotice(null);
    try{const result=await invoke<unknown>(command,args);setNotice(typeof result==="string"?result:"Saved synchronization sources.");await load();}catch(error){setError(String(error));}finally{setBusy(false);}
  }
  const choices=[...new Set([...sources,...selection.sources])].sort();
  return <><section className="fb-card"><header><h2>Private synchronization</h2><p className="fb-muted">Choose exactly which local sources may upload quotas, local daily usage and available request metadata. Provider credentials and request bodies stay private.</p></header><div className="fb-card-body fb-form">
    {choices.map(source=><label key={source}><span><input type="checkbox" disabled={busy} checked={selection.sources.includes(source)} onChange={event=>setSelection({sources:event.target.checked?[...selection.sources,source]:selection.sources.filter(value=>value!==source)})}/>{source}</span></label>)}
    {!choices.length&&<p>No local sources found. Connect a provider or refresh Overview.</p>}
    <p className="fb-muted">Only reported data is synchronized. Downloaded observations stay separate from local billable request totals.</p>
    <p role="status">{!connected ? "Connect this installation to Fabrials above." : !enabled ? "Enable private history synchronization and save your sharing preferences above." : !selection.sources.length ? "Select at least one source to synchronize." : "Ready to synchronize the selected sources."}</p>
    <button className="fb-button" disabled={busy || !connected} onClick={()=>void perform("save_sync_settings",{settings:selection})}>Save selected sources</button>
    <button className="fb-button fb-button-primary" disabled={busy || !connected || !enabled || !selection.sources.length} onClick={async()=>{setBusy(true);setError(null);setNotice(null);try{await invoke("save_sync_settings",{settings:selection});const result=await invoke<string>("sync_now");setNotice(result);await load();}catch(error){setError(String(error));}finally{setBusy(false);}}}>Save sources and synchronize now</button>
    {selection.sources.includes("codex") && <details><summary>Match Codex with its hosted account</summary><p className="fb-muted">Authorize the same Codex account on ai-relay, then verify its identity here. This sends signed OpenAI identity information once, including profile claims. Access and refresh tokens are not sent.</p><button className="fb-button" disabled={busy || !enabled || !connected} onClick={()=>void perform("link_codex_source")}>Verify identity match</button></details>}
    <p>Last successful synchronization: {sync?.lastSuccessMs?new Date(sync.lastSuccessMs).toLocaleString():"Never"}</p>{sync?.error&&<p className="fb-error">{sync.error}</p>}
    {sync?.lastSuccessMs&&<p className="fb-muted">Last run: {sync.uploaded} uploaded · {sync.downloaded} downloaded</p>}
  </div></section><div className="desktop-settings-history"><PrivateHistory/></div><section className="fb-card"><header><h2>Public metrics publication</h2><p className="fb-muted">Uses your saved publication consent and Fabrials account. Automatic publication runs once per product day.</p></header><div className="fb-card-body fb-form">
    <p>Last publication: {publication?.lastSharedDay??"Never"}</p><p className="fb-muted">{publication?.schedule}</p>
    <button className="fb-button fb-button-primary" disabled={busy} onClick={()=>void perform("publish_metrics")}>Publish metrics now</button>
    <button className="fb-button" disabled={busy} onClick={()=>void perform("set_publication_schedule",{enabled:true})}>Enable daily publication</button>
    <button className="fb-button" disabled={busy} onClick={()=>void perform("set_publication_schedule",{enabled:false})}>Disable daily publication</button>
  </div></section>{error&&<p className="fb-error" role="alert">{error}</p>}{notice&&<p role="status">{notice}</p>}</>;
}
