import { ProviderCard, useClock } from "./index";
import type { SynchronizedAccount } from "./contracts";

export function SynchronizedAccounts({ accounts: allAccounts, includeLinked = false }: { accounts: SynchronizedAccount[]; includeLinked?: boolean }) {
  const accounts=allAccounts.filter(account=>includeLinked || !account.linked_account_id);
  const now = useClock();
  if (!accounts.length) return null;
  return <section aria-label="Synchronized accounts" className="fb-synchronized-accounts">
    {!includeLinked && <header><h2>Accounts from your machines</h2><p className="fb-muted">Your synchronized usage. Authorize a provider on ai-relay to use it through the hosted proxy.</p></header>}
    <div className={includeLinked ? "fb-synchronized-linked" : "fb-synchronized-grid"}>{accounts.map(account => <div key={`${account.device}:${account.source}`} className="fb-synchronized-account">
      <div className="fb-row"><strong>{account.source}</strong><span>{account.linked_account_id ? "Verified identity match" : "Usage synchronized"}</span></div>
      <p className="fb-muted">Installation {account.device.slice(0, 8)} · {now !== null && now - account.observed_at_ms > 900_000 ? "Last known reading · " : ""}<time dateTime={new Date(account.observed_at_ms).toISOString()}>{new Date(account.observed_at_ms).toLocaleString()}</time></p>
      <ProviderCard provider={account.output}/>
      {account.local_usage && <section className="fb-card"><header><h3>Local log history</h3><p className="fb-muted">These logs can contain several accounts and overlap relay requests. Totals are kept separate from relay traffic.</p></header><div className="fb-card-body">
        <p>{account.local_usage.days.reduce((sum,day)=>sum+day.tokens,0).toLocaleString()} tokens · ~${account.local_usage.days.reduce((sum,day)=>sum+day.estimated_usd,0).toFixed(2)} API list-price estimate{account.local_usage.partial ? " (partial)" : ""}</p>
        <details><summary>Daily usage</summary><table><thead><tr><th>Date</th><th>Tokens</th><th>Estimated cost</th></tr></thead><tbody>{account.local_usage.days.map(day=><tr key={day.date}><td>{day.date}</td><td>{day.tokens.toLocaleString()}</td><td>${day.estimated_usd.toFixed(2)}</td></tr>)}</tbody></table></details>
      </div></section>}
    </div>)}</div>
  </section>;
}

export function LinkedAccountUsage({accounts}:{accounts:SynchronizedAccount[]}) {
  if (!accounts.length) return null;
  return <details className="fb-linked-account-usage"><summary>Usage from {accounts.length} local {accounts.length === 1 ? "source" : "sources"}</summary><SynchronizedAccounts accounts={accounts} includeLinked/></details>;
}
