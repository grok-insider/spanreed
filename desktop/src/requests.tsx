import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Skeleton, StatePanel, Table } from "@fabrials/ui";
import { useClock, type UsageRecord } from "@fabrials/ai-ui";
import { useLocalData } from "./local-data";
import { ErrorAlert } from "./feedback";
import { LinkButton } from "./link-button";
import { absoluteTime, ago, formatCount } from "./format";
import { routeHref } from "./routes";

function usage(record: UsageRecord) {
  if (record.unit && record.unit !== "tokens") return record.quantity == null ? "Not reported" : `${formatCount(record.quantity)} ${record.unit}`;
  return `${formatCount(record.total_tokens || record.input_tokens + record.output_tokens)} tokens`;
}

function outputTps(record: UsageRecord) {
  if ((record.kind ?? "chat") !== "chat" || (record.status ?? 0) >= 400 || !record.output_tokens || !record.duration_ms) return null;
  return record.output_tokens / (record.duration_ms / 1000);
}

function tpsLabel(record: UsageRecord) {
  const rate = outputTps(record);
  return rate == null ? "Not recorded" : rate.toFixed(1);
}

export function RequestsTab() {
  const { signal, proxy } = useLocalData();
  const now = useClock() ?? Date.now();
  const [rows, setRows] = React.useState<UsageRecord[] | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  React.useEffect(() => {
    let active = true;
    setError(null);
    void invoke<UsageRecord[]>("hops").then((value) => { if (active) setRows(value); }).catch((error) => { if (active) setError(String(error)); });
    return () => { active = false; };
  }, [signal]);
  return <div className="sr-stack">
    <p className="fui-description">Requests your tools sent through the local proxy on this computer, newest first. The latest 200 are kept.</p>
    <ErrorAlert title="Couldn't read requests" error={error} />
    {!rows && !error && <Skeleton className="sr-table-skeleton" />}
    {rows && !rows.length && <StatePanel state="empty" title="No requests yet"
      description={proxy?.state === "running" ? "The proxy is running. Requests appear here once a connected tool sends one." : "Requests are recorded when your tools send them through Connect. Start the proxy and point a tool at it."}
      actions={proxy?.state === "running" ? undefined : <LinkButton href={routeHref({ workspace: "local", page: "connect" })}>Open Connect</LinkButton>} />}
    {rows && rows.length > 0 && <Table aria-label="Requests" regionLabel="Requests">
      <thead><tr><th scope="col">Completed</th><th scope="col">Account</th><th scope="col">Model</th><th scope="col">Type</th><th scope="col">Status</th><th scope="col">Usage</th><th scope="col">Duration</th><th scope="col">tok/s</th></tr></thead>
      <tbody>{rows.map((row, index) => <tr key={row.request_id || `${row.ts_ms}:${index}`}>
        <td><time dateTime={new Date(row.ts_ms).toISOString()} title={absoluteTime(row.ts_ms)}>{ago(row.ts_ms, now)}</time></td>
        <td>{row.account_id || row.provider || "grok"}</td>
        <td>{row.model || "Not reported"}</td>
        <td>{row.kind || "chat"}</td>
        <td>{row.status == null ? "Not recorded" : <Badge tone={row.status >= 400 ? "danger" : "neutral"}>{row.status}</Badge>}</td>
        <td className="sr-numeric">{usage(row)}</td>
        <td className="sr-numeric">{row.duration_ms == null ? "Not recorded" : `${(row.duration_ms / 1000).toFixed(1)} s`}</td>
        <td className="sr-numeric">{tpsLabel(row)}</td>
      </tr>)}</tbody>
    </Table>}
  </div>;
}
