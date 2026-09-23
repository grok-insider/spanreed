import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, CollectionToolbar, Label, NativeSelect, Skeleton, StatePanel, Table } from "@fabrials/ui";
import { useClock, type HistorySample } from "@fabrials/ai-ui";
import { useLocalData } from "./local-data";
import { ErrorAlert } from "./feedback";
import { absoluteTime, ago, providerName } from "./format";

export function HistoryTab() {
  const { signal } = useLocalData();
  const now = useClock() ?? Date.now();
  const [rows, setRows] = React.useState<HistorySample[] | null>(null);
  const [provider, setProvider] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);
  React.useEffect(() => {
    let active = true;
    setError(null);
    void invoke<HistorySample[]>("history").then((value) => { if (active) setRows(value); }).catch((error) => { if (active) setError(String(error)); });
    return () => { active = false; };
  }, [signal]);
  const providers = [...new Set((rows ?? []).map((row) => row.provider))].sort();
  const visible = (rows ?? []).filter((row) => !provider || row.provider === provider).slice().reverse();
  return <div className="sr-stack">
    <p className="fui-description">Each time Spanreed reads your plan limits it keeps the reading, so you can see how fast a limit fills up. The latest 500 readings are shown.</p>
    <ErrorAlert title="Couldn't read limit history" error={error} />
    {!rows && !error && <Skeleton className="sr-table-skeleton" />}
    {rows && !rows.length && <StatePanel state="empty" title="No readings yet" description="Readings are recorded whenever limits refresh, while this app or the background service is running." />}
    {rows && rows.length > 0 && <>
      <CollectionToolbar label="History filters" filters={<Label>Provider<NativeSelect value={provider} onChange={(event) => setProvider(event.target.value)}>
        <option value="">All providers</option>{providers.map((id) => <option key={id} value={id}>{providerName(id)}</option>)}
      </NativeSelect></Label>} />
      <Table aria-label="Limit readings" regionLabel="Limit readings">
        <thead><tr><th scope="col">Recorded</th><th scope="col">Provider</th><th scope="col">Limit</th><th scope="col">Used</th><th scope="col"><span className="fui-sr-only">Event</span></th></tr></thead>
        <tbody>{visible.map((row, index) => <tr key={`${row.provider}:${row.label}:${row.ts_ms}:${index}`}>
          <td><time dateTime={new Date(row.ts_ms).toISOString()} title={absoluteTime(row.ts_ms)}>{ago(row.ts_ms, now)}</time></td>
          <td>{providerName(row.provider)}</td>
          <td>{row.label}</td>
          <td className="sr-numeric">{row.limit > 0 ? `${Math.round((row.used / row.limit) * 1000) / 10}%` : `${row.used}`}</td>
          <td>{row.event === "reset" && <Badge tone="success">Limit reset</Badge>}</td>
        </tr>)}</tbody>
      </Table>
    </>}
  </div>;
}
