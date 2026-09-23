import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Input, Label, NativeSelect } from "@fabrials/ui";
import { Done, ErrorAlert } from "./feedback";
import type { HostedClientReview } from "./contracts";
import type { AccountView } from "./relay-contracts";

export function HostedClientConfiguration({ owner, accounts }: { owner: string; accounts: AccountView[] }) {
  const codex = accounts.filter((account) => account.provider === "codex");
  const [alias, setAlias] = React.useState("");
  const selected = codex.find((account) => account.alias === alias)?.alias ?? codex[0]?.alias ?? "";
  const [client, setClient] = React.useState("codex");
  const [key, setKey] = React.useState("");
  const [model, setModel] = React.useState("");
  const [review, setReview] = React.useState<HostedClientReview | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  async function prepare() {
    setBusy(true); setError(null); setNotice(null);
    try { setReview(await invoke<HostedClientReview>("preview_hosted_client", { owner, client, alias: selected, key: key.trim(), model: model.trim() })); setKey(""); }
    catch (error) { setError(String(error)); } finally { setBusy(false); }
  }
  async function apply() {
    if (!review) return;
    setBusy(true); setError(null);
    try { setNotice(await invoke<string>("apply_hosted_client", { owner, id: review.id })); setReview(null); }
    catch (error) { setError(String(error)); setReview(null); } finally { setBusy(false); }
  }
  return <Card>
    <CardHeader><CardTitle>Use hosted Codex in a tool</CardTitle><CardDescription>Point Codex or OpenCode on this computer at a Codex account signed in on the hosted relay.</CardDescription></CardHeader>
    <CardContent className="sr-form">
      {!codex.length ? <p className="fui-description">Sign in to a Codex account under Accounts first.</p> : !review ? <>
        <div className="sr-field-row">
          <Label>Tool<NativeSelect disabled={busy} value={client} onChange={(event) => setClient(event.target.value)}><option value="codex">Codex</option><option value="opencode">OpenCode</option></NativeSelect></Label>
          <Label>Hosted account<NativeSelect disabled={busy} value={selected} onChange={(event) => setAlias(event.target.value)}>{codex.map((account) => <option key={account.id} value={account.alias}>{account.alias}</option>)}</NativeSelect></Label>
        </div>
        <Label>Model ID<Input disabled={busy} value={model} onChange={(event) => setModel(event.target.value)} placeholder="Pick one from Models below" maxLength={256} /></Label>
        <Label>Proxy key<Input type="password" autoComplete="off" disabled={busy} value={key} onChange={(event) => setKey(event.target.value)} maxLength={512} /></Label>
        <p className="fui-description">Create a key under Proxy keys that allows this account, the codex provider and route, and chat and models requests. The key is saved only in the tool's private settings.</p>
        <div className="fui-actions"><Button variant="outline" disabled={busy || !model.trim() || !key.trim()} onClick={() => void prepare()}>Review change</Button></div>
      </> : <div className="sr-review">
        <dl className="sr-facts">
          <dt>File</dt><dd><code>{review.path}</code></dd>
          <dt>Account</dt><dd>{review.account_id}</dd>
          <dt>Model</dt><dd><code>{review.model}</code></dd>
          <dt>Endpoint</dt><dd><code>{review.endpoint}</code></dd>
        </dl>
        <p className="fui-description">{review.client === "codex"
          ? "Sets this model and the Fabrials provider as Codex defaults. Profile, project and command-line overrides may still take precedence. Your local sign-in stays available to other settings."
          : "Adds a separate OpenCode provider and keeps your default model and other providers. Its default output-token cap is turned off because Codex subscriptions can't enforce it."} The file is backed up before it changes. This review expires in five minutes.</p>
        <div className="fui-actions"><Button disabled={busy} onClick={() => void apply()}>Apply change</Button><Button variant="outline" disabled={busy} onClick={() => setReview(null)}>Cancel</Button></div>
      </div>}
      <ErrorAlert title="Couldn't prepare the change" error={error} />
      <Done>{notice}</Done>
    </CardContent>
  </Card>;
}
