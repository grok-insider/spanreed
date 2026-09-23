import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Boxes, ChartColumn, ChevronDown, CircleGauge, KeyRound, Plug, Plus, RefreshCw, Route as RouteIcon } from "lucide-react";
import {
  Badge, Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
  DropdownMenu, DropdownMenuContent, DropdownMenuGroup, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
  Input, Label, Meter, NativeSelect, PageHeader, SectionHeader, Skeleton, Stat, StatGroup, StatePanel, StatusDot, Table, Tabs, TabsContent, TabsList, TabsTrigger,
} from "@fabrials/ui";
import {
  ApiKeyFields, BalanceCard, LinkedAccountUsage, ProviderIcon, ResetInventory, SynchronizedAccounts, SynchronizedConsumption,
  useClock, type ConsumptionSnapshot, type MetricLine, type ProviderOutput, type SynchronizedAccount,
} from "@fabrials/ai-ui";
import { HostedClientConfiguration } from "./hosted-client-configuration";
import { CodexSessionMove } from "./codex-session-move";
import { PrivateHistory } from "./private-history";
import { RemoteMigration } from "./remote-migration";
import { RemoteDeviceLogin } from "./remote-device-login";
import { FabrialsLink } from "./fabrials-link";
import { DesktopShell, settingsItem, type NavGroup } from "./desktop-shell";
import { Done, ErrorAlert } from "./feedback";
import { absoluteTime, ago, formatCount, providerName, relativeTime } from "./format";
import { loadDashboard, boundRemote, RemoteContext, useRemote } from "./remote-api";
import { routeHref, type Route } from "./routes";
import type { ThemePreference } from "./theme";
import type { AccountView, DashboardPayload, IssuedKey, KeyPolicyRequest, KeyView, SteerProviderView } from "./relay-contracts";
import type { LinkView, RemoteOperation } from "./contracts";

const money = (value: number) => new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 4 }).format(value);
type Mutate = (operation: RemoteOperation, body: object) => Promise<void>;

const groups: NavGroup[] = [
  { label: "Monitor", items: [{ page: "overview", label: "Overview", icon: CircleGauge }, { page: "usage", label: "Usage", icon: ChartColumn }] },
  { label: "Set up", items: [
    { page: "accounts", label: "Accounts", icon: Boxes }, { page: "routing", label: "Routing", icon: RouteIcon },
    { page: "keys", label: "Proxy keys", icon: KeyRound }, { page: "connect", label: "Connect", icon: Plug },
  ] },
];
const pageText: Record<string, [string, string]> = {
  overview: ["Overview", "Requests, tokens and recorded cost on your hosted relay."],
  usage: ["Usage", "Consumption synced from your devices, private history and recent requests."],
  accounts: ["Accounts", "Provider accounts held by your hosted relay."],
  routing: ["Routing", "How the hosted relay picks an account for requests that don't name one."],
  keys: ["Proxy keys", "Keys your tools use to reach the hosted relay. Each can be limited to accounts, models and a spending guard."],
  connect: ["Connect", "Point your tools at the hosted relay, or move a local Codex session there."],
  settings: ["Settings", "Your Fabrials connection and account transfers."],
};

