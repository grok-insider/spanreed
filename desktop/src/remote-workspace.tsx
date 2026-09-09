import { HostedClientConfiguration } from "./hosted-client-configuration";
import { CodexSessionMove } from "./codex-session-move";
import { PrivateHistory } from "./private-history";
import { RemoteMigration } from "./remote-migration";
import * as React from "react";
import { ApiKeyForm, ProviderIcon, ProviderCard, SynchronizedAccounts, LinkedAccountUsage, type SynchronizedAccount, BalanceCard, ResetInventory, RoutingExplanation } from "@fabrials/ui";
import { RemoteDeviceLogin } from "./remote-device-login";
import { FabrialsLink } from "./fabrials-link";
import { loadDashboard, boundRemote, RemoteContext, useRemote } from "./remote-api";
import type { AccountView, DashboardPayload, IssuedKey, KeyView, KeyPolicyRequest } from "./relay-contracts";
import type { RemoteOperation } from "./contracts";

export const remoteNavigation = [{id:"overview",label:"Overview"},{id:"accounts",label:"Accounts"},{id:"routing",label:"Autosteer"},{id:"keys",label:"Proxy keys"},{id:"models",label:"Models"},{id:"requests",label:"Requests"},{id:"history",label:"Private history"},{id:"connections",label:"Connections"},{id:"migration",label:"Migration"},{id:"settings",label:"Settings"}];
const money = (value: number) => new Intl.NumberFormat("en-US", {style:"currency",currency:"USD",maximumFractionDigits:4}).format(value);

