import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { RoutingExplanation, RoutingPolicyForm } from "@fabrials/ui";

import type { RoutingSnapshot } from "./contracts";

export function RoutingPage() {
  const [data, setData] = React.useState<RoutingSnapshot | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [pending, setPending] = React.useState(false);
  const load = React.useCallback(async () => {
    try { setData(await invoke<RoutingSnapshot>("routing")); setError(null); }
    catch (error) { setError(String(error)); }
  }, []);
  React.useEffect(() => { void load(); }, [load]);
  async function save(provider: string, on: boolean, threshold: number) {
    setPending(true); setNotice(null); setError(null);
    try {
      await invoke("set_routing", {provider, on, threshold});
      setData(await invoke<RoutingSnapshot>("routing"));
      setNotice(`${provider} routing preferences saved. Changes apply to the next request.`);
    } catch (error) { setError(String(error)); }
    finally { setPending(false); }
  }
  return <>
    <div className="fb-heading"><h1>Autosteer</h1><p>Choose how unpinned requests select an account on this machine.</p></div>
    <RoutingExplanation />
    {error && <p className="fb-error" role="alert">{error} <button className="fb-button" disabled={pending} onClick={() => void load()}>Retry</button></p>}
    {notice && <p role="status">{notice}</p>}
    {!data && !error && <p role="status">Reading routing preferences…</p>}
    <div className="fb-grid">{data?.policies.map(policy => {
      const pool = data.limits.providers.find(pool => pool.provider === policy.provider);
      return <article className="fb-card" key={policy.provider}>
        <header><h2>{policy.provider}</h2><p className="fb-muted">{pool?.route}</p></header>
        <div className="fb-card-body">
          <RoutingPolicyForm key={`${policy.autosteer}:${policy.exhausted_pct}`} provider={policy.provider} enabled={policy.autosteer} threshold={policy.exhausted_pct} pending={pending} onSave={(on, threshold) => save(policy.provider, on, threshold)} />
          {pool?.accounts.length ? <ul>{pool.accounts.map(account => <li key={account.alias}>
            <strong>{account.alias}</strong> · {account.role === "next" ? "Next account" : account.role === "active" ? "Active account" : "Available account"}
            <p className="fb-muted">{account.plan || "Plan not reported"} · {account.used_pct == null ? "Quota unknown" : `${account.used_pct.toFixed(1)}% used`}{policy.autosteer && account.exhausted ? " · Skipped at current threshold" : ""}</p>
          </li>)}</ul> : <p className="fb-muted">No managed accounts. Add an account to use automatic selection.</p>}
        </div>
      </article>;
    })}</div>
  </>;
}