export function HostedWorkspace({ route, theme, onThemeChange }: { route: Route; theme: ThemePreference; onThemeChange: (theme: ThemePreference) => void }) {
  const [data, setData] = React.useState<DashboardPayload | null>(null);
  const [linked, setLinked] = React.useState<boolean | null>(null);
  const [days, setDays] = React.useState(7);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [issuedKey, setIssuedKey] = React.useState<string | null>(null);
  const api = React.useMemo(() => boundRemote(data?.session.owner), [data?.session.owner]);
  const sequence = React.useRef(0);
  const loadedOwner = React.useRef<string | undefined>(undefined);
  const load = React.useCallback(async () => {
    const current = ++sequence.current;
    setBusy(true); setError(null);
    try {
      const next = await loadDashboard<DashboardPayload>(days);
      if (current !== sequence.current) return;
      if (loadedOwner.current !== next.session.owner) setIssuedKey(null);
      loadedOwner.current = next.session.owner;
      setData(next);
    } catch (error) { if (current === sequence.current) { setData(null); setIssuedKey(null); setError(String(error)); } }
    finally { if (current === sequence.current) setBusy(false); }
  }, [days]);
  React.useEffect(() => {
    void invoke<LinkView>("fabrials_status").then((view) => setLinked(view.state === "linked")).catch(() => setLinked(null));
  }, []);
  React.useEffect(() => { if (linked !== false) void load(); return () => { sequence.current++; }; }, [load, linked]);
  const onLink = (view: LinkView) => { const next = view.state === "linked"; setLinked(next); if (!next) { setData(null); setIssuedKey(null); } };
  async function mutate(operation: RemoteOperation, body: object) { await api(operation, body); await load(); }
  const [title, description] = pageText[route.page] ?? pageText.overview;
  const status = <StatusDot tone={linked ? "success" : "neutral"} label={linked ? `Signed in${data?.session.username ? ` as @${data.session.username}` : ""}` : "Not connected"} />;
  const actions = linked ? <>
    <Label className="sr-inline-label">Period<NativeSelect value={days} onChange={(event) => setDays(Number(event.target.value))}><option value={1}>Today</option><option value={7}>7 days</option><option value={30}>30 days</option></NativeSelect></Label>
    <Button variant="outline" size="sm" disabled={busy} onClick={() => void load()}><RefreshCw aria-hidden size={14} className={busy ? "fui-spin" : undefined} />{busy ? "Refreshing…" : "Refresh"}</Button>
  </> : undefined;
  const gate = linked === false && route.page !== "settings";
  return <DesktopShell route={route} groups={groups} footer={[settingsItem]} status={status} actions={actions} theme={theme} onThemeChange={onThemeChange}>
    <RemoteContext.Provider key={data?.session.owner} value={api}>
      {gate ? <PageHeader title="Hosted relay" description="Manage the accounts, keys and routing of your relay at ai.fabrials.com." /> : <PageHeader title={title} description={description} />}
      {gate ? <Card className="sr-gate">
        <CardHeader><CardTitle>Connect this computer to Fabrials</CardTitle><CardDescription>The hosted relay runs your accounts at ai.fabrials.com, so your tools work from any machine. Connect this computer with your Fabrials account to manage it here.</CardDescription></CardHeader>
        <CardContent><FabrialsLink onChange={onLink} /></CardContent>
      </Card> : route.page === "settings" ? <HostedSettings tab={route.tab} data={data} onLink={onLink} /> : <>
        <ErrorAlert title="Couldn't reach the hosted relay" error={error} action={<Button variant="outline" size="sm" disabled={busy} onClick={() => void load()}>Try again</Button>} />
        {!data && busy && <div className="sr-grid" aria-busy="true">{[0, 1, 2].map((index) => <Skeleton key={index} className="sr-card-skeleton" />)}</div>}
        {data && <>
          <p className="fui-description">Updated {absoluteTime(data.generated_at_ms)}{data.usage_truncated ? ". Only part of the available history is included." : "."}</p>
          {route.page === "overview" && <HostedOverview data={data} />}
          {route.page === "usage" && <HostedUsage tab={route.tab} data={data} />}
          {route.page === "accounts" && <HostedAccounts data={data} mutate={mutate} reload={load} />}
          {route.page === "routing" && <HostedRouting data={data} mutate={mutate} />}
          {route.page === "keys" && <HostedKeys data={data} issuedKey={issuedKey} onIssued={setIssuedKey} reload={load} />}
          {route.page === "connect" && <HostedConnect data={data} reload={load} />}
        </>}
      </>}
    </RemoteContext.Provider>
  </DesktopShell>;
}

