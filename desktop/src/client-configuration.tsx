import * as React from "react";
import { invoke } from "@tauri-apps/api/core";

import type { Preview } from "./contracts";
export function ClientConfiguration({provider, alias}: {provider: string; alias: string}) {
  const [client, setClient] = React.useState<"opencode" | "grok">("opencode");
  const [operation, setOperation] = React.useState<"create" | "update" | "remove">("create");
  const update = operation === "update";
  const remove = operation === "remove";
  const [model, setModel] = React.useState("");
  const [review, setReview] = React.useState<Preview | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  async function preview() {
    setBusy(true);setError(null);setNotice(null);setReview(null);
    try { setReview(await invoke<Preview>(client === "grok" ? "preview_grok_configuration" : remove ? "preview_opencode_remove" : update ? "preview_opencode_update" : "preview_opencode_configuration", client === "grok" ? {alias} : {provider, alias, model:model.trim()})); }
    catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  async function apply() {
    if (!review) return;
    setBusy(true);setError(null);
    try {
      const backup = await invoke<string | null>("apply_client_configuration", {id:review.id});
      const instruction = review.operation === "remove" ? "Connection removed. Restart OpenCode to refresh the available providers." : review.client === "grok" ? "Grok Build endpoint saved. Start a new Grok session to use this account. Other endpoint overrides may take precedence." : `Connection added. Restart OpenCode and select ${review.providerId}/${model.trim()} in /models.`;
      setNotice(`${review.warnings.join(" ")}${review.warnings.length ? " " : ""}${review.operation === "update" && review.client === "opencode" ? instruction.replace("Connection added.", "Connection updated.") : instruction}${backup ? ` Previous configuration saved to ${backup}.` : ""}`);
      setReview(null);
    } catch (error) { setError(String(error));setReview(null); }
    finally { setBusy(false); }
  }
  return <section className="fb-form" aria-label="Client configuration">
    <h3>Save a client connection</h3>
    <label>Client<select value={client} disabled={busy || !!review} onChange={event=>{setClient(event.target.value as "opencode" | "grok");setError(null);setNotice(null);}}><option value="opencode">OpenCode</option>{provider === "grok" && <option value="grok">Grok Build</option>}</select></label>
    <p className="fb-muted">{client === "grok" ? "Update the Grok Build chat endpoint for this account. Existing model settings and TOML comments are preserved." : remove ? "Remove this generated connection. Change any default model references first." : update ? "Update the endpoint and model list of this Spanreed connection. Your default model stays as configured." : "Add a separate connection for this account. Your default model stays as configured."} Review the changes before saving.</p>
    {client === "opencode" && <label>Operation<select value={operation} disabled={busy || !!review} onChange={event=>setOperation(event.target.value as typeof operation)}><option value="create">Create connection</option><option value="update">Update existing connection</option><option value="remove">Remove connection</option></select></label>}
    {client === "opencode" && !remove && <label>Model ID<input value={model} disabled={busy || !!review} maxLength={256} onChange={event=>{setModel(event.target.value);setNotice(null);}} placeholder="Use an ID from the model catalog" /></label>}
    {!review && <button className="fb-button" disabled={busy || (client === "opencode" && !remove && !model.trim())} onClick={()=>void preview()}>{busy ? "Preparing…" : "Review configuration"}</button>}
    {review && <div className="fb-form">
      {review.warnings.length > 0 && <div role="status">{review.warnings.map(warning => <p key={warning}>{warning}</p>)}</div>}
      <p>File: <code style={{overflowWrap:"anywhere"}}>{review.path}</code></p>
      <p>{review.operation === "remove" ? "Remove" : review.operation === "update" ? "Update" : "Create"} provider: <strong>{review.providerId}</strong></p>{review.client === "opencode" && review.operation === "update" && <p>This replaces the connection endpoint and model list with the values below.</p>}
      {review.operation !== "remove" && <pre style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{JSON.stringify(review.addition,null,2)}</pre>}
      <p className="fb-muted">A private backup is saved before changing an existing file. This review expires in five minutes.</p>
      <div className="fb-row"><button className="fb-button fb-button-primary" disabled={busy} onClick={()=>void apply()}>{busy ? "Saving…" : "Apply configuration"}</button><button className="fb-button" disabled={busy} onClick={()=>setReview(null)}>Cancel</button></div>
    </div>}
    {error && <p className="fb-error" role="alert">{error}</p>}
    {notice && <p role="status" style={{overflowWrap:"anywhere"}}>{notice}</p>}
  </section>;
}
