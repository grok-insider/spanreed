import * as React from "react";
import { invoke } from "@tauri-apps/api/core";

import type { UsageRecord as Hop } from "@fabrials/ui";
function usage(record: Hop) {
  if (record.unit && record.unit !== "tokens") return record.quantity == null ? "Unknown" : `${record.quantity.toLocaleString()} ${record.unit}`;
  return `${(record.total_tokens || record.input_tokens + record.output_tokens).toLocaleString()} tokens`;
}
export function RequestsPage() {
  const [rows, setRows] = React.useState<Hop[]>([]);
  const [pending, setPending] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);
  const load = React.useCallback(async () => {
    setPending(true); setError(null);
    try { setRows(await invoke<Hop[]>("hops")); }
    catch (error) { setError(String(error)); }
    finally { setPending(false); }
  }, []);
  React.useEffect(() => { void load(); }, [load]);
  return <>
    <div className="fb-heading"><h1>Recent requests</h1><p>The latest 200 completed requests captured on this machine.</p></div>
    <button className="fb-button" disabled={pending} onClick={() => void load()}>{pending ? "Reading…" : "Refresh requests"}</button>
    {error && <p className="fb-error" role="alert">{error}</p>}
    {!pending && !error && !rows.length && <div className="fb-empty"><h2>No captured requests yet</h2><p>Requests sent through the local Spanreed proxy appear here after completion.</p></div>}
    {!!rows.length && <div className="fb-table-scroll" role="region" aria-label="Completed requests" tabIndex={0}><table className="fb-table"><caption>Provider-reported usage and request metadata</caption><thead><tr><th scope="col">Completed</th><th scope="col">Provider / account</th><th scope="col">Model</th><th scope="col">Type</th><th scope="col">Status</th><th scope="col">Usage</th><th scope="col">Duration</th></tr></thead><tbody>{rows.map((row, index) => <tr key={row.request_id || `${row.ts_ms}:${index}`}><td>{new Date(row.ts_ms).toLocaleString()}</td><td>{row.account_id || row.provider || "grok"}</td><td>{row.model || "Not reported"}</td><td>{row.kind || "chat"}</td><td>{row.status ?? "Not recorded"}</td><td>{usage(row)}</td><td>{row.duration_ms == null ? "Not recorded" : `${(row.duration_ms / 1000).toFixed(2)}s`}</td></tr>)}</tbody></table></div>}
  </>;
}
