import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Card, Input, Label, PageHeader, Progress, Skeleton, StatePanel, Switch } from "@fabrials/ui";
import { ProviderIcon } from "@fabrials/ai-ui";
import { useLocalData } from "./local-data";
import { Done, ErrorAlert } from "./feedback";
import { LinkButton } from "./link-button";
import { providerName } from "./format";
import { routeHref } from "./routes";
import type { ProviderPool, RoutingPolicy, RoutingSnapshot } from "./contracts";

function RoutingCard({ policy, pool, pending, onSave }: { policy: RoutingPolicy; pool: ProviderPool; pending: boolean; onSave: (on: boolean, threshold: number) => Promise<void> }) {
  const [limit, setLimit] = React.useState(String(policy.exhausted_pct));
  React.useEffect(() => setLimit(String(policy.exhausted_pct)), [policy.exhausted_pct]);
  const numeric = Number(limit);
  const valid = limit.trim() !== "" && Number.isFinite(numeric) && numeric > 0 && numeric <= 100;
  const name = providerName(policy.provider);
  const switchId = `routing-${policy.provider}`;
  return <Card className="sr-flush" aria-labelledby={`${switchId}-title`}>
    <header className="sr-provider-header">
      <ProviderIcon provider={policy.provider} size={20} />
      <h2 id={`${switchId}-title`}>{name}</h2>
      <label className="sr-switch-label" htmlFor={switchId}>Automatic routing</label>
      <Switch id={switchId} checked={policy.autosteer} disabled={pending} onCheckedChange={(checked) => void onSave(checked, policy.exhausted_pct)} />
    </header>
    <div className="sr-routing-body">
      <p className="fui-description">{policy.autosteer
        ? `The next request that doesn't name an account goes to the best ${name} account that is below the threshold.`
        : `Requests that don't name an account use the active ${name} account.`}</p>
      <form className="sr-inline-form" onSubmit={(event) => { event.preventDefault(); if (valid) void onSave(policy.autosteer, numeric); }}>
        <Label>Skip an account at (% of its limit)
          <Input type="number" min="1" max="100" step="any" required value={limit} disabled={pending} onChange={(event) => setLimit(event.target.value)} />
        </Label>
        <Button type="submit" variant="outline" disabled={pending || !valid || numeric === policy.exhausted_pct}>Save threshold</Button>
      </form>
    </div>
    <ol className="sr-account-list" aria-label={`${name} accounts in routing order`}>
      {pool.accounts.map((account) => <li key={account.alias} className="sr-account-row">
        <div className="sr-account-main">
          <span className="sr-account-alias">{account.alias}</span>
          <span className="fui-description">{account.plan || "Plan not reported"}</span>
        </div>
        <div className="sr-usage-meter">
          {account.used_pct == null ? <span className="fui-description">Limit not reported</span> : <>
            <Progress aria-label={`${account.alias} limit used`} max={100} value={account.used_pct} />
            <span className="sr-numeric">{Math.round(account.used_pct)}%</span>
          </>}
        </div>
        <div className="sr-account-badges">
          {account.role === "next" && <Badge tone="success">Next</Badge>}
          {account.role === "active" && <Badge tone="success">Active</Badge>}
          {policy.autosteer && account.exhausted && <Badge tone="warning">Skipped</Badge>}
        </div>
      </li>)}
    </ol>
  </Card>;
}

export function RoutingPage() {
  const { signal } = useLocalData();
  const [data, setData] = React.useState<RoutingSnapshot | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [pending, setPending] = React.useState(false);
  const load = React.useCallback(async () => {
    try { setData(await invoke<RoutingSnapshot>("routing")); setError(null); }
    catch (error) { setError(String(error)); }
  }, []);
  React.useEffect(() => { void load(); }, [load, signal]);
  async function save(provider: string, on: boolean, threshold: number) {
    setPending(true); setNotice(null); setError(null);
    try {
      await invoke("set_routing", { provider, on, threshold });
      setData(await invoke<RoutingSnapshot>("routing"));
      setNotice(`${providerName(provider)} routing saved. It applies from the next request.`);
    } catch (error) { setError(String(error)); }
    finally { setPending(false); }
  }
  const pools = new Map((data?.limits.providers ?? []).map((pool) => [pool.provider, pool]));
  const active = (data?.policies ?? []).filter((policy) => pools.get(policy.provider)?.accounts.length);
  const idle = (data?.policies ?? []).filter((policy) => !pools.get(policy.provider)?.accounts.length);
  return <>
    <PageHeader title="Routing" description="When you have more than one account for a provider, Spanreed can move requests to another account before a limit runs out." />
    <p className="fui-description sr-lede">Routing applies to requests your tools send through <a href={routeHref({ workspace: "local", page: "connect" })}>Connect</a> without naming an account. A pinned account always gets its requests, and routing never switches models or providers.</p>
    <ErrorAlert title="Couldn't read routing" error={error} action={<Button variant="outline" size="sm" disabled={pending} onClick={() => void load()}>Try again</Button>} />
    <Done>{notice}</Done>
    {!data && !error && <Skeleton className="sr-card-skeleton" />}
    {data && !active.length && <StatePanel state="empty" title="Nothing to route yet"
      description="Routing needs at least one added SuperGrok, Codex, Nous or OpenAI API account. With two or more, Spanreed can switch between them."
      actions={<LinkButton href={routeHref({ workspace: "local", page: "accounts", tab: "add" })}>Add account</LinkButton>} />}
    {active.map((policy) => <RoutingCard key={`${policy.provider}:${policy.autosteer}:${policy.exhausted_pct}`} policy={policy} pool={pools.get(policy.provider)!} pending={pending} onSave={(on, threshold) => save(policy.provider, on, threshold)} />)}
    {active.length > 0 && idle.length > 0 && <p className="fui-description">No accounts yet for {idle.map((policy) => providerName(policy.provider)).join(", ")}.</p>}
  </>;
}
