import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { ExternalLink } from "lucide-react";
import { Button, Input, Label } from "@fabrials/ui";
import { Done, ErrorAlert } from "./feedback";
import type { LoginView as Login, Progress } from "./contracts";

export type DeviceProvider = "nous" | "grok" | "codex";
export const deviceProviderName = (provider: DeviceProvider) => provider === "grok" ? "SuperGrok" : provider === "codex" ? "Codex" : "Nous";
const portalName = (provider: DeviceProvider) => provider === "grok" ? "Grok sign-in" : provider === "codex" ? "OpenAI sign-in" : "Nous Portal";

export function DeviceLogin({ provider, accountId, initialAlias, lockedAlias = false, inactive = false, migration, onConnected }: {
  provider: DeviceProvider;
  accountId?: string;
  initialAlias?: string;
  lockedAlias?: boolean;
  inactive?: boolean;
  migration?: { id: string; sourceId: string };
  onConnected: (alias: string) => void;
}) {
  const name = deviceProviderName(provider);
  const portal = portalName(provider);
  const [alias, setAlias] = React.useState(initialAlias ?? (provider === "nous" ? "portal" : "personal"));
  const [login, setLogin] = React.useState<Login | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [connected, setConnected] = React.useState<string | null>(null);
  const [retry, setRetry] = React.useState(0);
  const onConnectedRef = React.useRef(onConnected);
  onConnectedRef.current = onConnected;
  const mounted = React.useRef(false);
  React.useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  React.useEffect(() => {
    if (!login) return;
    return () => { void invoke("cancel_device_login", { id: login.id }).catch(() => undefined); };
  }, [login]);
  React.useEffect(() => {
    if (!login) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const progress = await invoke<Progress>("poll_device_login", { id: login!.id });
        if (stopped) return;
        if (progress.state === "connected") {
          setLogin(null); setConnected(login!.alias); onConnectedRef.current(login!.alias);
        } else {
          timer = setTimeout(() => void poll(), Math.max(1, progress.retryAfterSecs) * 1000);
        }
      } catch (error) { if (!stopped) setError(String(error)); }
    }
    timer = setTimeout(() => void poll(), login.device.interval * 1000);
    return () => { stopped = true; clearTimeout(timer); };
  }, [login, retry]);
  async function begin(event: React.FormEvent) {
    event.preventDefault(); setBusy(true); setError(null); setConnected(null);
    try {
      const flow = migration ? await invoke<Login>("begin_migration_authorization", migration)
        : accountId ? await invoke<Login>("reauthorize_account", { id: accountId })
        : await invoke<Login>(inactive ? "begin_inactive_device_login" : "begin_device_login", { provider, alias: alias.trim() });
      if (mounted.current) setLogin(flow);
      else await invoke("cancel_device_login", { id: flow.id });
    } catch (error) { if (mounted.current) setError(String(error)); }
    finally { if (mounted.current) setBusy(false); }
  }
  async function cancel() {
    if (!login) return;
    setBusy(true);
    try { await invoke("cancel_device_login", { id: login.id }); setLogin(null); setError(null); }
    catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  if (connected) return <Done>Signed in as {provider}/{connected}.</Done>;
  return <div className="sr-form">
    <ErrorAlert title="Sign-in didn't complete" error={error} action={login ? <Button variant="outline" size="sm" onClick={() => { setError(null); setRetry(retry + 1); }}>Check again</Button> : undefined} />
    {!login ? <form className="sr-form" onSubmit={(event) => void begin(event)}>
      <Label>Account name
        <Input required pattern="[A-Za-z0-9_\-]+" maxLength={40} value={alias} disabled={busy || Boolean(accountId) || lockedAlias} onChange={(event) => setAlias(event.target.value)} aria-describedby="device-alias-hint" />
      </Label>
      <p id="device-alias-hint" className="fui-description">A short name to tell this {name} account apart, such as "personal" or "work". Letters, numbers, - and _.</p>
      <div className="fui-actions"><Button type="submit" disabled={busy}>{busy ? "Starting…" : accountId ? "Sign in again" : `Sign in to ${name}`}</Button></div>
    </form> : <div className="sr-form">
      <p>Open {portal} and enter this code:</p>
      <output className="sr-device-code" aria-label={`${name} sign-in code`}>{login.device.userCode}</output>
      <p className="fui-description">Waiting for you to approve. The code expires in {Math.ceil(login.device.expiresIn / 60)} minutes.</p>
      <div className="fui-actions">
        <Button disabled={busy} onClick={() => void invoke("open_device_login", { id: login.id }).catch((error) => setError(String(error)))}><ExternalLink aria-hidden size={16} />Open {portal}</Button>
        <Button variant="outline" disabled={busy} onClick={() => void cancel()}>Cancel</Button>
      </div>
    </div>}
  </div>;
}