export function RemoteWorkspace({ page }: { page: string }) {
  const [data, setData] = React.useState<DashboardPayload | null>(null);
  const [days, setDays] = React.useState(7);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [issuedKey, setIssuedKey] = React.useState<string | null>(null);
  const [query, setQuery] = React.useState("");
  const api = React.useMemo(()=>boundRemote(data?.session.owner),[data?.session.owner]);
  const sequence = React.useRef(0);
  const loadedOwner = React.useRef<string | undefined>(undefined);
  const load = React.useCallback(async () => {
    const current = ++sequence.current;
    setBusy(true); setError(null);
    try { const next = await loadDashboard<DashboardPayload>(days); if (current === sequence.current) { if (loadedOwner.current !== next.session.owner) setIssuedKey(null); loadedOwner.current = next.session.owner; setData(next); } }
    catch (error) { if (current === sequence.current) { setData(null); setIssuedKey(null); setError(String(error)); } }
    finally { if (current === sequence.current) setBusy(false); }
  }, [days]);
  React.useEffect(() => { void load(); return () => { sequence.current++; }; }, [load]);
  async function mutate(operation: RemoteOperation, body: object) { await api(operation, body); await load(); }
  if (page === "settings") return <FabrialsLink onChange={view => {if (view.state !== "linked") {setData(null);setIssuedKey(null);} else void load();}} />;
  return <RemoteContext.Provider key={data?.session.owner} value={api}><div className="fb-heading"><h1>ai-relay · {remoteNavigation.find(item => item.id === page)?.label ?? "Overview"}</h1><p>Your hosted accounts and usage on ai.fabrials.com.</p></div>
    <div className="fb-row"><label>Period<select value={days} onChange={event => setDays(Number(event.target.value))}><option value={1}>Today</option><option value={7}>7 days</option><option value={30}>30 days</option></select></label><button className="fb-button" disabled={busy} onClick={() => void load()}>{busy ? "Refreshing…" : "Refresh hosted workspace"}</button></div>
    {error && <p role="alert" className="fb-error">{error}</p>}
    {!data ? <div className="fb-empty"><p>{busy ? "Reading ai-relay…" : "Connect your Fabrials account in Settings, then refresh."}</p></div> : <>
      <p className="fb-muted">Updated {new Date(data.generated_at_ms).toLocaleString()}{data.usage_truncated ? " · This response contains a limited portion of the available history." : ""}</p>
      {page === "overview" && <><div className="fb-grid">{[{title:"Requests",value:data.summary.window.requests.toLocaleString()},{title:"Tokens",value:(data.summary.window.input_tokens + data.summary.window.output_tokens).toLocaleString()},{title:"Recorded cost",value:money(data.summary.window.usd)}].map(metric => <article className="fb-card" key={metric.title}><header><h2>{metric.title}</h2></header><div className="fb-card-body"><strong>{metric.value}</strong></div></article>)}</div><div className="fb-grid">{data.accounts.map(account => <AccountCard key={account.id} account={account} sources={(data.synchronized_accounts ?? []).filter(source=>source.linked_account_id===account.id)} />)}</div><SynchronizedAccounts accounts={data.synchronized_accounts ?? []}/></>}
      {page === "accounts" && <><div className="fb-grid"><RemoteDeviceLogin provider="grok" onConnected={load}/><RemoteDeviceLogin provider="nous" onConnected={load}/><RemoteDeviceLogin provider="codex" onConnected={load}/></div><ApiKeyForm accounts={data.accounts} onSave={async ({provider, alias, key}) => mutate("addAccount", {provider,alias,api_key:key,active:false,plan:null})} /><div className="fb-grid">{data.accounts.map(account => <AccountCard key={account.id} account={account} sources={(data.synchronized_accounts ?? []).filter(source=>source.linked_account_id===account.id)}><AccountControls account={account} mutate={mutate}/></AccountCard>)}</div><SynchronizedAccounts accounts={data.synchronized_accounts ?? []}/></>}
      {page === "routing" && <><RoutingExplanation/><div className="fb-grid">{data.steering.map(pool => <section className="fb-card" key={pool.provider}><header><h2>{pool.provider}</h2><p className="fb-muted">{pool.mode} · skip at {pool.exhausted_pct}%</p></header><div className="fb-card-body"><Action label={pool.enabled ? "Disable autosteer" : "Enable autosteer"} run={() => mutate("setAutosteer", {provider:pool.provider,on:!pool.enabled})}/>{pool.queue.map(account => <div className="fb-row" key={account.id}><span>{account.alias}{account.active ? " · active" : ""}</span><span>{account.used_pct ?? "—"}% · {account.exhausted ? "Exhausted" : "Eligible"}</span></div>)}</div></section>)}</div></>}
      {page === "keys" && <>{issuedKey && <section className="fb-card"><header><h2>New proxy key</h2><p>Copy it now. It is shown only once.</p></header><div className="fb-card-body"><code style={{overflowWrap:"anywhere"}}>{issuedKey}</code><button className="fb-button" onClick={() => setIssuedKey(null)}>Hide key</button></div></section>}<KeyEditor onSaved={load} onIssued={setIssuedKey}/><div className="fb-grid">{data.keys.map(key => <KeyEditor key={key.key_hash} existing={key} onSaved={load} onIssued={setIssuedKey}/>)}</div></>}
      {page === "models" && <><label>Find a model<input value={query} onChange={event => setQuery(event.target.value)}/></label><div className="fb-grid">{data.catalog.filter(model => `${model.provider} ${model.model} ${model.account_alias}`.toLowerCase().includes(query.toLowerCase())).map(model => <article className="fb-card" key={`${model.provider}:${model.account_alias}:${model.model}`}><header><h2>{model.model}</h2><p className="fb-muted">{model.provider} · {model.account_alias}</p></header></article>)}</div></>}
      {page === "requests" && <div className="fb-grid">{data.summary.recent.map((hop,index) => <article className="fb-card" key={`${hop.ts_ms}:${index}`}><header><h2>{hop.model ?? "Unknown model"}</h2><time>{new Date(hop.ts_ms).toLocaleString()}</time></header><div className="fb-card-body"><p>{hop.kind ?? "Request"} · HTTP {hop.status ?? "—"} · {money(hop.usd)}</p><p>{hop.input_tokens} input · {hop.output_tokens} output tokens</p></div></article>)}</div>}
      {page === "migration" && <RemoteMigration key={data.session.owner}/>}
      {page === "history" && <PrivateHistory hosted key={data.session.owner}/>}
      {page === "connections" && <><HostedClientConfiguration key={`clients-${data.session.owner}`} owner={data.session.owner} accounts={data.accounts}/><CodexSessionMove key={data.session.owner} owner={data.session.owner} onChanged={load}/><p>Relay {data.relay.id} · {data.relay.version}</p><div className="fb-grid">{data.relay.endpoints.map(endpoint => <article className="fb-card" key={endpoint.key}><header><h2>{endpoint.label}</h2></header><div className="fb-card-body"><code>{endpoint.http_url}</code><p>{endpoint.available ? "Configured" : "Unavailable"} · {endpoint.reachability}</p>{endpoint.websocket_url && <code>{endpoint.websocket_url}</code>}</div></article>)}{data.connectors.map(connector => <article className="fb-card" key={connector.id}><header><h2>{connector.name}</h2></header><div className="fb-card-body"><p>{connector.machine_kind} · {connector.transport} · {connector.status}</p><code>{connector.endpoint}</code></div></article>)}</div></>}
    </>}
  </RemoteContext.Provider>;
}
function AccountCard({account, sources, children}: {account: AccountView; sources:SynchronizedAccount[]; children?: React.ReactNode}) {
  return <article className="fb-card"><header><h2><ProviderIcon provider={account.provider || account.id.split("/")[0]}/> {account.alias}</h2><p className="fb-muted">{account.provider} · {account.plan ?? "Plan not reported"}{account.active ? " · active" : ""}</p></header><div className="fb-card-body">{account.usage_output && <ProviderCard provider={account.usage_output}/>}<p>{account.used_pct == null ? "Quota not reported" : `${account.used_pct}% used`}</p>{account.resets_at && <p>Resets {new Date(account.resets_at).toLocaleString()}</p>}{account.needs_reauth && <p className="fb-error">Authorization needs renewal.</p>}<LinkedAccountUsage accounts={sources}/><ResetInventory observation={account.reset_inventory}/><BalanceCard observation={account.balance}/>{children}</div></article>;
}
function Action({label, run}: {label: string; run: () => Promise<unknown>}) {
  const [busy,setBusy] = React.useState(false); const [error,setError] = React.useState<string | null>(null);
  return <><button className="fb-button" disabled={busy} onClick={async () => {setBusy(true); setError(null); try {await run();} catch (error) {setError(String(error));} finally {setBusy(false);}}}>{busy ? "Working…" : label}</button>{error && <p className="fb-error" role="alert">{error}</p>}</>;
}
function AccountControls({account, mutate}: {account: AccountView; mutate: (operation:RemoteOperation, body:object) => Promise<void>}) {
  const [deleting,setDeleting] = React.useState(false);
  const target = {provider:account.provider,alias:account.alias};
  return <div className="fb-form">{(account.provider === "grok" || account.provider === "nous" || account.provider === "codex") && <details><summary>Renew provider authorization</summary><RemoteDeviceLogin provider={account.provider} initialAlias={account.alias} reauthorize onConnected={() => mutate("probeAccount",target)}/></details>}<ManualQuota account={account} mutate={mutate}/><Action label="Check provider connection" run={() => mutate("probeAccount",target)}/>{!account.active && <Action label="Make active" run={() => mutate("activateAccount",target)}/>}{deleting ? <><p>Remove {account.alias} from ai-relay? Its hosted credentials will be deleted.</p><Action label="Confirm removal" run={() => mutate("deleteAccount",target)}/><button className="fb-button" onClick={() => setDeleting(false)}>Keep account</button></> : <button className="fb-button" onClick={() => setDeleting(true)}>Remove account</button>}</div>;
}
function KeyEditor({existing,onSaved,onIssued}: {existing?: KeyView; onSaved: () => Promise<void>; onIssued: (value:string|null) => void}) {
  const remote = useRemote();
  const [editing,setEditing] = React.useState(!existing);
  const [policy,setPolicy] = React.useState<KeyPolicyRequest>(existing ? {...existing.policy,key_hash:existing.key_hash} : {name:"",budget_period:"calendar_month",budget_usd:null});
  const [busy,setBusy] = React.useState(false); const [error,setError] = React.useState<string | null>(null);
  const scopes = ["allow_providers","allow_accounts","allow_models","allow_routes","allow_kinds"] as const;
  return <section className="fb-card"><header><h2>{existing?.policy.name || (existing ? "Proxy key" : "Create a proxy key")}</h2>{existing && <p className="fb-muted">{existing.prefix} · {existing.enabled ? "Enabled" : "Revoked"} · {money(existing.spent_usd)}</p>}</header><div className="fb-card-body fb-form">
    {editing && <form className="fb-form" onSubmit={async event => {event.preventDefault();setBusy(true);setError(null);try {const result = await remote<IssuedKey>(existing ? "updateKey" : "createKey", policy);if (result.key) onIssued(result.key);setEditing(false);await onSaved();} catch(error) {setError(String(error));} finally {setBusy(false);}}}>
      <label>Name<input required value={policy.name ?? ""} onChange={event => setPolicy({...policy,name:event.target.value})}/></label>
      <label>Recorded spend limit (USD)<input type="number" min="0" step="any" value={policy.budget_usd ?? ""} onChange={event => setPolicy({...policy,budget_usd:event.target.value === "" ? null : Number(event.target.value)})}/></label>
      <p className="fb-muted">A pre-request soft guard. Concurrent requests may settle above this amount.</p>
      <label>Budget period<select value={policy.budget_period} onChange={event => setPolicy({...policy,budget_period:event.target.value})}><option value="calendar_month">Calendar month</option><option value="rolling_30d">Rolling 30 days</option><option value="lifetime">Lifetime</option></select></label>
      {scopes.map(scope => <label key={scope}>{scope.replace("allow_", "Allowed ").replaceAll("_"," ")}<input placeholder="Comma-separated; empty allows all" value={Array.isArray(policy[scope]) ? policy[scope].join(", ") : policy[scope] ?? ""} onChange={event => setPolicy({...policy,[scope]:event.target.value})}/></label>)}
      <button className="fb-button fb-button-primary" disabled={busy} type="submit">{busy ? "Saving…" : existing ? "Save policy" : "Create key"}</button>
    </form>}
    {error && <p className="fb-error" role="alert">{error}</p>}
    {existing?.enabled && <><button className="fb-button" onClick={() => setEditing(value => !value)}>{editing ? "Close editor" : "Edit policy"}</button><Action label="Rotate key" run={async () => { const result = await remote<IssuedKey>("rotateKey",{key_hash:existing.key_hash});onIssued(result.key);await onSaved();}}/><Action label="Revoke key" run={async () => {await remote("revokeKey",{key_hash:existing.key_hash});onIssued(null);await onSaved();}}/></>}
    {!existing && !editing && <button className="fb-button" onClick={() => {onIssued(null);setEditing(true);}}>Create another key</button>}
  </div></section>;
}