function HostedOverview({ data }: { data: DashboardPayload }) {
  const metrics = [
    { label: "Requests", value: formatCount(data.summary.window.requests) },
    { label: "Tokens", value: formatCount(data.summary.window.input_tokens + data.summary.window.output_tokens) },
    { label: "Recorded cost", value: money(data.summary.window.usd) },
  ];
  return <>
    <StatGroup>{metrics.map((metric) => <Stat key={metric.label} label={metric.label} value={metric.value} />)}</StatGroup>
    {data.accounts.length ? <section className="sr-stack"><SectionHeader title="Accounts" />
      <div className="sr-grid">{data.accounts.map((account) => <AccountCard key={account.id} account={account} sources={(data.synchronized_accounts ?? []).filter((source) => source.linked_account_id === account.id)} />)}</div>
    </section> : <StatePanel state="empty" title="No hosted accounts yet" description="Add a provider account to the hosted relay under Accounts." />}
    <SynchronizedAccounts accounts={data.synchronized_accounts ?? []} />
  </>;
}

function HostedUsage({ tab, data }: { tab: string | null; data: DashboardPayload }) {
  const api = useRemote();
  const loadConsumption = React.useCallback(() => api<{ snapshots: ConsumptionSnapshot[] }>("consumption", {}), [api]);
  const now = useClock() ?? Date.now();
  const current = ["consumption", "history", "requests"].includes(tab ?? "") ? tab! : "consumption";
  return <Tabs value={current} onValueChange={(value) => { location.hash = routeHref({ workspace: "hosted", page: "usage", tab: String(value) }); }}>
    <TabsList aria-label="Usage views"><TabsTrigger value="consumption">Synced consumption</TabsTrigger><TabsTrigger value="history">Private history</TabsTrigger><TabsTrigger value="requests">Requests</TabsTrigger></TabsList>
    <TabsContent value="consumption"><SynchronizedConsumption load={loadConsumption} /></TabsContent>
    <TabsContent value="history"><PrivateHistory hosted key={data.session.owner} /></TabsContent>
    <TabsContent value="requests">{data.summary.recent.length ? <Table aria-label="Recent hosted requests" regionLabel="Recent hosted requests">
      <thead><tr><th scope="col">Completed</th><th scope="col">Model</th><th scope="col">Type</th><th scope="col">Status</th><th scope="col">Input</th><th scope="col">Output</th><th scope="col">Cost</th></tr></thead>
      <tbody>{data.summary.recent.map((hop, index) => <tr key={`${hop.ts_ms}:${index}`}>
        <td><time title={absoluteTime(hop.ts_ms)}>{ago(hop.ts_ms, now)}</time></td><td>{hop.model ?? "Not reported"}</td><td>{hop.kind ?? "chat"}</td>
        <td>{hop.status == null ? "—" : <Badge tone={hop.status >= 400 ? "danger" : "neutral"}>{hop.status}</Badge>}</td>
        <td className="sr-numeric">{formatCount(hop.input_tokens)}</td><td className="sr-numeric">{formatCount(hop.output_tokens)}</td><td className="sr-numeric">{money(hop.usd)}</td>
      </tr>)}</tbody>
    </Table> : <StatePanel state="empty" title="No requests in this period" />}</TabsContent>
  </Tabs>;
}

function meterFormat(line: Extract<MetricLine, { type: "progress" }>): Intl.NumberFormatOptions {
  if (line.format.kind === "dollars") return { style: "currency", currency: "USD", maximumFractionDigits: 2 };
  if (line.format.kind === "percent") return { style: "unit", unit: "percent", maximumFractionDigits: 0 };
  return { maximumFractionDigits: 0 };
}

function HostedQuota({ provider, now }: { provider: ProviderOutput; now: number }) {
  const lines = provider.lines.filter((line) => line.type !== "barChart" && !(provider.resetInventory && (line.label.startsWith("Reset") || line.label === "Limit reset credits")));
  return <div className="sr-stack">
    {lines.map((line, index) => {
      if (line.type === "progress") {
        const resets = line.resetsAt ? Date.parse(line.resetsAt) : NaN;
        return <Meter key={index} label={line.label} value={line.used} min={0} max={Math.max(line.limit, 1)} format={meterFormat(line)}
          hint={Number.isFinite(resets) ? <time dateTime={line.resetsAt} title={absoluteTime(resets)}>Resets {relativeTime(resets, now)}</time> : undefined} />;
      }
      if (line.kind === "error") return <p role="status" className="sr-warning" key={index}>{line.type === "text" ? line.value : line.type === "badge" ? line.text : line.label}</p>;
      return <div className="sr-list-row" key={index}><span className="fui-description">{line.label}</span><span>{line.type === "text" ? line.value : line.type === "badge" ? line.text : ""}</span></div>;
    })}
    {!lines.length && <p className="fui-description">No quota reported by this provider.</p>}
  </div>;
}

