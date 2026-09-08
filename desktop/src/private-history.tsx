import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { PrivateHistoryView, type PrivateRecentPage } from "@fabrials/ui";
import { useRemote } from "./remote-api";
export function PrivateHistory({hosted=false}:{hosted?:boolean}) {
  const remote=useRemote();
  const fetchPage=React.useCallback((before:number|null)=>hosted?remote<PrivateRecentPage>("recentSync",{before}):invoke<PrivateRecentPage>("private_history",{before}),[hosted,remote]);
  return <PrivateHistoryView fetchPage={fetchPage} description={hosted?"Observations from your connected installations.":"Downloaded observations for your connected Fabrials account."}/>;
}
