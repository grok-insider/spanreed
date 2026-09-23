import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Alert, AlertAction, AlertDescription, Button, Card, Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, NativeCheckbox, Switch } from "@fabrials/ui";
import type { SharingConsent } from "@fabrials/ai-ui";
import { PrivateHistory } from "./private-history";
import { useLocalData } from "./local-data";
import { Done, ErrorAlert } from "./feedback";
import { LinkButton } from "./link-button";
import { absoluteTime, providerName } from "./format";
import { routeHref } from "./routes";
import type { PublicationStatus, SyncSettings, SyncStatus } from "./contracts";

function useRunner() {
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  const run = React.useCallback(async (action: () => Promise<string | void>) => {
    setBusy(true); setError(null); setNotice(null);
    try { const message = await action(); if (message) setNotice(message); }
    catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }, []);
  return { busy, error, notice, run };
}

function Publication({ linked }: { linked: boolean }) {
  const [status, setStatus] = React.useState<PublicationStatus | null>(null);
  const { busy, error, notice, run } = useRunner();
  const load = React.useCallback(() => invoke<PublicationStatus>("publication_status").then(setStatus), []);
  React.useEffect(() => { void load().catch(() => setStatus(null)); }, [load]);
  const perform = (command: string, args?: Record<string, unknown>) => run(async () => { const message = await invoke<string>(command, args); await load(); return typeof message === "string" ? message : "Saved."; });
  return <div className="sr-subpanel">
    <dl className="sr-facts">
      <dt>Last published</dt><dd>{status?.lastSharedDay ?? "Never"}</dd>
      <dt>Schedule</dt><dd className="sr-muted-code">{status?.schedule ?? "Unknown"}</dd>
    </dl>
    <div className="fui-actions">
      <Button size="sm" disabled={busy || !linked} onClick={() => void perform("publish_metrics")}>Publish now</Button>
      <Button size="sm" variant="outline" disabled={busy} onClick={() => void perform("set_publication_schedule", { enabled: true })}>Publish daily</Button>
      <Button size="sm" variant="ghost" disabled={busy} onClick={() => void perform("set_publication_schedule", { enabled: false })}>Stop daily publishing</Button>
    </div>
    <ErrorAlert title="Publishing didn't work" error={error} />
    <Done>{notice}</Done>
  </div>;
}

function Synchronization({ linked }: { linked: boolean }) {
  const { outputs, accounts } = useLocalData();
  const [selection, setSelection] = React.useState<string[]>([]);
  const [clients, setClients] = React.useState<{ id: string; name: string }[]>([]);
  const [status, setStatus] = React.useState<SyncStatus | null>(null);
  const [historyOpen, setHistoryOpen] = React.useState(false);
  const { busy, error, notice, run } = useRunner();
  const load = React.useCallback(async () => {
    const [settings, sync] = await Promise.all([invoke<SyncSettings>("sync_settings"), invoke<SyncStatus>("sync_status")]);
    setSelection(settings.sources); setStatus(sync);
  }, []);
  React.useEffect(() => { void load().catch(() => undefined); }, [load]);
  React.useEffect(() => { void invoke<{ clients: { id: string; name: string }[] }>("usage_sources").then((value) => setClients(value.clients)).catch(() => setClients([])); }, []);
  const label = (id: string) => {
    const account = accounts.find((item) => item.id === id);
    if (account) return { name: `${providerName(account.provider)} · ${account.alias}`, kind: "Account limits and requests" };
    const output = outputs.find((item) => item.providerId === id);
    const client = clients.find((item) => item.id === id);
    if (output && client) return { name: output.displayName, kind: `Limits and ${client.name} usage` };
    if (output) return { name: output.displayName, kind: "Plan limits" };
    if (client) return { name: client.name, kind: "Local usage" };
    return { name: id, kind: "Saved source" };
  };
  const choices = [...new Set([...outputs.map((output) => output.providerId), ...accounts.map((account) => account.id), ...clients.map((client) => client.id), ...selection])]
    .map((id) => ({ id, ...label(id) })).sort((a, b) => a.name.localeCompare(b.name));
  const save = () => invoke("save_sync_settings", { settings: { sources: selection } });
  return <div className="sr-subpanel">
    <fieldset className="sr-choices">
      <legend>What to sync</legend>
      {choices.map((choice) => <label key={choice.id}>
        <NativeCheckbox checked={selection.includes(choice.id)} disabled={busy} onChange={(event) => setSelection(event.target.checked ? [...selection, choice.id] : selection.filter((value) => value !== choice.id))} />
        <span><span className="sr-choice-name">{choice.name}</span><span className="fui-description">{choice.kind}</span></span>
      </label>)}
    </fieldset>
    <dl className="sr-facts">
      <dt>Last sync</dt><dd>{status?.lastSuccessMs ? `${absoluteTime(status.lastSuccessMs)} · ${status.uploaded} sent, ${status.downloaded} received` : "Never"}</dd>
    </dl>
    {status?.error && <p className="sr-warning">{status.error}</p>}
    <div className="fui-actions">
      <Button size="sm" disabled={busy || !linked || !selection.length} onClick={() => void run(async () => { await save(); const message = await invoke<string>("sync_now"); await load(); return message; })}>Save and sync now</Button>
      <Button size="sm" variant="outline" disabled={busy} onClick={() => void run(async () => { await save(); await load(); return "Sync selection saved."; })}>Save selection</Button>
      <Button size="sm" variant="ghost" disabled={!linked} onClick={() => setHistoryOpen(true)}>View synced history</Button>
    </div>
    {selection.includes("codex") && <details className="sr-disclosure">
      <summary>Match Codex with its hosted account</summary>
      <p className="fui-description">Sign in to the same Codex account on the hosted relay, then verify the match here. This sends signed OpenAI identity details once, including profile claims. Access and refresh tokens are not sent.</p>
      <Button size="sm" variant="outline" disabled={busy || !linked} onClick={() => void run(async () => invoke<string>("link_codex_source"))}>Verify the match</Button>
    </details>}
    <ErrorAlert title="Sync didn't work" error={error} />
    <Done>{notice}</Done>
    <Dialog open={historyOpen} onOpenChange={setHistoryOpen}>
      <DialogContent className="sr-dialog sr-dialog-wide">
        <DialogHeader><DialogTitle>Synced history</DialogTitle><DialogDescription>Observations downloaded from your Fabrials account, including other devices. They're kept apart from this computer's own totals.</DialogDescription></DialogHeader>
        {historyOpen && <PrivateHistory />}
      </DialogContent>
    </Dialog>
  </div>;
}

