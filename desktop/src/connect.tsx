import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Input, Label, NativeSelect, PageHeader, Snippet, StatePanel } from "@fabrials/ui";
import { useLocalData } from "./local-data";
import { ClientConfiguration } from "./client-configuration";
import { ErrorAlert } from "./feedback";
import { LinkButton } from "./link-button";
import { providerName } from "./format";
import { routeHref } from "./routes";
import type { Status } from "./contracts";

const routeProviders = ["grok", "codex", "nous", "openai"];

export function ConnectPage() {
  const { accounts, setProxy } = useLocalData();
  const [status, setStatus] = React.useState<Status | null>(null);
  const [bind, setBind] = React.useState("127.0.0.1:18736");
  const [provider, setProvider] = React.useState("grok");
  const [alias, setAlias] = React.useState("");
  const [pending, setPending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const busy = React.useRef(false);
  const sequence = React.useRef(0);
  const initialized = React.useRef(false);
  const publish = React.useRef(setProxy);
  publish.current = setProxy;
  React.useEffect(() => {
    async function refresh() {
      if (busy.current) return;
      const request = ++sequence.current;
      try {
        const value = await invoke<Status>("local_proxy_status");
        if (request !== sequence.current) return;
        setStatus(value); publish.current(value);
        if (!initialized.current) { setBind(value.bind); initialized.current = true; }
      } catch (error) { if (request === sequence.current) setError(String(error)); }
    }
    void refresh();
    const timer = setInterval(() => void refresh(), 2000);
    return () => { clearInterval(timer); sequence.current++; };
  }, []);
  async function change(command: "start_local_proxy" | "stop_local_proxy") {
    busy.current = true; setPending(true); setError(null); sequence.current++;
    try { const value = await invoke<Status>(command, command === "start_local_proxy" ? { bind: bind.trim() } : {}); setStatus(value); publish.current(value); }
    catch (error) { setError(String(error)); }
    finally { busy.current = false; setPending(false); }
  }
  const running = status?.state === "running";
  const providerAccounts = accounts.filter((account) => account.provider === provider);
  const pinned = providerAccounts.find((account) => account.alias === alias);
  const suffix = `${pinned ? `/acct/${pinned.alias}` : ""}${provider === "grok" ? "" : `/${provider}`}/v1`;
  const endpoint = status ? `http://${status.bind}${suffix}` : "";
  const state = !status ? <Badge>Checking…</Badge> : running ? <Badge tone="success">Running</Badge> : status.state === "stopping" ? <Badge tone="warning">Stopping</Badge> : <Badge>Stopped</Badge>;
  return <>
    <PageHeader title="Connect" description="Point your AI tools at Spanreed's local proxy. It records their requests and lets Spanreed route between your accounts. It runs while this app is open." />
    <Card>
      <CardHeader className="sr-card-header-row">
        <div><CardTitle>Local proxy</CardTitle><CardDescription>{running ? `Listening on ${status!.bind}.` : status?.state === "stopping" ? "Waiting for active requests to finish…" : "Start it to get an address for your tools."}</CardDescription></div>
        {state}
      </CardHeader>
      <CardContent>
        <form className="sr-inline-form" onSubmit={(event) => { event.preventDefault(); void change("start_local_proxy"); }}>
          <Label>Address<Input required value={bind} disabled={pending || !status || status.state !== "stopped"} onChange={(event) => setBind(event.target.value)} /></Label>
          {running || status?.state === "stopping"
            ? <Button type="button" variant="outline" disabled={pending || !running} onClick={() => void change("stop_local_proxy")}>Stop proxy</Button>
            : <Button type="submit" disabled={pending || !status}>Start proxy</Button>}
        </form>
        <p className="fui-description">Only this computer can reach it. If the Spanreed background capture service already uses this port, stop that service from the command line or choose another port.</p>
        <ErrorAlert title="The proxy reported a problem" error={error || status?.error} />
      </CardContent>
    </Card>
    {!running ? <StatePanel state="empty" title="Start the proxy to connect a tool" description="You'll get an address to paste into OpenCode, Grok Build or any OpenAI-compatible tool." />
      : <Card>
        <CardHeader><CardTitle>Set up a tool</CardTitle><CardDescription>Choose which account the tool should use, then paste the address into its OpenAI-compatible base URL setting.</CardDescription></CardHeader>
        <CardContent className="sr-form">
          <div className="sr-field-row">
            <Label>Provider<NativeSelect value={provider} onChange={(event) => { setProvider(event.target.value); setAlias(""); }}>
              {routeProviders.map((id) => <option key={id} value={id}>{providerName(id)}</option>)}
            </NativeSelect></Label>
            <Label>Account<NativeSelect value={pinned?.alias ?? ""} onChange={(event) => setAlias(event.target.value)}>
              <option value="">Active account (follows routing)</option>
              {providerAccounts.map((account) => <option key={account.id} value={account.alias}>Always {account.alias}</option>)}
            </NativeSelect></Label>
          </div>
          <Snippet prompt={false} label="Base URL" copyLabel="Copy base URL">{endpoint}</Snippet>
          <p className="fui-description">{providerAccounts.length
            ? "If the tool asks for an API key, enter spanreed-local. Spanreed supplies the real credentials."
            : <>No {providerName(provider)} account is added yet. <a href={routeHref({ workspace: "local", page: "accounts", tab: "add" })}>Add one</a>, or enter your own provider key in the tool.</>}</p>
          {providerAccounts.length > 0 && <Snippet label="Check the model list" copyLabel="Copy command">{`curl --fail '${endpoint}/models'`}</Snippet>}
          {pinned ? <ClientConfiguration key={`${provider}/${pinned.alias}/${status!.bind}`} provider={provider} alias={pinned.alias} accountId={pinned.id} />
            : providerAccounts.length > 0 && <p className="fui-description">To have Spanreed write OpenCode or Grok Build settings, choose a specific account above.</p>}
        </CardContent>
      </Card>}
    {!accounts.length && <p className="fui-description">Tip: routing and pinning need accounts added in Spanreed. <LinkButton variant="link" size="sm" href={routeHref({ workspace: "local", page: "accounts", tab: "add" })}>Add account</LinkButton></p>}
  </>;
}
