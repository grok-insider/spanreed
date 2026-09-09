import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import type { HostedClientReview } from "./contracts";
import type { AccountView } from "./relay-contracts";
export function HostedClientConfiguration({owner,accounts}:{owner:string;accounts:AccountView[]}) {
  const codex=accounts.filter(a=>a.provider==="codex");
  const [alias,setAlias]=React.useState("");const selected=codex.find(a=>a.alias===alias)?.alias??codex[0]?.alias??"";
  const [client,setClient]=React.useState("codex");const [key,setKey]=React.useState("");const [model,setModel]=React.useState("");
  const [review,setReview]=React.useState<HostedClientReview|null>(null);const [busy,setBusy]=React.useState(false);const [error,setError]=React.useState<string|null>(null);const [notice,setNotice]=React.useState<string|null>(null);
  async function prepare(){setBusy(true);setError(null);setNotice(null);try{setReview(await invoke<HostedClientReview>("preview_hosted_client",{owner,client,alias:selected,key:key.trim(),model:model.trim()}));setKey("");}catch(error){setError(String(error));}finally{setBusy(false);}}
  async function apply(){if(!review)return;setBusy(true);setError(null);try{setNotice(await invoke<string>("apply_hosted_client",{owner,id:review.id}));setReview(null);}catch(error){setError(String(error));setReview(null);}finally{setBusy(false);}}
  return <section className="fb-card"><header><h2>Connect a client to hosted Codex</h2><p>Use your independently authorized hosted account with Codex or OpenCode.</p></header><div className="fb-card-body fb-form">
    {!codex.length?<p>Authorize a Codex account in Accounts first.</p>:!review?<>
      <label>Client<select disabled={busy} value={client} onChange={event=>setClient(event.target.value)}><option value="codex">Codex</option><option value="opencode">OpenCode</option></select></label>
      <label>Hosted account<select disabled={busy} value={selected} onChange={event=>setAlias(event.target.value)}>{codex.map(a=><option key={a.id} value={a.alias}>{a.alias}</option>)}</select></label>
      <label>Model ID<input disabled={busy} value={model} onChange={event=>setModel(event.target.value)} placeholder="Choose an ID from Models" maxLength={256}/></label>
      <label>ai-relay proxy key<input type="password" autoComplete="off" disabled={busy} value={key} onChange={event=>setKey(event.target.value)} maxLength={512}/></label><p className="fb-muted">Create a key in Proxy keys with access to this account, the codex provider and route, and chat and models. The key is saved only in the private client configuration.</p>
      <button className="fb-button" disabled={busy||!model.trim()||!key.trim()} onClick={()=>void prepare()}>Review client configuration</button>
    </>:<><p>File: <code style={{overflowWrap:"anywhere"}}>{review.path}</code></p><p>Account: {review.account_id} · Model: {review.model}</p><p>Endpoint: <code style={{overflowWrap:"anywhere"}}>{review.endpoint}</code></p><p>{review.client==="codex"?"Set the selected model and Fabrials provider as the Codex defaults. Profile, project and command-line overrides may take precedence. Local OAuth stays available to other configurations.":"Add a separate OpenCode provider. Keep the current default model and other providers."} A private backup is saved before replacing an existing file. This review expires in five minutes.</p><div className="fb-row"><button className="fb-button fb-button-primary" disabled={busy} onClick={()=>void apply()}>Apply configuration</button><button className="fb-button" disabled={busy} onClick={()=>setReview(null)}>Cancel</button></div></>}
    {error&&<p role="alert" className="fb-error">{error}</p>}{notice&&<p role="status">{notice}</p>}
  </div></section>;
}