function AccountCard({ account, sources, children }: { account: AccountView; sources: SynchronizedAccount[]; children?: React.ReactNode }) {
  const now = useClock() ?? Date.now();
  const resetsAt = account.resets_at;
  const resets = resetsAt ? Date.parse(resetsAt) : NaN;
  return <Card className="sr-flush">
    <header className="sr-provider-header">
      <ProviderIcon provider={account.provider || account.id.split("/")[0]} size={20} />
      <h2>{account.alias}</h2>
      <span className="fui-description">{providerName(account.provider)} · {account.plan ?? "Plan not reported"}</span>
      {account.active && <Badge tone="success">Active</Badge>}
      {account.needs_reauth && <Badge tone="danger">Sign in again</Badge>}
    </header>
    <div className="sr-hosted-account">
      {account.usage_output ? <HostedQuota provider={account.usage_output} now={now} /> : <p className="fui-description">{account.used_pct == null ? "Limit not reported." : `${account.used_pct}% used.`}{Number.isFinite(resets) ? <> <time dateTime={resetsAt ?? undefined} title={absoluteTime(resets)}>Resets {relativeTime(resets, now)}</time></> : null}</p>}
      <LinkedAccountUsage accounts={sources} />
      <ResetInventory observation={account.reset_inventory} />
      <BalanceCard observation={account.balance} />
      {children}
    </div>
  </Card>;
}

function Action({ label, run, variant = "outline" }: { label: string; run: () => Promise<unknown>; variant?: "outline" | "ghost" | "destructive" }) {
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  return <>
    <Button size="sm" variant={variant} disabled={busy} onClick={async () => { setBusy(true); setError(null); try { await run(); } catch (error) { setError(String(error)); } finally { setBusy(false); } }}>{busy ? "Working…" : label}</Button>
    {error && <ErrorAlert title={`${label} didn't work`} error={error} />}
  </>;
}

function AccountControls({ account, mutate, onReauthorize }: { account: AccountView; mutate: Mutate; onReauthorize?: () => void }) {
  const [deleting, setDeleting] = React.useState(false);
  const target = { provider: account.provider, alias: account.alias };
  return <div className="sr-form">
    <div className="fui-actions">
      {!account.active && <Action label="Make active" run={() => mutate("activateAccount", target)} />}
      <Action label="Check connection" variant="ghost" run={() => mutate("probeAccount", target)} />
      {onReauthorize && <Button size="sm" variant="ghost" onClick={onReauthorize}>Sign in again</Button>}
      {!deleting && <Button size="sm" variant="ghost" onClick={() => setDeleting(true)}>Remove…</Button>}
    </div>
    {deleting && <div className="sr-review"><p>Remove {account.alias} from the hosted relay? Its hosted credentials are deleted.</p><div className="fui-actions"><Action label="Remove" variant="destructive" run={() => mutate("deleteAccount", target)} /><Button size="sm" variant="outline" onClick={() => setDeleting(false)}>Keep account</Button></div></div>}
    <ManualQuota account={account} mutate={mutate} />
  </div>;
}

type HostedFlow = { kind: "signin"; provider: "grok" | "nous" | "codex"; alias?: string; reauthorize?: boolean } | { kind: "apikey" };

