import * as React from "react";
import { invoke } from "@tauri-apps/api/core";

import type { HistorySample as Sample } from "@fabrials/ui";

export function HistoryPage() {
  const [rows, setRows] = React.useState<Sample[]>([]);
  const [provider, setProvider] = React.useState("");
  const [pending, setPending] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);
  const load = React.useCallback(async () => {
    setPending(true); setError(null);
    try { setRows(await invoke<Sample[]>("history")); }
    catch (error) { setError(String(error)); }
    finally { setPending(false); }
  }, []);
  React.useEffect(() => { void load(); }, [load]);
  const visible = rows.filter(row => !provider || row.provider === provider).slice().reverse();
  return <>
    <div className="fb-heading"><h1>Usage history</h1><p>Recorded session and weekly limits on this machine. The latest 500 observations are shown.</p></div>
    <div className="fb-form"><div className="fb-row"><label>Provider <select value={provider} onChange={event => setProvider(event.target.value)}><option value="">All providers</option>{[...new Set(rows.map(row => row.provider))].sort().map(id => <option key={id} value={id}>{id}</option>)}</select></label><button className="fb-button" disabled={pending} onClick={() => void load()}>{pending ? "Reading…" : "Refresh history"}</button></div></div>
    {error && <p className="fb-error" role="alert">{error}</p>}
    {!pending && !error && !visible.length && <div className="fb-empty"><h2>No observations yet</h2><p>Refresh your providers or run the background usage service to record history.</p></div>}
    {!!visible.length && <div className="fb-table-scroll" role="region" aria-label="Recorded usage observations" tabIndex={0}><table className="fb-table"><caption>Most recent observations first</caption><thead><tr><th scope="col">Recorded</th><th scope="col">Provider</th><th scope="col">Window</th><th scope="col">Usage</th><th scope="col">Event</th></tr></thead><tbody>{visible.map((row, index) => <tr key={`${row.provider}:${row.label}:${row.ts_ms}:${index}`}><td>{new Date(row.ts_ms).toLocaleString()}</td><td>{row.provider}</td><td>{row.label}</td><td>{row.used.toFixed(1)} / {row.limit.toFixed(1)}%</td><td>{row.event === "reset" ? "Limit reset" : "Observation"}</td></tr>)}</tbody></table></div>}
  </>;
}
