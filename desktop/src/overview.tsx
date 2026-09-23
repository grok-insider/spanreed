import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowRight, Check, CircleAlert, TriangleAlert } from "lucide-react";
import { Badge, Button, Card, PageHeader, SectionHeader, Skeleton, StatePanel } from "@fabrials/ui";
import { ProviderCard, ProviderIcon, useClock, type ConsumptionReport, type UsageRecord } from "@fabrials/ai-ui";
import { useLocalData } from "./local-data";
import { attentionItems, formatCompact, formatCount, formatUsd, sortByUtilization } from "./format";
import { ErrorAlert } from "./feedback";
import { LinkButton } from "./link-button";
import { routeHref } from "./routes";

const WEEK = 7 * 24 * 3_600_000;

function GetStarted() {
  const { detection, accounts, proxy } = useLocalData();
  const detected = detection.filter((provider) => provider.detected);
  const steps = [
    {
      done: detected.length > 0,
      title: "Use the AI tools you already have",
      detail: detected.length
        ? `Found sign-ins for ${detected.map((provider) => provider.name).join(", ")}.`
        : `Spanreed reads existing sign-ins from ${detection.length || "many"} supported tools, including Claude Code, Codex, Grok and Copilot. Sign in to one, then refresh.`,
      action: { label: "See supported tools", href: routeHref({ workspace: "local", page: "accounts", tab: "detected" }) },
    },
    {
      done: accounts.length > 0,
      title: "Add an account",
      detail: "Sign in to SuperGrok, Codex or Nous, or add an API key. Added accounts can be routed and connected to other tools.",
      action: { label: "Add account", href: routeHref({ workspace: "local", page: "accounts", tab: "add" }) },
    },
    {
      done: proxy?.state === "running",
      title: "Optional: connect your tools",
      detail: "Start the local proxy to record requests and switch accounts automatically before a limit runs out.",
      action: { label: "Open Connect", href: routeHref({ workspace: "local", page: "connect" }) },
    },
  ];
  return <Card className="sr-get-started" aria-labelledby="get-started-title">
    <h2 id="get-started-title">Get started</h2>
    <p className="fui-description">Spanreed shows how much of your AI subscriptions you have used. Everything stays on this computer unless you turn on sharing.</p>
    <ol className="sr-steps">
      {steps.map((step) => <li key={step.title} data-done={step.done || undefined}>
        <span className="sr-step-marker" aria-hidden>{step.done ? <Check size={14} /> : null}</span>
        <div>
          <p className="sr-step-title">{step.title}{step.done && <span className="fui-sr-only"> (done)</span>}</p>
          <p className="fui-description">{step.detail}</p>
        </div>
        {!step.done && <LinkButton href={step.action.href}>{step.action.label}</LinkButton>}
      </li>)}
    </ol>
  </Card>;
}

function NeedsAttention({ now }: { now: number }) {
  const { outputs } = useLocalData();
  const items = attentionItems(outputs, now);
  if (!items.length) return null;
  return <section aria-labelledby="attention-title">
    <SectionHeader title={<span id="attention-title">Needs attention</span>} />
    <ul className="sr-attention">
      {items.map((item) => <li key={item.key} data-tone={item.tone}>
        {item.tone === "danger" ? <CircleAlert aria-hidden size={18} /> : <TriangleAlert aria-hidden size={18} />}
        <ProviderIcon provider={item.providerId} />
        <div>
          <p className="sr-attention-title">{item.title}</p>
          {item.detail && <p className="fui-description">{item.detail}</p>}
        </div>
      </li>)}
    </ul>
  </section>;
}

function RecentTotals() {
  const { signal, proxy } = useLocalData();
  const [report, setReport] = React.useState<ConsumptionReport | null>(null);
  const [requests, setRequests] = React.useState<number | null>(null);
  React.useEffect(() => {
    let active = true;
    const since = Date.now() - WEEK;
    void invoke<ConsumptionReport>("usage_report", { force: false, filter: { client: null, model: null, since_ms: since } })
      .then((value) => { if (active) setReport(value); }).catch(() => { if (active) setReport(null); });
    void invoke<UsageRecord[]>("hops").then((rows) => { if (active) setRequests(rows.filter((row) => row.ts_ms >= since).length); })
      .catch(() => { if (active) setRequests(null); });
    return () => { active = false; };
  }, [signal]);
  const tiles = [
    { label: "Tokens", value: report ? formatCompact(report.total.tokens) : "—", note: report?.total.unknown_token_records ? "Some tools don't report tokens" : "Recorded by your tools", tab: "consumption" },
    { label: "Known cost", value: report ? formatUsd(report.total.known_usd) : "—", note: report?.total.partial ? "Partial: some prices are unknown" : "Reported costs and list-price estimates", tab: "consumption" },
    { label: "Requests through Connect", value: requests === null ? "—" : formatCount(requests), note: proxy?.state === "running" ? "Proxy is running" : "Proxy is off", tab: "requests" },
  ];
  return <section aria-labelledby="totals-title">
    <SectionHeader title={<span id="totals-title">Last 7 days</span>} actions={<LinkButton variant="ghost" href={routeHref({ workspace: "local", page: "usage" })}>Open Usage <ArrowRight aria-hidden size={14} /></LinkButton>} />
    <div className="sr-stats">
      {tiles.map((tile) => <a key={tile.label} className="sr-stat" href={routeHref({ workspace: "local", page: "usage", tab: tile.tab })}>
        <span className="sr-stat-label">{tile.label}</span>
        <strong className="sr-stat-value">{tile.value}</strong>
        <span className="fui-description">{tile.note}</span>
      </a>)}
    </div>
  </section>;
}

export function OverviewPage() {
  const { outputs, accounts, loaded, loading, error, refresh } = useLocalData();
  const now = useClock() ?? Date.now();
  const limits = sortByUtilization(outputs.filter((output) => !output.lines.length || output.lines.some((line) => line.kind !== "error")));
  const firstRun = loaded && !outputs.length && !accounts.length;
  return <>
    <PageHeader title="Overview" description="Plan limits across your AI subscriptions, closest to running out first." />
    <ErrorAlert title="Couldn't refresh your providers" error={error} action={<Button variant="outline" size="sm" disabled={loading} onClick={() => void refresh()}>Try again</Button>} />
    {firstRun && <GetStarted />}
    <NeedsAttention now={now} />
    {!loaded && <div className="sr-grid" aria-busy="true" aria-label="Loading limits">{[0, 1, 2].map((index) => <Skeleton key={index} className="sr-card-skeleton" />)}</div>}
    {loaded && limits.length > 0 && <section aria-labelledby="limits-title">
      <SectionHeader title={<span id="limits-title">Plan limits</span>} actions={<Badge>{limits.length} provider{limits.length === 1 ? "" : "s"}</Badge>} />
      <div className="sr-grid">{limits.map((output) => <ProviderCard key={output.providerId} provider={output} />)}</div>
    </section>}
    {loaded && !firstRun && !limits.length && !error && <StatePanel state="empty" title="No limits reported yet"
      description="Your accounts are connected, but no provider has reported a limit. Refresh after using one of your tools."
      actions={<Button variant="outline" size="sm" disabled={loading} onClick={() => void refresh()}>Refresh</Button>} />}
    {loaded && !firstRun && <RecentTotals />}
  </>;
}