function HostedAccounts({ data, mutate, reload }: { data: DashboardPayload; mutate: Mutate; reload: () => Promise<void> }) {
  const [flow, setFlow] = React.useState<HostedFlow | null>(null);
  const selection = React.useRef<HostedFlow | null>(null);
  const [provider, setProvider] = React.useState("openai");
  const [alias, setAlias] = React.useState("");
  const [key, setKey] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [saved, setSaved] = React.useState<string | null>(null);
  const subscriptions = [{ provider: "grok", name: "SuperGrok", description: "Use your Grok subscription" }, { provider: "nous", name: "Nous Research", description: "Connect with Nous OAuth" }, { provider: "codex", name: "Codex", description: "Connect with your OpenAI account" }] as const;
  return <>
    <div className="fui-actions sr-page-actions">
      <DropdownMenu onOpenChangeComplete={(open) => { if (!open && selection.current) { setFlow(selection.current); selection.current = null; setSaved(null); setError(null); } }}>
        <DropdownMenuTrigger render={<Button />}><Plus aria-hidden size={16} />Add account<ChevronDown aria-hidden size={14} /></DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="sr-add-menu">
          <DropdownMenuGroup><DropdownMenuLabel>Subscription accounts</DropdownMenuLabel>
            {subscriptions.map((option) => <DropdownMenuItem key={option.provider} className="sr-menu-option" onClick={() => { selection.current = { kind: "signin", provider: option.provider }; }}>
              <ProviderIcon provider={option.provider} size={20} /><span><span className="sr-menu-option-title">{option.name}</span><span className="sr-menu-option-detail">{option.description}</span></span>
            </DropdownMenuItem>)}
          </DropdownMenuGroup>
          <DropdownMenuSeparator />
          <DropdownMenuGroup><DropdownMenuLabel>API access</DropdownMenuLabel>
            <DropdownMenuItem className="sr-menu-option" onClick={() => { selection.current = { kind: "apikey" }; }}><KeyRound aria-hidden size={20} /><span><span className="sr-menu-option-title">API key</span><span className="sr-menu-option-detail">Connect with a provider API key</span></span></DropdownMenuItem>
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
    {data.accounts.length ? <div className="sr-grid">{data.accounts.map((account) => <AccountCard key={account.id} account={account} sources={(data.synchronized_accounts ?? []).filter((source) => source.linked_account_id === account.id)}>
      <AccountControls account={account} mutate={mutate} onReauthorize={account.provider === "grok" || account.provider === "nous" || account.provider === "codex" ? () => setFlow({ kind: "signin", provider: account.provider as "grok" | "nous" | "codex", alias: account.alias, reauthorize: true }) : undefined} />
    </AccountCard>)}</div> : <StatePanel state="empty" title="No hosted accounts yet" description="Use Add account to sign in to a subscription or add an API key on the hosted relay." />}
    <SynchronizedAccounts accounts={data.synchronized_accounts ?? []} />
    <Dialog open={flow !== null} onOpenChange={(open) => { if (!open) setFlow(null); }}>
      <DialogContent className="sr-dialog">
        {flow?.kind === "signin" && <>
          <DialogHeader><DialogTitle>{flow.reauthorize ? `Sign in to ${flow.alias} again` : `Sign in to ${providerName(flow.provider)}`}</DialogTitle><DialogDescription>Approve the sign-in in your browser.</DialogDescription></DialogHeader>
          <RemoteDeviceLogin provider={flow.provider} initialAlias={flow.alias} reauthorize={flow.reauthorize} onConnected={flow.reauthorize ? () => mutate("probeAccount", { provider: flow.provider, alias: flow.alias }) : reload} />
        </>}
        {flow?.kind === "apikey" && <>
          <DialogHeader><DialogTitle>Add an API key</DialogTitle><DialogDescription>The key is stored by the hosted relay. Requests are billed by the provider's API.</DialogDescription></DialogHeader>
          {saved ? <Done>{saved}</Done> : <form className="sr-form" onSubmit={async (event) => {
            event.preventDefault(); setBusy(true); setError(null);
            try { await mutate("addAccount", { provider, alias: alias.trim(), api_key: key.trim(), active: false, plan: null }); setKey(""); setSaved(`Saved ${provider}/${alias.trim()}. It hasn't been checked with the provider yet.`); }
            catch (error) { setError(String(error)); } finally { setBusy(false); }
          }}>
            <ApiKeyFields provider={provider} alias={alias} secret={key} pending={busy} onProviderChange={setProvider} onAliasChange={setAlias} onSecretChange={setKey} />
            <ErrorAlert title="Couldn't save the key" error={error} />
            <div className="fui-actions"><Button type="submit" disabled={busy}>{busy ? "Saving…" : "Save key"}</Button></div>
          </form>}
        </>}
        <DialogFooter><Button variant="outline" onClick={() => setFlow(null)}>Close</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  </>;
}

