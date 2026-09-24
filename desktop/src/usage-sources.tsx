import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Card, CardContent, CardDescription, CardHeader, CardTitle, Input, Label, NativeSelect, Skeleton, Switch, Table, Textarea, type Tone } from "@fabrials/ui";
import { useClock, type ConsumptionReport, type ConsumptionSource } from "@fabrials/ai-ui";
import { useLocalData } from "./local-data";
import { Done, ErrorAlert } from "./feedback";
import { absoluteTime, ago, formatCount } from "./format";

type Settings = { additional_roots: Record<string, string[]>; disabled_clients: string[] };
export type UsageCatalog = { clients: { id: string; name: string; remote_collection: boolean }[]; settings: Settings; connections: string[] };

const states: Record<string, [string, Tone]> = {
  ready: ["Reading", "success"], partial: ["Partly read", "warning"], no_activity: ["No recent activity", "neutral"],
  no_data: ["Not found", "neutral"], needs_connection: ["Needs a connection", "warning"], unsupported_format: ["Unsupported format", "warning"], error: ["Error", "danger"],
};
const reportClients = [
  { id: "cursor", name: "Cursor", credential: "Cursor session cookie (WorkosCursorSessionToken)" },
  { id: "trae", name: "Trae", credential: "Trae access token" },
  { id: "warp", name: "Warp", credential: "Warp access token" },
];

