import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Label, NativeSelect } from "@fabrials/ui";
import { Done, ErrorAlert } from "./feedback";
import { useModelCatalog } from "./models";
import type { Preview } from "./contracts";

type Client = "opencode" | "grok";
type Operation = "create" | "update" | "remove";

export function ClientConfiguration({ provider, alias, accountId }: { provider: string; alias: string; accountId: string }) {
  const [client, setClient] = React.useState<Client>("opencode");
  const [operation, setOperation] = React.useState<Operation>("create");
  const [model, setModel] = React.useState("");
  const [review, setReview] = React.useState<Preview | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  const needsModel = client === "grok" || operation !== "remove";
  const { catalog, error: catalogError } = useModelCatalog(needsModel ? accountId : null);
  React.useEffect(() => { setModel(""); setReview(null); }, [accountId, client]);
  async function preview() {
    setBusy(true); setError(null); setNotice(null); setReview(null);
    try {
      const command = client === "grok" ? "preview_grok_configuration" : operation === "remove" ? "preview_opencode_remove" : operation === "update" ? "preview_opencode_update" : "preview_opencode_configuration";
      setReview(await invoke<Preview>(command, client === "grok" ? { alias, model: model.trim() } : { provider, alias, model: model.trim() }));
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  async function apply() {
    if (!review) return;
    setBusy(true); setError(null);
    try {
      const backup = await invoke<string | null>("apply_client_configuration", { id: review.id });
      const instruction = review.operation === "remove" ? "Connection removed. Restart OpenCode to refresh its providers."
        : review.client === "grok" ? `Grok Build now uses this account${model.trim() ? ` and ${model.trim()}` : ""}. Start a new Grok session. Other endpoint overrides may take precedence.`
        : `Connection ${review.operation === "update" ? "updated" : "added"}. Restart OpenCode and pick ${review.providerId}/${model.trim()} in /models.`;
      setNotice([...review.warnings, instruction, backup ? `Your previous file was backed up to ${backup}.` : ""].filter(Boolean).join(" "));
      setReview(null);
    } catch (error) { setError(String(error)); setReview(null); }
    finally { setBusy(false); }
  }
  return <section className="sr-form" aria-labelledby="client-configuration-title">
    <h3 id="client-configuration-title" className="sr-subsection-title">Write the tool's settings for you</h3>
    <p className="fui-description">Spanreed shows you the exact change first and backs up the file before saving.</p>
    <div className="sr-field-row">
      <Label>Tool<NativeSelect value={client} disabled={busy || !!review} onChange={(event) => { setClient(event.target.value as Client); setError(null); setNotice(null); }}>
        <option value="opencode">OpenCode</option>{provider === "grok" && <option value="grok">Grok Build</option>}
      </NativeSelect></Label>
      {client === "opencode" && <Label>Change<NativeSelect value={operation} disabled={busy || !!review} onChange={(event) => setOperation(event.target.value as Operation)}>
        <option value="create">Add a connection</option><option value="update">Update the existing connection</option><option value="remove">Remove the connection</option>
      </NativeSelect></Label>}
    </div>
    <p className="fui-description">{client === "grok" ? "Points Grok Build's chat endpoint at this account and sets the model you choose as its default. Other settings and TOML comments are kept."
      : operation === "remove" ? "Removes the connection Spanreed added. Change any default model that uses it first."
      : operation === "update" ? "Refreshes the address and model of the connection Spanreed added. Your default model stays as it is."
      : "Adds a separate provider for this account. Your default model stays as it is."}</p>
    {needsModel && (catalog?.models.length
      ? <Label>Model<NativeSelect value={catalog.models.includes(model) ? model : ""} disabled={busy || !!review} onChange={(event) => { setModel(event.target.value); setNotice(null); }}>
          <option value="">Choose a model</option>{catalog.models.map((id) => <option key={id} value={id}>{id}</option>)}
        </NativeSelect></Label>
      : !catalogError && <p className="fui-description">{catalog ? "This account hasn't reported any models yet. Check it under Accounts, then come back and choose one." : "Reading this account's models…"}</p>)}
    {catalogError && <ErrorAlert title="Couldn't read the model list" error={catalogError} />}
    <ErrorAlert title="Couldn't prepare the change" error={error} />
    <Done>{notice}</Done>
    {!review ? <div className="fui-actions"><Button variant="outline" disabled={busy || (needsModel && !model.trim())} onClick={() => void preview()}>{busy ? "Preparing…" : "Review change"}</Button></div>
      : <div className="sr-review">
        {review.warnings.map((warning) => <p key={warning} className="sr-warning">{warning}</p>)}
        <dl className="sr-facts">
          <dt>File</dt><dd><code>{review.path}</code></dd>
          <dt>{review.operation === "remove" ? "Removes" : review.operation === "update" ? "Updates" : "Adds"}</dt><dd><code>{review.providerId}</code></dd>
        </dl>
        {review.operation !== "remove" && <pre className="sr-code">{JSON.stringify(review.addition, null, 2)}</pre>}
        <p className="fui-description">This review expires in five minutes.</p>
        <div className="fui-actions"><Button disabled={busy} onClick={() => void apply()}>{busy ? "Saving…" : "Apply change"}</Button><Button variant="outline" disabled={busy} onClick={() => setReview(null)}>Cancel</Button></div>
      </div>}
  </section>;
}
