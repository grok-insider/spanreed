import * as React from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "@tauri-apps/api/core";
import { invoke } from "@tauri-apps/api/core";
import { ApiKeyForm, ProviderCard, ProviderIcon, WorkspaceShell, type ProviderOutput, type SharingConsent } from "@fabrials/ui";
import { AccountActions } from "./account-actions";
import { MigrationPage } from "./migration";
import { ConnectionsPage } from "./connections";
import { NotificationSettings } from "./notifications";
import { ModelsPage } from "./models";
import { RequestsPage } from "./requests";
import { HistoryPage } from "./history";
import { RoutingPage } from "./routing";
import { RemoteWorkspace, remoteNavigation } from "./remote-workspace";
import { SharingControls } from "./sharing-controls";
import { FabrialsLink } from "./fabrials-link";
import { DeviceLogin } from "./device-login";
import "@fabrials/ui/tokens.css";
import "@fabrials/ui/styles.css";
import "./desktop.css";

type Consent = SharingConsent;
import type { AccountSummary as Account, AccountsView } from "./contracts";
import type { Detection } from "./contracts";
const navigation = [{id:"overview",label:"Overview"},{id:"accounts",label:"Accounts"},{id:"routing",label:"Autosteer"},{id:"history",label:"History"},{id:"requests",label:"Requests"},{id:"connections",label:"Connections"},{id:"migration",label:"Migration"},{id:"models",label:"Models"},{id:"providers",label:"Providers"},{id:"settings",label:"Settings"}];