function ManualQuota({account,mutate}: {account:AccountView;mutate:(operation:RemoteOperation,body:object)=>Promise<void>}) {
  const [busy,setBusy] = React.useState(false);
  const [error,setError] = React.useState<string|null>(null);
  const [notice,setNotice] = React.useState<string|null>(null);
  return <details><summary>Manual quota</summary><form className="fb-form" onSubmit={async event=>{
    event.preventDefault(); const form = new FormData(event.currentTarget); setBusy(true);setError(null);setNotice(null);
    try { const plan=String(form.get("plan")??"").trim(); await mutate("setQuota",{provider:account.provider,alias:account.alias,credits_remaining:String(form.get("credits_remaining")??""),credits_grant:String(form.get("credits_grant")??""),resets_at:String(form.get("resets_at")??""),...(plan?{plan}:{})});setNotice("Manual quota saved."); }
    catch(error){setError(String(error));} finally{setBusy(false);}
  }}><p className="fb-muted">Record subscription credit capacity for routing. Exclude purchased credits; blank fields keep saved values.</p>
    <label>Remaining subscription credits (USD)<input name="credits_remaining" type="number" min="0" step="any" disabled={busy}/></label>
    <label>Subscription credit grant (USD)<input name="credits_grant" type="number" min="0" step="any" disabled={busy}/></label>
    <label>Next reset<input name="resets_at" placeholder={account.resets_at??"YYYY-MM-DD or RFC3339"} disabled={busy}/></label>
    <label>Plan identifier<input name="plan" maxLength={40} pattern="[A-Za-z0-9_-]*" disabled={busy}/></label>
    <button className="fb-button" disabled={busy} type="submit">Save manual quota</button>
    {error&&<p role="alert" className="fb-error">{error}</p>}{notice&&<p role="status">{notice}</p>}
  </form><Action label="Clear manual quota" run={()=>mutate("setQuota",{provider:account.provider,alias:account.alias,clear:true})}/></details>;
}