function HostedRoutingCard({ pool, mutate }: { pool: SteerProviderView; mutate: Mutate }) {
  const name = providerName(pool.provider);
  return <Card className="sr-flush">
    <header className="sr-provider-header">
      <ProviderIcon provider={pool.provider} size={20} /><h2>{name}</h2>
      <Badge tone={pool.enabled ? "success" : "neutral"}>{pool.enabled ? "Routing on" : "Routing off"}</Badge>
      <Action label={pool.enabled ? "Turn off" : "Turn on"} run={() => mutate("setAutosteer", { provider: pool.provider, on: !pool.enabled })} />
    </header>
    <div className="sr-routing-body">
      <p className="fui-description">{pool.enabled
        ? `The next request that doesn't name an account goes to the best ${name} account that is below the threshold.`
        : `Requests that don't name an account use the active ${name} account.`}</p>
      <Label>Skip an account at (% of its limit)
        <Input type="number" min="1" max="100" step="any" readOnly value={String(pool.exhausted_pct)} />
      </Label>
    </div>
    <ol className="sr-account-list">{pool.queue.map((account) => <li key={account.id} className="sr-account-row">
      <div className="sr-account-main"><span className="sr-account-alias">{account.alias}</span><span className="fui-description">{account.plan ?? "Plan not reported"}</span></div>
      <span className="sr-numeric">{account.used_pct == null ? "—" : `${account.used_pct}%`}</span>
      <div className="sr-account-badges">{account.active && <Badge tone="success">Active</Badge>}{account.exhausted && <Badge tone="warning">Skipped</Badge>}</div>
    </li>)}</ol>
  </Card>;
}

function HostedRouting({ data, mutate }: { data: DashboardPayload; mutate: Mutate }) {
  if (!data.steering.length) return <StatePanel state="empty" title="Nothing to route yet" description="Add accounts to the hosted relay first." />;
  return <>{data.steering.map((pool) => <HostedRoutingCard key={`${pool.provider}:${pool.exhausted_pct}`} pool={pool} mutate={mutate} />)}</>;
}

function HostedKeys({ data, issuedKey, onIssued, reload }: { data: DashboardPayload; issuedKey: string | null; onIssued: (key: string | null) => void; reload: () => Promise<void> }) {
  return <>
    {issuedKey && <Card className="sr-setting-card">
      <SectionHeader title="Your new proxy key" description="Copy it now. It is shown only once." />
      <code className="sr-code">{issuedKey}</code>
      <div className="fui-actions"><Button variant="outline" size="sm" onClick={() => onIssued(null)}>I've copied it</Button></div>
    </Card>}
    <KeyEditor onSaved={reload} onIssued={onIssued} />
    <div className="sr-grid">{data.keys.map((key) => <KeyEditor key={key.key_hash} existing={key} onSaved={reload} onIssued={onIssued} />)}</div>
  </>;
}

