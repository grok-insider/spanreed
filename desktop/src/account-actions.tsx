import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { ApiKeyFields } from "@fabrials/ui";

import type { AccountSummary as Account } from "./contracts";
export function AccountActions({ account, onChanged }: { account: Account; onChanged: () => Promise<void> }) {
  const [action, setAction] = React.useState<"key" | "remove" | null>(null);
  const [key, setKey] = React.useState("");
  const [pending, setPending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (pending || !action) return;
    setPending(true); setError(null); setNotice(null);
    try {
      await invoke(action === "key" ? "replace_api_key" : "remove_account", {
        id: account.id, generation: account.generation ?? null,
        ...(action === "key" ? {key: key.trim()} : {}),
      });
      setKey("");setAction(null);
      setNotice(action === "key" ? "Key replaced locally. It has not been verified with the provider." : "Account removed locally.");
      await onChanged();
    } catch (error) { setError(String(error)); }
    finally { setPending(false); }
  }
  function select(next: "key" | "remove" | null) { setAction(next); setKey(""); setError(null); setNotice(null); }
  return <div className="fb-form">
    {!action && <div className="fb-row">
      {(account.provider === "openai" || account.provider === "nous") && <button className="fb-button" onClick={()=>select("key")}>Replace API key</button>}
      <button className="fb-button" onClick={()=>select("remove")}>Remove account</button>
    </div>}
    {action && <form className="fb-form" onSubmit={event=>void submit(event)}>
      {action === "key" ? <><p>Replace the saved API key for {account.alias}. OAuth accounts require Authorize again. Requests use the provider’s API billing.</p>
        <ApiKeyFields provider={account.provider} alias={account.alias} secret={key} lockedIdentity pending={pending} onProviderChange={()=>{}} onAliasChange={()=>{}} onSecretChange={setKey} />
      </> : <p>Remove {account.alias} and its saved credentials from Spanreed? Usage history stays on this machine. This does not revoke access at the provider. If this account is active, another account for the same provider becomes active.</p>}
      <div className="fb-row"><button className="fb-button fb-button-primary" disabled={pending} type="submit">{pending ? "Saving…" : action === "key" ? "Replace saved key" : "Remove from Spanreed"}</button><button className="fb-button" type="button" disabled={pending} onClick={()=>select(null)}>Cancel</button></div>
    </form>}
    {error && <p className="fb-error" role="alert">{error}</p>}{notice && <p role="status">{notice}</p>}
  </div>;
}
