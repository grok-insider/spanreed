import * as React from "react";
import { ClientConfiguration } from "./client-configuration";
import { invoke } from "@tauri-apps/api/core";

import type { Status } from "./contracts";
import type { AccountSummary as Account } from "./contracts";
export function ConnectionsPage({ accounts }: { accounts: Account[] }) {
  const [status, setStatus] = React.useState<Status | null>(null);
  const [bind, setBind] = React.useState("127.0.0.1:18736");
  const [provider, setProvider] = React.useState("grok");
  const [alias, setAlias] = React.useState("");
  const [pending, setPending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const busy = React.useRef(false);
  const sequence = React.useRef(0);
  const initialized = React.useRef(false);
  React.useEffect(() => {
    async function refresh() {
      if (busy.current) return;
      const request = ++sequence.current;
      try {
        const value = await invoke<Status>("local_proxy_status");
        if (request === sequence.current) {
          setStatus(value);
          if (!initialized.current) { setBind(value.bind); initialized.current = true; }
        }
      }
      catch (error) { if (request === sequence.current) setError(String(error)); }
    }
    void refresh(); const timer = setInterval(() => void refresh(), 2000);
    return () => { clearInterval(timer);sequence.current++; };
  }, []);
  async function change(command: "start_local_proxy" | "stop_local_proxy") {
    busy.current = true;setPending(true);setError(null);sequence.current++;
    try { setStatus(await invoke<Status>(command, command === "start_local_proxy" ? {bind:bind.trim()} : {})); }
    catch (error) { setError(String(error)); }
    finally { busy.current = false;setPending(false); }
  }
  const providerAccounts = accounts.filter(account => account.provider === provider);
  const selectedAlias = providerAccounts.some(account => account.alias === alias) ? alias : "";
  const suffix = `${selectedAlias ? `/acct/${selectedAlias}` : ""}${provider === "grok" ? "" : `/${provider}`}/v1`;
  const endpoint = status ? `http://${status.bind}${suffix}` : "";
  return <>
    <div className="fb-heading"><h1>Client connections</h1><p>Run the local proxy and connect your clients to it. The proxy runs while this application is open.</p></div>
    <section className="fb-card"><header><h2>Proxy started by this app</h2><p role="status" className="fb-muted">{status ? status.state === "running" ? `Listening on ${status.bind}` : status.state === "stopping" ? "Stopping; waiting for active requests…" : "Stopped" : "Reading status…"}</p></header>
      <div className="fb-card-body"><form className="fb-form" onSubmit={event => {event.preventDefault();void change("start_local_proxy");}}>
        <label>Loopback address and port<input required value={bind} disabled={pending || !status || status.state !== "stopped"} onChange={event => setBind(event.target.value)} /></label>
        <div className="fb-row"><button className="fb-button fb-button-primary" type="submit" disabled={pending || !status || status.state !== "stopped"}>Start proxy</button><button className="fb-button" type="button" disabled={pending || status?.state !== "running"} onClick={()=>void change("stop_local_proxy")}>Stop proxy</button></div>
      </form><p className="fb-muted">A separate CLI capture service may already own this port. Manage that service from the CLI or choose another loopback port here.</p>
      {(error || status?.error) && <p className="fb-error" role="alert">{error || status?.error}</p>}</div>
    </section>
    {status?.state === "running" && <section className="fb-card"><header><h2>Configure a client</h2><p className="fb-muted">Choose a provider route and pin an account to configure a client.</p></header><div className="fb-card-body fb-form">
      <label>Provider<select value={provider} onChange={event=>{setProvider(event.target.value);setAlias("");}}><option value="grok">SuperGrok</option><option value="nous">Nous</option><option value="openai">OpenAI API</option></select></label>
      <label>Account routing<select value={selectedAlias} onChange={event=>setAlias(event.target.value)}><option value="">Active account / autosteer policy</option>{providerAccounts.map(account=><option key={account.id} value={account.alias}>{account.alias} (pinned)</option>)}</select></label>
      <label>OpenAI-compatible base URL<input readOnly value={endpoint} onFocus={event=>event.target.select()} /></label>
      <p className="fb-muted">{providerAccounts.length ? "For clients that require an API key field, use spanreed-local. Managed provider credentials are supplied by Spanreed." : "Connect a managed account in Accounts, or supply your provider key in the client."}</p>
      {selectedAlias && <ClientConfiguration key={`${provider}/${selectedAlias}/${status.bind}`} provider={provider} alias={selectedAlias} />}
      {providerAccounts.length > 0 && <label>Read the model catalog<input readOnly value={`curl --fail '${endpoint}/models'`} onFocus={event=>event.target.select()} /></label>}
    </div></section>}
  </>;
}