function HostedConnect({ data, reload }: { data: DashboardPayload; reload: () => Promise<void> }) {
  const [query, setQuery] = React.useState("");
  const models = data.catalog.filter((model) => `${model.provider} ${model.model} ${model.account_alias}`.toLowerCase().includes(query.toLowerCase()));
  return <>
    <section className="sr-stack">
      <SectionHeader title="Addresses" description={`Relay ${data.relay.id} · version ${data.relay.version}. "Configured" means advertised, not proven reachable from every network.`} />
      <div className="sr-grid">
        {data.relay.endpoints.map((endpoint) => <Card key={endpoint.key} className="sr-setting-card">
          <SectionHeader title={endpoint.label} actions={<Badge tone={endpoint.available ? "success" : "neutral"}>{endpoint.available ? "Configured" : "Unavailable"}</Badge>} />
          <code className="sr-code">{endpoint.http_url}</code>{endpoint.websocket_url && <code className="sr-code">{endpoint.websocket_url}</code>}
          <p className="fui-description">{endpoint.reachability}</p>
        </Card>)}
        {data.connectors.map((connector) => <Card key={connector.id} className="sr-setting-card">
          <SectionHeader title={connector.name} actions={<Badge>{connector.status}</Badge>} />
          <p className="fui-description">{connector.machine_kind} · {connector.transport}</p>{connector.endpoint && <code className="sr-code">{connector.endpoint}</code>}
        </Card>)}
      </div>
    </section>
    <HostedClientConfiguration key={`clients-${data.session.owner}`} owner={data.session.owner} accounts={data.accounts} />
    <CodexSessionMove key={data.session.owner} owner={data.session.owner} onChanged={reload} />
    <section className="sr-stack">
      <SectionHeader title="Models" description="Models the hosted accounts report. Use these IDs in your tools." />
      <Label>Find a model<Input type="search" value={query} onChange={(event) => setQuery(event.target.value)} /></Label>
      {models.length ? <Table aria-label="Hosted models" regionLabel="Hosted models">
        <thead><tr><th scope="col">Model</th><th scope="col">Provider</th><th scope="col">Account</th></tr></thead>
        <tbody>{models.map((model) => <tr key={`${model.provider}:${model.account_alias}:${model.model}`}><td><code>{model.model}</code></td><td>{providerName(model.provider)}</td><td>{model.account_alias}</td></tr>)}</tbody>
      </Table> : <StatePanel state="empty" headingLevel={3} title={data.catalog.length ? "No matching models" : "No models reported yet"} />}
    </section>
  </>;
}

function HostedSettings({ tab, data, onLink }: { tab: string | null; data: DashboardPayload | null; onLink: (view: LinkView) => void }) {
  const current = tab === "transfer" ? "transfer" : "account";
  return <Tabs value={current} onValueChange={(value) => { location.hash = routeHref({ workspace: "hosted", page: "settings", tab: String(value) }); }}>
    <TabsList aria-label="Settings sections"><TabsTrigger value="account">Fabrials account</TabsTrigger><TabsTrigger value="transfer">Transfer accounts</TabsTrigger></TabsList>
    <TabsContent value="account"><Card className="sr-setting-card"><SectionHeader title="Fabrials account" description="The hosted relay uses this connection." /><FabrialsLink onChange={onLink} /></Card></TabsContent>
    <TabsContent value="transfer">{data ? <RemoteMigration key={data.session.owner} /> : <StatePanel state="empty" title="Connect Fabrials first" description="Transfers need a connected hosted relay." />}</TabsContent>
  </Tabs>;
}