export function SharingSettings({ linked }: { linked: boolean | null }) {
  const { consent, setConsent } = useLocalData();
  const { busy, error, notice, run } = useRunner();
  const change = (next: SharingConsent, message: string) => run(async () => { await invoke("set_privacy", { consent: next }); setConsent(next); return message; });
  const ready = consent !== null;
  return <div className="sr-stack">
    {linked === false && <Alert>
      <AlertDescription>Publishing and sync need a Fabrials account. You can turn them on now; nothing is sent until this computer is connected.</AlertDescription>
      <AlertAction><LinkButton href={routeHref({ workspace: "local", page: "settings", tab: "account" })}>Connect Fabrials account</LinkButton></AlertAction>
    </Alert>}
    <ErrorAlert title="Couldn't save your choice" error={error} />
    <Done>{notice}</Done>
    <Card className="sr-setting-card">
      <div className="sr-setting">
        <div className="sr-setting-text">
          <label htmlFor="share-metrics" className="sr-setting-title">Publish plan metrics</label>
          <p className="fui-description">Adds a daily summary of your providers, plans, limits and costs to the public Fabrials usage pool. Credentials and request contents are never included.</p>
        </div>
        <Switch id="share-metrics" checked={consent?.shareMetrics ?? false} disabled={busy || !ready} onCheckedChange={(checked) => void change({ ...consent!, shareMetrics: checked }, checked ? "Plan metrics will be published." : "Plan metrics won't be published.")} />
      </div>
      {consent?.shareMetrics && <Publication linked={linked === true} />}
    </Card>
    <Card className="sr-setting-card">
      <div className="sr-setting">
        <div className="sr-setting-text">
          <label htmlFor="sync-history" className="sr-setting-title">Sync private history</label>
          <p className="fui-description">Copies the sources you pick to your own Fabrials account, so you can see them on your other devices and the hosted relay. Credentials and request bodies stay on this computer.</p>
        </div>
        <Switch id="sync-history" checked={consent?.syncHistory ?? false} disabled={busy || !ready} onCheckedChange={(checked) => void change({ ...consent!, syncHistory: checked }, checked ? "Private history sync is on. Pick what to sync." : "Private history sync is off.")} />
      </div>
      {consent?.syncHistory && <Synchronization linked={linked === true} />}
    </Card>
    <p className="fui-description">These two choices are independent, and both are off until you turn them on.</p>
  </div>;
}
