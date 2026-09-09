import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { ConsumptionView, type ConsumptionReport } from "@fabrials/ui";

interface Settings { additional_roots: Record<string, string[]>; disabled_clients: string[] }
interface Catalog { clients: {id: string; name: string; remote_collection: boolean}[]; settings: Settings; connections: string[] }

export function LocalUsage() {
  const [report,setReport] = React.useState<ConsumptionReport | null>(null);
  const [catalog,setCatalog] = React.useState<Catalog | null>(null);
  const [client,setClient] = React.useState("");
  const [days,setDays] = React.useState(31);
  const [model,setModel] = React.useState("");
  const [modelFilter,setModelFilter] = React.useState("");
  React.useEffect(()=>{const timer=setTimeout(()=>setModelFilter(model.trim()),350);return()=>clearTimeout(timer);},[model]);
  const [error,setError] = React.useState<string | null>(null);
  const [busy,setBusy] = React.useState(false);
  const [source,setSource] = React.useState("codex");
  const [roots,setRoots] = React.useState("");
  const [notice,setNotice] = React.useState("");
  const [remoteClient,setRemoteClient]=React.useState("cursor");
  const [account,setAccount]=React.useState("");
  const [credential,setCredential]=React.useState("");
  const generation = React.useRef(0);
  const refresh = React.useCallback(async (force = false) => {
    const current = ++generation.current;
    setBusy(true);setError(null);
    try {
      const result = await invoke<ConsumptionReport>("usage_report",{force,filter:{client:client || null,model:modelFilter || null,since_ms:Date.now()-days*86400000}});
      if (current === generation.current) setReport(result);
    } catch(error) {if(current === generation.current)setError(String(error));}
    finally {if(current === generation.current)setBusy(false);}
  },[client,days,modelFilter]);
  React.useEffect(()=>{void refresh();const timer=setInterval(()=>{if(!document.hidden)void refresh();},60000);return ()=>{clearInterval(timer);generation.current++;};},[refresh]);
  React.useEffect(()=>{void invoke<Catalog>("usage_sources").then(setCatalog).catch(error=>setError(String(error)));},[]);
  React.useEffect(()=>{setRoots((catalog?.settings.additional_roots[source] || []).join("\n"));},[catalog,source]);
  async function saveRoots() {
    if (!catalog) return;
    setBusy(true);setNotice("");
    try {
      const settings = {...catalog.settings,additional_roots:{...catalog.settings.additional_roots,[source]:roots.split("\n").map(root=>root.trim()).filter(Boolean)}};
      await invoke("save_usage_sources",{settings});setCatalog({...catalog,settings});setNotice("Source folders saved.");await refresh(true);
    } catch(error) {setError(String(error));} finally {setBusy(false);}
  }
  return <>
    <div className="fb-heading"><h1>Usage</h1><p>Consumption from your local clients. No proxy connection is required.</p></div>
    <div className="fb-consumption-filters">
      <label>Client <select value={client} onChange={event=>setClient(event.target.value)}><option value="">All clients</option>{catalog?.clients.map(client=><option key={client.id} value={client.id}>{client.name}</option>)}</select></label>
      <label>Period <select value={days} onChange={event=>setDays(Number(event.target.value))}><option value={7}>7 days</option><option value={31}>31 days</option><option value={90}>90 days</option><option value={365}>365 days</option></select></label>
      <label>Model <input value={model} onChange={event=>setModel(event.target.value)} placeholder="All models" /></label>
      <button className="fb-button" disabled={busy} onClick={()=>void refresh(true)}>{busy?"Reading usage…":"Refresh usage"}</button>
    </div>
    {error && <p role="alert" className="fb-error">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {report && <ConsumptionView report={report} />}
    {!report && !busy && !error && <p>No local usage found.</p>}
    <details className="fb-card"><summary className="fb-card-body">Connect usage reports</summary><form className="fb-card-body fb-form" onSubmit={event=>{
      event.preventDefault();setBusy(true);setError(null);
      void invoke("connect_usage_source",{connection:{client:remoteClient,account,credential}})
        .then(async()=>{setCredential("");setNotice("Usage connection saved on this machine.");setCatalog(await invoke<Catalog>("usage_sources"));await refresh(true);})
        .catch(error=>setError(String(error))).finally(()=>setBusy(false));
    }}>
      <p className="fb-muted">Connected reports: {catalog?.connections?.join(", ") || "None"}</p>
      <p>Read usage from an existing account. The credential stays on this machine and is not included in synchronization.</p>
      <label>Client <select value={remoteClient} onChange={event=>{setRemoteClient(event.target.value);setCredential("");}}><option value="cursor">Cursor</option><option value="trae">Trae</option><option value="warp">Warp</option></select></label>
      <label>Account label <input required value={account} onChange={event=>setAccount(event.target.value)} pattern="[A-Za-z0-9_.-]+" maxLength={128}/></label>
      <label>{remoteClient==="cursor"?"Cursor session cookie (WorkosCursorSessionToken)":remoteClient==="trae"?"Trae access token":"Warp access token"}<input required type="password" autoComplete="off" value={credential} onChange={event=>setCredential(event.target.value)}/></label>
      <button className="fb-button" disabled={busy}>Save connection and read usage</button>
      <button type="button" className="fb-button" disabled={busy} onClick={()=>{
        setBusy(true);void invoke("disconnect_usage_source",{client:remoteClient}).then(async()=>{setCredential("");setNotice("Connection and imported report cleared.");setCatalog(await invoke<Catalog>("usage_sources"));await refresh(true);}).catch(error=>setError(String(error))).finally(()=>setBusy(false));
      }}>Disconnect and clear imported report</button>
    </form></details>
    <details className="fb-card"><summary className="fb-card-body">Additional source folders</summary><div className="fb-card-body fb-form">
      <p>Default locations are detected automatically. Add other homes or archives here, one absolute path per line.</p>
      <label>Client <select value={source} onChange={event=>setSource(event.target.value)}>{catalog?.clients.map(client=><option key={client.id} value={client.id}>{client.name}</option>)}</select></label>
      <label><span><input type="checkbox" checked={!catalog?.settings.disabled_clients.includes(source)} onChange={event=>{if(catalog)setCatalog({...catalog,settings:{...catalog.settings,disabled_clients:event.target.checked?catalog.settings.disabled_clients.filter(id=>id!==source):[...new Set([...catalog.settings.disabled_clients,source])]}});}}/> Collect this source</span></label>
      <label style={{display:"block",marginBlock:12}}>Folders<textarea rows={4} style={{display:"block",width:"100%",boxSizing:"border-box"}} value={roots} onChange={event=>setRoots(event.target.value)} /></label>
      <button className="fb-button" disabled={busy || !catalog} onClick={()=>void saveRoots()}>Save folders</button>
    </div></details>
  </>;
}