function App() {
  const [page, setPage] = React.useState("overview");
  function navigate(next: string) { setPage(next); window.scrollTo(0, 0); }
  const [outputs, setOutputs] = React.useState<ProviderOutput[]>([]);
  const [reauthorize, setReauthorize] = React.useState<Account | null>(null);
  const [accounts, setAccounts] = React.useState<Account[]>([]);
  const [providers, setProviders] = React.useState<Detection[]>([]);
  const [consent, setConsent] = React.useState<Consent>({shareMetrics:false,syncHistory:false});
  const [savedConsent, setSavedConsent] = React.useState<Consent | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [mode, setMode] = React.useState(() => localStorage.getItem("spanreed.mode") === "remote" ? "remote" : "local");
  function changeMode(next: string) { setMode(next); localStorage.setItem("spanreed.mode",next); setPage("overview"); setError(null); setNotice(null); }
  const [theme, setTheme] = React.useState(() => localStorage.getItem("spanreed.theme") || "system");
  React.useEffect(() => {
    const media = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && media.matches);
      document.documentElement.classList.toggle("dark", dark);
      document.documentElement.style.colorScheme = dark ? "dark" : "light";
      if (isTauri()) void getCurrentWindow().setTheme(theme === "system" ? null : dark ? "dark" : "light").catch(error => setError(`Could not update window appearance: ${String(error)}`));
    };
    apply(); media.addEventListener("change",apply); localStorage.setItem("spanreed.theme",theme);
    return () => media.removeEventListener("change",apply);
  },[theme]);
  const load = React.useCallback(async (force = false) => {
    setBusy(true); setError(null);
    try {
      const [usage, registry, detected, preferences] = await Promise.all([
        invoke<ProviderOutput[]>("snapshot",{force}), invoke<AccountsView>("accounts"),
        invoke<Detection[]>("detection"),invoke<Consent>("privacy")
      ]);
      setOutputs(usage);setAccounts(registry.accounts);setProviders(detected);setConsent(preferences);setSavedConsent(preferences);
      await invoke("check_reset_notifications");
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  },[]);
  React.useEffect(() => {
    if (mode !== "local") return;
    void load(); const timer = setInterval(() => { if (!document.hidden) void load(); else void invoke("check_reset_notifications").catch(error => setError(String(error))); },120_000);
    return () => clearInterval(timer);
  },[mode,load]);
  async function openHosted() {
    try { await invoke("open_hosted");setNotice("ai-relay opened in your browser. Use Connect to Fabrials to link this installation."); }
    catch(error) { setError(String(error)); }
  }
  async function saveConsent() {
    setBusy(true);setNotice(null);
    try { await invoke("set_privacy",{consent});setSavedConsent(consent);setNotice("Sharing preferences saved."); }
    catch(error) {setError(String(error));} finally {setBusy(false);}
  }
  const [linkIdentity,setLinkIdentity] = React.useState("");
  const [linkConnected,setLinkConnected] = React.useState(false);
  const workspacePicker = <label className="desktop-workspace-picker">Workspace<select aria-label="Workspace" value={mode} onChange={event => changeMode(event.target.value)}><option value="local">On this machine</option><option value="remote">ai-relay · Fabrials</option></select></label>;
  if (mode === "remote") return <WorkspaceShell title="Spanreed" navigation={remoteNavigation} active={page} onNavigate={navigate} actions={workspacePicker}><RemoteWorkspace page={page}/></WorkspaceShell>;
  return <WorkspaceShell title="Spanreed" navigation={navigation} active={page} onNavigate={navigate} actions={<div className="fb-row">{workspacePicker}<button className="fb-button" disabled={busy} onClick={()=>void load(true)}>{busy?"Refreshing…":"Refresh"}</button></div>}>
    {error && <p role="alert" className="fb-error">{error}</p>}{notice && <p role="status">{notice}</p>}
    {page === "overview" && <><div className="fb-heading"><h1>Your usage, at a glance</h1><p>Live limits and reset credits across your connected providers.</p></div>{!outputs.length?<div className="fb-empty"><h2>{busy?"Reading your providers…":"No usage available yet"}</h2><p className="fb-muted">Sign in through a supported CLI, then refresh. Open Providers to inspect detection.</p></div>:<div className="fb-grid">{outputs.map(output=><ProviderCard key={output.providerId} provider={output}/>)}</div>}</>}
    {page === "accounts" && <><div className="fb-heading"><h1>Accounts</h1><p>Managed accounts on this machine. CLI credentials also appear in Overview.</p></div>{reauthorize && (reauthorize.provider === "grok" || reauthorize.provider === "nous" || reauthorize.provider === "codex") && <section><DeviceLogin key={reauthorize.id} provider={reauthorize.provider} accountId={reauthorize.id} initialAlias={reauthorize.alias} onConnected={()=>{setReauthorize(null);void load();}} /><button className="fb-button" onClick={()=>setReauthorize(null)}>Close authorization</button></section>}<div className="fb-grid"><DeviceLogin provider="grok" onConnected={()=>void load()} /><DeviceLogin provider="nous" onConnected={()=>void load()} /><DeviceLogin provider="codex" onConnected={()=>void load()} /><ApiKeyForm accounts={accounts} onSave={async input => { await invoke("add_api_key", {...input}); await load(); }} /></div><div className="fb-grid">{accounts.map(account=><article className="fb-card" key={account.id}><header><div className="fb-row"><h2>{account.alias}</h2><ProviderIcon provider={account.provider}/></div><p className="fb-muted">{account.provider} · {account.plan_label || "Plan not reported"}</p></header><div className="fb-card-body"><button className="fb-button" disabled={busy || account.active} onClick={async()=>{try{await invoke("activate_account",{id:account.id});await load();}catch(error){setError(String(error));}}}>{account.active?"Active account":"Make active"}</button>{(account.provider === "grok" || account.provider === "nous" || account.provider === "codex") && <button className="fb-button" disabled={busy} onClick={()=>setReauthorize(account)}>Authorize again</button>}<AccountActions key={`${account.id}:${account.generation ?? "legacy"}`} account={account} onChanged={()=>load()} /></div></article>)}</div>{!accounts.length&&<div className="fb-empty"><p>No managed accounts. Existing CLI logins remain available to usage detection.</p></div>}</>}
    {page === "migration" && <MigrationPage />}
    {page === "connections" && <ConnectionsPage accounts={accounts} />}
    {page === "models" && <ModelsPage accounts={accounts} />}
    {page === "requests" && <RequestsPage />}
    {page === "history" && <HistoryPage />}
    {page === "routing" && <RoutingPage />}
    {page === "providers" && <><div className="fb-heading"><h1>Provider detection</h1><p>Detection checks local signals. It does not change your client configuration.</p></div><div className="fb-grid">{providers.map(provider=><article className="fb-card" key={provider.id}><header className="fb-row"><h2>{provider.name}</h2><span className="fb-muted">{provider.detected?"Detected":"Not detected"}</span></header></article>)}</div></>}
    {page === "settings" && <><div className="fb-heading"><h1>Workspace settings</h1><p>Appearance, notifications and how this machine connects to Fabrials.</p></div><div className="desktop-settings fb-form"><section className="fb-card"><header><h2>Appearance</h2><p className="fb-muted">Use your system appearance or choose a theme for Spanreed.</p></header><div className="fb-card-body"><label>Theme<select value={theme} onChange={event=>setTheme(event.target.value)}><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></select></label></div></section>
      <FabrialsLink onChange={view=>{setLinkIdentity(view.state === "linked" ? view.user.id : view.state);setLinkConnected(view.state === "linked");}}/>
      <NotificationSettings />
      <section className="fb-card"><header><h2>Sharing preferences</h2><p className="fb-muted">A Fabrials login is also required for uploads. Enabling one option does not enable the other.</p></header><div className="fb-card-body">
        <label><span><input type="checkbox" checked={consent.shareMetrics} onChange={e=>setConsent({...consent,shareMetrics:e.target.checked})}/> Publish aggregate usage metrics</span><small className="fb-muted">Share provider, plan and usage summaries with the public metrics pool.</small></label>
        <label><span><input type="checkbox" checked={consent.syncHistory} onChange={e=>setConsent({...consent,syncHistory:e.target.checked})}/> Synchronize private usage history</span><small className="fb-muted">Transfer account usage records to your Fabrials account.</small></label>
        <button className="fb-button fb-button-primary" disabled={busy || savedConsent===null} onClick={saveConsent}>Save sharing preferences</button></div></section>
      <SharingControls key={linkIdentity} enabled={savedConsent?.syncHistory ?? false} connected={linkConnected} sources={[...outputs.map(output=>output.providerId),...accounts.map(account=>account.id)]}/>
      <div className="desktop-settings-footer"><button className="fb-button" onClick={openHosted}>Open hosted ai-relay</button></div>
    </div></>}
  </WorkspaceShell>;
}
createRoot(document.getElementById("root")!).render(<App/>);
