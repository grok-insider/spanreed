import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import type { RemoteOperation } from "./contracts";
export function remote<T>(operation: RemoteOperation, body: object = {}, days?: number, owner?:string): Promise<T> {
  return invoke<T>("remote_request", { operation, body, days: days ?? null, owner:owner ?? null });
}
export function boundRemote(owner:string|undefined) {
  return <T,>(operation:RemoteOperation,body:object={},days?:number)=>remote<T>(operation,body,days,owner);
}
export const RemoteContext=React.createContext<ReturnType<typeof boundRemote>>(boundRemote(undefined));
export function useRemote(){return React.useContext(RemoteContext);}