export function UsageSourcesTab() {
  const { signal } = useLocalData();
  const now = useClock() ?? Date.now();
  const [catalog, setCatalog] = React.useState<UsageCatalog | null>(null);
  const [sources, setSources] = React.useState<ConsumptionSource[]>([]);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [folderClient, setFolderClient] = React.useState("codex");
  const [folders, setFolders] = React.useState("");
  const [reportClient, setReportClient] = React.useState("cursor");
  const [account, setAccount] = React.useState("");
  const [credential, setCredential] = React.useState("");
  const load = React.useCallback(async (force = false) => {
    const [nextCatalog, report] = await Promise.all([
      invoke<UsageCatalog>("usage_sources"),
      invoke<ConsumptionReport>("usage_report", { force, filter: { client: null, model: null, since_ms: Date.now() - 31 * 86_400_000 } }),
    ]);
    setCatalog(nextCatalog);
    setSources(report.sources);
  }, []);
  React.useEffect(() => { void load().catch((error) => setError(String(error))); }, [load, signal]);
  React.useEffect(() => { setFolders((catalog?.settings.additional_roots[folderClient] ?? []).join("\n")); }, [catalog, folderClient]);
  async function run(action: () => Promise<string>) {
    setBusy(true); setError(null); setNotice(null);
    try { const message = await action(); await load(true); setNotice(message); }
    catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  async function saveSettings(settings: Settings, message: string) {
    await invoke("save_usage_sources", { settings });
    return message;
  }
  if (!catalog) return error ? <ErrorAlert title="Couldn't read usage sources" error={error} /> : <Skeleton className="sr-table-skeleton" />;
  const status = new Map(sources.map((source) => [source.client, source]));
  const rows = catalog.clients.map((client) => ({ client, source: status.get(client.id) }));
  const found = rows.filter(({ source }) => source && (source.records > 0 || !["no_data", "needs_connection"].includes(source.state)));
  const missing = rows.filter((row) => !found.includes(row));
  const table = (items: typeof rows, label: string) => <Table aria-label={label} regionLabel={label}>
    <thead><tr><th scope="col">Tool</th><th scope="col">Status</th><th scope="col">Records</th><th scope="col">Last read</th><th scope="col">Collect</th></tr></thead>
    <tbody>{items.map(({ client, source }) => {
      const [text, tone] = states[source?.state ?? "no_data"] ?? [source?.state ?? "Unknown", "neutral"];
      const enabled = !catalog.settings.disabled_clients.includes(client.id);
      return <tr key={client.id}>
        <th scope="row">{client.name}</th>
        <td><Badge tone={tone}>{text}</Badge>{source?.detail && <span className="sr-cell-note">{source.detail}</span>}</td>
        <td className="sr-numeric">{source ? formatCount(source.records) : "—"}</td>
        <td>{source?.last_success_ms ? <time title={absoluteTime(source.last_success_ms)}>{ago(source.last_success_ms, now)}</time> : "—"}</td>
        <td><Switch aria-label={`Collect usage from ${client.name}`} checked={enabled} disabled={busy} onCheckedChange={(checked) => void run(() => saveSettings({
          ...catalog.settings,
          disabled_clients: checked ? catalog.settings.disabled_clients.filter((id) => id !== client.id) : [...new Set([...catalog.settings.disabled_clients, client.id])],
        }, `${client.name} ${checked ? "will be collected" : "is no longer collected"}.`))} /></td>
      </tr>;
    })}</tbody>
  </Table>;
  const connected = reportClients.filter((client) => catalog.connections.includes(client.id));
  const selectedReport = reportClients.find((client) => client.id === reportClient) ?? reportClients[0];
  return <div className="sr-stack">
    <ErrorAlert title="Couldn't save" error={error} />
    <Done>{notice}</Done>
    <section className="sr-stack" aria-labelledby="sources-found">
      <h2 id="sources-found" className="sr-section-title">Found on this computer</h2>
      <p className="fui-description">Spanreed reads these tools' own log files. Nothing is sent anywhere unless you turn on sync in Settings.</p>
      {found.length ? table(found, "Tools with usage") : <p className="fui-description">No supported tool has written usage on this computer yet.</p>}
      {missing.length > 0 && <details className="sr-disclosure">
        <summary>Not found ({missing.length})</summary>
        {table(missing, "Tools not found")}
      </details>}
    </section>
    <div className="sr-two-columns">
      <Card>
        <CardHeader><CardTitle>Extra folders</CardTitle><CardDescription>Default locations are found automatically. Add other home folders or archives, one absolute path per line.</CardDescription></CardHeader>
        <CardContent className="sr-form">
          <Label>Tool<NativeSelect value={folderClient} onChange={(event) => setFolderClient(event.target.value)}>{catalog.clients.map((client) => <option key={client.id} value={client.id}>{client.name}</option>)}</NativeSelect></Label>
          <Label>Folders<Textarea rows={4} value={folders} spellCheck={false} onChange={(event) => setFolders(event.target.value)} placeholder="/home/you/archive/.codex" /></Label>
          <div className="fui-actions"><Button disabled={busy} onClick={() => void run(() => saveSettings({
            ...catalog.settings,
            additional_roots: { ...catalog.settings.additional_roots, [folderClient]: folders.split("\n").map((root) => root.trim()).filter(Boolean) },
          }, "Folders saved."))}>Save folders</Button></div>
        </CardContent>
      </Card>
      <Card>
        <CardHeader><CardTitle>Online usage reports</CardTitle><CardDescription>Cursor, Trae and Warp keep usage online. Connect an account to import it. The credential stays on this computer and is never synced.</CardDescription></CardHeader>
        <CardContent className="sr-form">
          {connected.length > 0 && <ul className="sr-list">{connected.map((client) => <li key={client.id}>
            <span>{client.name}</span>
            <Button variant="outline" size="sm" disabled={busy} onClick={() => void run(async () => { await invoke("disconnect_usage_source", { client: client.id }); return `${client.name} disconnected and its imported report cleared.`; })}>Disconnect</Button>
          </li>)}</ul>}
          <form className="sr-form" onSubmit={(event) => {
            event.preventDefault();
            void run(async () => {
              await invoke("connect_usage_source", { connection: { client: reportClient, account, credential } });
              setCredential("");
              return `${selectedReport.name} connected. Its usage appears under Consumption.`;
            });
          }}>
            <Label>Tool<NativeSelect value={reportClient} onChange={(event) => { setReportClient(event.target.value); setCredential(""); }}>{reportClients.map((client) => <option key={client.id} value={client.id}>{client.name}</option>)}</NativeSelect></Label>
            <Label>Account label<Input required value={account} pattern="[A-Za-z0-9_.\-]+" maxLength={128} placeholder="work" onChange={(event) => setAccount(event.target.value)} /></Label>
            <Label>{selectedReport.credential}<Input required type="password" autoComplete="off" value={credential} onChange={(event) => setCredential(event.target.value)} /></Label>
            <div className="fui-actions"><Button type="submit" disabled={busy}>Connect and import</Button></div>
          </form>
        </CardContent>
      </Card>
    </div>
  </div>;
}
