import * as React from "react";
import { invoke } from "@tauri-apps/api/core";

import type { ModelCatalog as Catalog } from "./contracts";
import type { AccountSummary as Account } from "./contracts";
export function ModelsPage({ accounts }: { accounts: Account[] }) {
  const supported = accounts.filter(account => ["grok", "codex", "nous", "openai"].includes(account.provider));
  const [accountId, setAccountId] = React.useState("");
  const [catalog, setCatalog] = React.useState<Catalog | null>(null);
  const [query, setQuery] = React.useState("");
  const [pending, setPending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const selected = supported.some(account => account.id === accountId);
  const models = catalog?.models.filter(model => model.toLowerCase().includes(query.toLowerCase())) ?? [];
  async function discover(event: React.FormEvent) {
    event.preventDefault();
    if (!selected || pending) return;
    setPending(true); setError(null);
    try { setCatalog(await invoke<Catalog>("models", {accountId})); }
    catch (error) { setError(String(error)); }
    finally { setPending(false); }
  }
  return <>
    <div className="fb-heading"><h1>Models</h1><p>Read the model catalog reported for a managed account. Listing a model does not guarantee access to every operation.</p></div>
    {!supported.length ? <div className="fb-empty"><h2>Connect an account first</h2><p>Add a managed Grok, Nous or OpenAI account in Accounts.</p></div> : <form className="fb-form" onSubmit={event => void discover(event)}>
      <label>Account<select required value={accountId} disabled={pending} onChange={event => {setAccountId(event.target.value);setCatalog(null);setError(null);}}><option value="">Choose an account</option>{supported.map(account => <option key={account.id} value={account.id}>{account.provider} / {account.alias}</option>)}</select></label>
      <button className="fb-button fb-button-primary" type="submit" disabled={!selected || pending}>{pending ? "Reading provider catalog…" : "Discover models"}</button>
    </form>}
    {error && <p role="alert" className="fb-error">{error}</p>}
    {catalog && <section>
      <p className="fb-muted">{catalog.accountId} · Last successful discovery: <time dateTime={new Date(catalog.observedAtMs).toISOString()}>{new Date(catalog.observedAtMs).toLocaleString()}</time></p>
      <label className="fb-form">Search models<input type="search" value={query} onChange={event => setQuery(event.target.value)} /></label>
      {!models.length ? <div className="fb-empty"><p>{catalog.models.length ? "No matching models." : "The provider returned an empty catalog."}</p></div> : <div className="fb-table-scroll" role="region" aria-label="Provider model catalog" tabIndex={0}><table className="fb-table"><caption>{models.length} matching model{models.length === 1 ? "" : "s"}</caption><thead><tr><th scope="col">Model identifier</th></tr></thead><tbody>{models.map(model => <tr key={model}><td>{model}</td></tr>)}</tbody></table></div>}
    </section>}
  </>;
}
