import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { CollectionToolbar, Input, Label, NativeSelect, PageHeader, Skeleton, StatePanel, Tabs, TabsContent, TabsList, TabsTrigger } from "@fabrials/ui";
import { ConsumptionView, type ConsumptionReport, type ConsumptionTotal } from "@fabrials/ai-ui";
import { useLocalData } from "./local-data";
import { ErrorAlert } from "./feedback";
import { formatCompact, formatUsd } from "./format";
import { HistoryTab } from "./history";
import { RequestsTab } from "./requests";
import { UsageSourcesTab, type UsageCatalog } from "./usage-sources";
import { routeHref } from "./routes";

const tabs = [
  { id: "consumption", label: "Consumption" },
  { id: "history", label: "Limit history" },
  { id: "requests", label: "Requests" },
  { id: "sources", label: "Sources" },
] as const;

export function UsagePage({ tab }: { tab: string | null }) {
  const current = tabs.some((item) => item.id === tab) ? tab! : "consumption";
  return <>
    <PageHeader title="Usage" description="What your tools used, read from their local logs, limit readings over time and requests sent through Connect." />
    <Tabs value={current} onValueChange={(value) => { location.hash = routeHref({ workspace: "local", page: "usage", tab: String(value) }); }}>
      <TabsList aria-label="Usage views">
        {tabs.map((item) => <TabsTrigger key={item.id} value={item.id}>{item.label}</TabsTrigger>)}
      </TabsList>
      <TabsContent value="consumption"><ConsumptionTab /></TabsContent>
      <TabsContent value="history"><HistoryTab /></TabsContent>
      <TabsContent value="requests"><RequestsTab /></TabsContent>
      <TabsContent value="sources"><UsageSourcesTab /></TabsContent>
    </Tabs>
  </>;
}

function DailyChart({ days }: { days: ConsumptionTotal[] }) {
  if (days.length < 2) return null;
  const peak = days.reduce((best, day) => (day.tokens > best.tokens ? day : best), days[0]);
  const max = Math.max(1, peak.tokens);
  return <figure className="sr-chart">
    <figcaption className="sr-chart-caption">
      <span>Tokens per day</span>
      <span className="fui-description">Peak {formatCompact(peak.tokens)} on {peak.key}</span>
    </figcaption>
    <div className="sr-bars" role="img" aria-label={`Tokens per day from ${days[0].key} to ${days[days.length - 1].key}. Peak ${formatCompact(peak.tokens)} on ${peak.key}. The breakdown table lists every day.`}>
      {days.map((day) => <span key={day.key} className="sr-bar" style={{ height: `${Math.max(day.tokens ? 2 : 0, (day.tokens / max) * 100)}%` }} title={`${day.key}: ${formatCompact(day.tokens)} tokens · ${formatUsd(day.known_usd)}`} />)}
    </div>
    <div className="sr-chart-axis" aria-hidden><span>{days[0].key}</span><span>{days[days.length - 1].key}</span></div>
  </figure>;
}

function ConsumptionTab() {
  const { signal } = useLocalData();
  const [report, setReport] = React.useState<ConsumptionReport | null>(null);
  const [catalog, setCatalog] = React.useState<UsageCatalog | null>(null);
  const [client, setClient] = React.useState("");
  const [days, setDays] = React.useState(31);
  const [model, setModel] = React.useState("");
  const [modelFilter, setModelFilter] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const generation = React.useRef(0);
  React.useEffect(() => { const timer = setTimeout(() => setModelFilter(model.trim()), 350); return () => clearTimeout(timer); }, [model]);
  const refresh = React.useCallback(async (force: boolean) => {
    const current = ++generation.current;
    setBusy(true); setError(null);
    try {
      const result = await invoke<ConsumptionReport>("usage_report", { force, filter: { client: client || null, model: modelFilter || null, since_ms: Date.now() - days * 86_400_000 } });
      if (current === generation.current) setReport(result);
    } catch (error) { if (current === generation.current) setError(String(error)); }
    finally { if (current === generation.current) setBusy(false); }
  }, [client, days, modelFilter]);
  const handledSignal = React.useRef(signal);
  React.useEffect(() => {
    const force = handledSignal.current !== signal;
    handledSignal.current = signal;
    void refresh(force);
    const timer = setInterval(() => { if (!document.hidden) void refresh(false); }, 60_000);
    return () => { clearInterval(timer); generation.current++; };
  }, [refresh, signal]);
  React.useEffect(() => { void invoke<UsageCatalog>("usage_sources").then(setCatalog).catch((error) => setError(String(error))); }, []);
  const empty = report && report.total.records === 0 && !report.daily.some((day) => day.tokens > 0);
  return <div className="sr-stack">
    <CollectionToolbar label="Consumption filters" filters={<>
      <Label>Tool<NativeSelect value={client} onChange={(event) => setClient(event.target.value)}><option value="">All tools</option>{catalog?.clients.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</NativeSelect></Label>
      <Label>Period<NativeSelect value={days} onChange={(event) => setDays(Number(event.target.value))}><option value={7}>Last 7 days</option><option value={31}>Last 31 days</option><option value={90}>Last 90 days</option><option value={365}>Last 365 days</option></NativeSelect></Label>
      <Label>Model<Input type="search" value={model} onChange={(event) => setModel(event.target.value)} placeholder="All models" /></Label>
    </>} />
    <ErrorAlert title="Couldn't read local usage" error={error} />
    {!report && busy && <Skeleton className="sr-chart-skeleton" />}
    {report && empty && <StatePanel state="empty" title="No usage in this period"
      description="Spanreed reads the local logs of supported tools. Use one of them, pick a longer period, or check which tools were found under Sources." />}
    {report && !empty && <>
      <DailyChart days={report.daily} />
      <ConsumptionView report={report} />
    </>}
  </div>;
}
