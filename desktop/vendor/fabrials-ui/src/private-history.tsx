"use client";
import * as React from "react";
import { ProviderCard } from "./index";
import type { PrivateRecentPage, PrivateStoredObservation } from "./contracts";

export function PrivateHistoryView({fetchPage, description}: {fetchPage:(before:number|null)=>Promise<PrivateRecentPage>;description:string}) {
  const [pages, setPages] = React.useState<PrivateRecentPage[]>([]);
  const [busy, setBusy] = React.useState(true);
  const [error, setError] = React.useState<string | null>(null);
  const generation = React.useRef(0);
  const load = React.useCallback(async (before: number | null = null) => {
    const current = ++generation.current;
    try {
      const page = await fetchPage(before);
      if (current === generation.current) {setError(null);setPages(previous => before === null ? [page] : [...previous, page]);}
    } catch (error) { if (current === generation.current) setError(String(error)); }
    finally { if (current === generation.current) setBusy(false); }
  }, [fetchPage]);
  const invalidate = React.useCallback(() => { generation.current++; }, []);
  React.useEffect(() => {
    const current=++generation.current;
    fetchPage(null).then(page=>{if(current===generation.current)setPages([page]);})
      .catch(error=>{if(current===generation.current)setError(String(error));})
      .finally(()=>{if(current===generation.current)setBusy(false);});
    return invalidate;
  }, [fetchPage,invalidate]);
  const last = pages.at(-1);
  return <section className="fb-form"><div className="fb-heading"><h2>Private synchronized history</h2><p className="fb-muted">{description} These observations are separate from local request totals.</p></div>
    <button className="fb-button" disabled={busy} onClick={() => {setBusy(true);setError(null);void load();}}>Refresh private history</button>
    {error && <p role="alert" className="fb-error">{error}</p>}
    {!busy && !error && !pages.some(page => page.observations.length) && <p>No synchronized observations yet.</p>}
    <div className="fb-grid">{pages.flatMap(page => page.observations).map(item => <Observation key={item.cursor} item={item}/>)}</div>
    {last?.has_more && <button className="fb-button" disabled={busy} onClick={() => {setBusy(true);setError(null);void load(last.before);}}>Load older observations</button>}
    {busy && <p role="status">Loading private history…</p>}
  </section>;
}
function Observation({item}: {item: PrivateStoredObservation}) {
  const {event, source} = item.observation;
  const at = event.kind === "quota" ? event.at_ms : event.kind === "history" ? event.sample.ts_ms : event.record.ts_ms;
  return <article className="fb-card"><header><h3>{source}</h3><time>{new Date(at).toLocaleString()}</time><p className="fb-muted">Installation {item.device}</p></header><div className="fb-card-body">
    {event.kind === "quota" ? <ProviderCard provider={event.output}/> : event.kind === "history" ? <><p>{event.sample.label}: {event.sample.used} / {event.sample.limit}</p>{event.sample.resets_at && <p>Resets {new Date(event.sample.resets_at).toLocaleString()}</p>}</> : <><p>{event.record.model ?? "Unknown model"}</p><p>{event.record.input_tokens} input · {event.record.output_tokens} output tokens</p></>}
  </div></article>;
}
