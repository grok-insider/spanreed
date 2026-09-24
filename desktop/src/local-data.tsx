import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ProviderOutput, SharingConsent } from "@fabrials/ai-ui";
import type { AccountSummary, AccountsView, Detection, Status } from "./contracts";

type LocalData = {
  outputs: ProviderOutput[];
  accounts: AccountSummary[];
  detection: Detection[];
  consent: SharingConsent | null;
  proxy: Status | null;
  loaded: boolean;
  loading: boolean;
  error: string | null;
  updatedAt: number | null;
  /** Increments on every user-requested refresh so pages can reload their own data. */
  signal: number;
  refresh: (force?: boolean) => Promise<void>;
  setConsent: (consent: SharingConsent) => void;
  setProxy: (status: Status) => void;
};

const Context = React.createContext<LocalData | null>(null);

export function useLocalData() {
  const value = React.useContext(Context);
  if (!value) throw new Error("useLocalData must be used inside LocalDataProvider");
  return value;
}

export function LocalDataProvider({ children }: { children: React.ReactNode }) {
  const [outputs, setOutputs] = React.useState<ProviderOutput[]>([]);
  const [accounts, setAccounts] = React.useState<AccountSummary[]>([]);
  const [detection, setDetection] = React.useState<Detection[]>([]);
  const [consent, setConsent] = React.useState<SharingConsent | null>(null);
  const [proxy, setProxy] = React.useState<Status | null>(null);
  const [loaded, setLoaded] = React.useState(false);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [updatedAt, setUpdatedAt] = React.useState<number | null>(null);
  const [signal, setSignal] = React.useState(0);
  const sequence = React.useRef(0);

  const load = React.useCallback(async (force: boolean) => {
    const current = ++sequence.current;
    setLoading(true);
    setError(null);
    try {
      const [usage, registry, detected, preferences] = await Promise.all([
        invoke<ProviderOutput[]>("snapshot", { force }),
        invoke<AccountsView>("accounts"),
        invoke<Detection[]>("detection"),
        invoke<SharingConsent>("privacy"),
      ]);
      if (current !== sequence.current) return;
      setOutputs(usage);
      setAccounts(registry.accounts);
      setDetection(detected);
      setConsent(preferences);
      setUpdatedAt(Date.now());
      await invoke("check_reset_notifications");
    } catch (error) {
      if (current === sequence.current) setError(String(error));
    } finally {
      if (current === sequence.current) { setLoading(false); setLoaded(true); }
    }
  }, []);

  const refresh = React.useCallback(async (force = true) => {
    setSignal((value) => value + 1);
    await load(force);
  }, [load]);

  React.useEffect(() => {
    void load(false);
    const timer = setInterval(() => {
      if (!document.hidden) void load(false);
      else void invoke("check_reset_notifications").catch((error) => setError(String(error)));
    }, 120_000);
    return () => { clearInterval(timer); sequence.current++; };
  }, [load]);

  React.useEffect(() => {
    let active = true;
    const read = () => {
      if (document.hidden) return;
      void invoke<Status>("local_proxy_status").then((status) => { if (active) setProxy(status); }).catch(() => undefined);
    };
    read();
    const timer = setInterval(read, 5_000);
    return () => { active = false; clearInterval(timer); };
  }, []);

  const value = React.useMemo<LocalData>(() => ({
    outputs, accounts, detection, consent, proxy, loaded, loading, error, updatedAt, signal, refresh, setConsent, setProxy,
  }), [outputs, accounts, detection, consent, proxy, loaded, loading, error, updatedAt, signal, refresh]);
  return <Context.Provider value={value}>{children}</Context.Provider>;
}
