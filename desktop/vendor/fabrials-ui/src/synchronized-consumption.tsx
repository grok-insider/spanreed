import * as React from "react";
import { ConsumptionView, type ConsumptionReport, type ConsumptionTotal } from "./consumption";

import type { LocalUsageSnapshotV2 } from "./contracts";
export type ConsumptionSnapshot = LocalUsageSnapshotV2;
const empty = (key: string): ConsumptionTotal => ({key,tokens:0,unknown_token_records:0,known_usd:0,partial:false,records:0});
function add(target: ConsumptionTotal, value: ConsumptionTotal) {
  target.tokens+=value.tokens;target.unknown_token_records+=value.unknown_token_records;
  target.known_usd+=value.known_usd;target.partial ||= value.partial;
}
function project(snapshots: ConsumptionSnapshot[]): ConsumptionReport {
  const daily = new Map<string, ConsumptionTotal>();
  const clients: ConsumptionTotal[] = [], periods: ConsumptionTotal[] = [];
  const total = empty("total");
  for (const snapshot of snapshots) {
    const client = empty(snapshot.source);client.partial=snapshot.partial;
    for (const day of snapshot.days) {
      const row={...empty(day.date),tokens:day.tokens,known_usd:day.estimated_usd,partial:snapshot.partial};
      const date=daily.get(day.date) || empty(day.date);add(date,row);daily.set(day.date,date);add(client,row);
    }
    if (snapshot.period_totals) {
      const period={...empty(snapshot.source),tokens:snapshot.period_totals.tokens || 0,unknown_token_records:snapshot.period_totals.tokens===null?1:0,known_usd:snapshot.period_totals.known_usd,partial:snapshot.period_totals.partial};
      periods.push(period);add(client,period);
    }
    clients.push(client);add(total,client);
  }
  return {revision:Math.max(0,...snapshots.map(s=>s.revision)),sources:snapshots.map(s=>({source:s.source,client:s.source,revision:s.revision,detail:null,state:s.partial?"partial":"ready",records:s.days.length+(s.period_totals?1:0),last_success_ms:s.observed_at_ms})),total,daily:[...daily.values()].sort((a,b)=>a.key.localeCompare(b.key)),period_totals:periods,clients,models:[],sessions:[],projects:[]};
}
export function SynchronizedConsumption({load}: {load:()=>Promise<{snapshots:ConsumptionSnapshot[]}>}) {
  const [snapshots,setSnapshots]=React.useState<ConsumptionSnapshot[]>([]);
  const [device,setDevice]=React.useState("");
  const [error,setError]=React.useState<string | null>(null);
  const [busy,setBusy]=React.useState(true);
  const [refreshKey,setRefreshKey]=React.useState(0);
  React.useEffect(()=>{
    let active=true,inFlight=false;
    async function refresh() {
      if(inFlight)return;
      inFlight=true;setBusy(true);setError(null);
      try {
        const result=await load();
        if(active){setSnapshots(result.snapshots);setDevice(current=>result.snapshots.some(s=>s.device===current)?current:result.snapshots[0]?.device || "");}
      } catch(error){if(active)setError(String(error));}
      finally{inFlight=false;if(active)setBusy(false);}
    }
    void refresh();const timer=setInterval(()=>{if(!document.hidden)void refresh();},60000);
    return()=>{active=false;clearInterval(timer);};
  },[load,refreshKey]);
  return <>
    <div className="fb-heading"><h1>Local consumption</h1><p>Private usage shared by your Spanreed installations. Devices are shown separately to avoid counting copied logs twice.</p></div>
    <button className="fb-button" disabled={busy} onClick={()=>setRefreshKey(value=>value+1)}>Refresh consumption</button>
    {error && <p role="alert" className="fb-error">{error}</p>}
    {busy && <p role="status">Loading consumption…</p>}
    {!busy && !error && !snapshots.length && <p>No consumption synchronized yet. In Spanreed, enable private synchronization and select the clients you want to share.</p>}
    {snapshots.length>0 && <><label className="fb-consumption-device">Device <select value={device} onChange={event=>setDevice(event.target.value)}>{[...new Set(snapshots.map(s=>s.device))].map(id=><option key={id}>{id}</option>)}</select></label><ConsumptionView synchronized report={project(snapshots.filter(s=>s.device===device))}/></>}
  </>;
}