function KeyEditor({ existing, onSaved, onIssued }: { existing?: KeyView; onSaved: () => Promise<void>; onIssued: (value: string | null) => void }) {
  const remote = useRemote();
  const [editing, setEditing] = React.useState(!existing);
  const [policy, setPolicy] = React.useState<KeyPolicyRequest>(existing ? { ...existing.policy, key_hash: existing.key_hash } : { name: "", budget_period: "calendar_month", budget_usd: null });
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const scopes = [["allow_providers", "Providers"], ["allow_accounts", "Accounts"], ["allow_models", "Models"], ["allow_routes", "Routes"], ["allow_kinds", "Request types"]] as const;
  return <Card className="sr-setting-card">
    <SectionHeader title={existing?.policy.name || (existing ? "Proxy key" : "Create a proxy key")} description={existing ? `${existing.prefix ?? "Key"} · ${money(existing.spent_usd)} recorded` : "Give it a name. Everything else is optional."} actions={existing && <Badge tone={existing.enabled ? "success" : "neutral"}>{existing.enabled ? "Enabled" : "Revoked"}</Badge>} />
    {editing && <form className="sr-form" onSubmit={async (event) => {
      event.preventDefault(); setBusy(true); setError(null);
      try { const result = await remote<IssuedKey>(existing ? "updateKey" : "createKey", policy); if (result.key) onIssued(result.key); setEditing(false); await onSaved(); }
      catch (error) { setError(String(error)); } finally { setBusy(false); }
    }}>
      <Label>Name<Input required value={policy.name ?? ""} onChange={(event) => setPolicy({ ...policy, name: event.target.value })} /></Label>
      <details className="sr-disclosure">
        <summary>Limit this key</summary>
        <div className="sr-form">
          <div className="sr-field-row">
            <Label>Spending guard (USD)<Input type="number" min="0" step="any" value={policy.budget_usd ?? ""} onChange={(event) => setPolicy({ ...policy, budget_usd: event.target.value === "" ? null : Number(event.target.value) })} /></Label>
            <Label>Period<NativeSelect value={policy.budget_period} onChange={(event) => setPolicy({ ...policy, budget_period: event.target.value })}><option value="calendar_month">Calendar month</option><option value="rolling_30d">Rolling 30 days</option><option value="lifetime">Lifetime</option></NativeSelect></Label>
          </div>
          <p className="fui-description">Checked before each request. Requests already running can finish above the amount.</p>
          {scopes.map(([scope, label]) => <Label key={scope}>Allowed {label.toLowerCase()}<Input placeholder="Comma-separated. Empty allows all." value={Array.isArray(policy[scope]) ? (policy[scope] as string[]).join(", ") : (policy[scope] as string | null | undefined) ?? ""} onChange={(event) => setPolicy({ ...policy, [scope]: event.target.value })} /></Label>)}
        </div>
      </details>
      <ErrorAlert title="Couldn't save the key" error={error} />
      <div className="fui-actions"><Button disabled={busy} type="submit">{busy ? "Saving…" : existing ? "Save changes" : "Create key"}</Button></div>
    </form>}
    {existing?.enabled && <div className="fui-actions">
      <Button size="sm" variant="outline" onClick={() => setEditing((value) => !value)}>{editing ? "Close editor" : "Edit"}</Button>
      <Action label="Rotate" run={async () => { const result = await remote<IssuedKey>("rotateKey", { key_hash: existing.key_hash }); onIssued(result.key); await onSaved(); }} />
      <Action label="Revoke" variant="destructive" run={async () => { await remote("revokeKey", { key_hash: existing.key_hash }); onIssued(null); await onSaved(); }} />
    </div>}
    {!existing && !editing && <div className="fui-actions"><Button size="sm" variant="outline" onClick={() => { onIssued(null); setEditing(true); }}>Create another key</Button></div>}
  </Card>;
}

function ManualQuota({ account, mutate }: { account: AccountView; mutate: Mutate }) {
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  return <details className="sr-disclosure"><summary>Set subscription credits by hand</summary><form className="sr-form" onSubmit={async (event) => {
    event.preventDefault(); const form = new FormData(event.currentTarget); setBusy(true); setError(null); setNotice(null);
    try {
      const plan = String(form.get("plan") ?? "").trim();
      await mutate("setQuota", { provider: account.provider, alias: account.alias, credits_remaining: String(form.get("credits_remaining") ?? ""), credits_grant: String(form.get("credits_grant") ?? ""), resets_at: String(form.get("resets_at") ?? ""), ...(plan ? { plan } : {}) });
      setNotice("Saved.");
    } catch (error) { setError(String(error)); } finally { setBusy(false); }
  }}>
    <p className="fui-description">Used by routing when the provider doesn't report credits. Don't include purchased credits. Blank fields keep their saved values.</p>
    <div className="sr-field-row">
      <Label>Remaining credits (USD)<Input name="credits_remaining" type="number" min="0" step="any" disabled={busy} /></Label>
      <Label>Credit grant (USD)<Input name="credits_grant" type="number" min="0" step="any" disabled={busy} /></Label>
    </div>
    <div className="sr-field-row">
      <Label>Next reset<Input name="resets_at" placeholder={account.resets_at ?? "YYYY-MM-DD or RFC 3339"} disabled={busy} /></Label>
      <Label>Plan identifier<Input name="plan" maxLength={40} pattern="[A-Za-z0-9_\-]*" disabled={busy} /></Label>
    </div>
    <ErrorAlert title="Couldn't save credits" error={error} />
    <Done>{notice}</Done>
    <div className="fui-actions"><Button size="sm" disabled={busy} type="submit">Save credits</Button><Action label="Clear" variant="ghost" run={() => mutate("setQuota", { provider: account.provider, alias: account.alias, clear: true })} /></div>
  </form></details>;
}
