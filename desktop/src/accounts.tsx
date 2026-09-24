import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, Ellipsis, KeyRound, Plus } from "lucide-react";
import {
  AlertDialog, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
  Badge, Button, Card, Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
  DropdownMenu, DropdownMenuContent, DropdownMenuGroup, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
  PageHeader, SectionHeader, StatePanel,
} from "@fabrials/ui";
import { ApiKeyFields, ProviderIcon } from "@fabrials/ai-ui";
import { useLocalData } from "./local-data";
import { DeviceLogin, deviceProviderName, type DeviceProvider } from "./device-login";
import { ModelCatalogList, modelProviders } from "./models";
import { Done, ErrorAlert } from "./feedback";
import { providerName } from "./format";
import { routeHref } from "./routes";
import type { AccountSummary, RoutingSnapshot } from "./contracts";

type Flow =
  | { kind: "signin"; provider: DeviceProvider }
  | { kind: "apikey" }
  | { kind: "reauthorize"; account: AccountSummary }
  | { kind: "replace"; account: AccountSummary }
  | { kind: "models"; account: AccountSummary };

const subscriptions: { provider: DeviceProvider; name: string; description: string }[] = [
  { provider: "grok", name: "SuperGrok", description: "Use your Grok subscription" },
  { provider: "nous", name: "Nous Research", description: "Connect with Nous OAuth" },
  { provider: "codex", name: "Codex", description: "Connect with your OpenAI account" },
];
const deviceProviders = new Set<string>(["grok", "nous", "codex"]);
const keyProviders = new Set<string>(["openai", "nous"]);

function AddAccountMenu({ open, onOpenChange, onChoose }: { open: boolean; onOpenChange: (open: boolean) => void; onChoose: (flow: Flow) => void }) {
  const selection = React.useRef<Flow | null>(null);
  return <DropdownMenu open={open} onOpenChange={onOpenChange} onOpenChangeComplete={(next) => {
    if (!next && selection.current) { const flow = selection.current; selection.current = null; onChoose(flow); }
  }}>
    <DropdownMenuTrigger render={<Button />}><Plus aria-hidden size={16} />Add account<ChevronDown aria-hidden size={14} /></DropdownMenuTrigger>
    <DropdownMenuContent align="end" className="sr-add-menu">
      <DropdownMenuGroup>
        <DropdownMenuLabel>Subscription accounts</DropdownMenuLabel>
        {subscriptions.map((option) => <DropdownMenuItem key={option.provider} className="sr-menu-option" onClick={() => { selection.current = { kind: "signin", provider: option.provider }; }}>
          <ProviderIcon provider={option.provider} size={20} />
          <span><span className="sr-menu-option-title">{option.name}</span><span className="sr-menu-option-detail">{option.description}</span></span>
        </DropdownMenuItem>)}
      </DropdownMenuGroup>
      <DropdownMenuSeparator />
      <DropdownMenuGroup>
        <DropdownMenuLabel>API access</DropdownMenuLabel>
        <DropdownMenuItem className="sr-menu-option" onClick={() => { selection.current = { kind: "apikey" }; }}>
          <KeyRound aria-hidden size={20} />
          <span><span className="sr-menu-option-title">API key</span><span className="sr-menu-option-detail">Connect with an OpenAI or Nous API key</span></span>
        </DropdownMenuItem>
      </DropdownMenuGroup>
    </DropdownMenuContent>
  </DropdownMenu>;
}

function ApiKeyFlow({ account, onSaved }: { account?: AccountSummary; onSaved: () => Promise<void> }) {
  const [provider, setProvider] = React.useState(account?.provider ?? "openai");
  const [alias, setAlias] = React.useState(account?.alias ?? "");
  const [key, setKey] = React.useState("");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [saved, setSaved] = React.useState(false);
  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true); setError(null);
    try {
      if (account) await invoke("replace_api_key", { id: account.id, generation: account.generation ?? null, key: key.trim() });
      else await invoke("add_api_key", { provider, alias: alias.trim(), key: key.trim() });
      setKey(""); setSaved(true); await onSaved();
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }
  if (saved) return <Done>{account ? `Key replaced for ${account.alias}.` : `Saved ${provider}/${alias.trim()}.`} Spanreed hasn't checked it with the provider yet.</Done>;
  return <form className="sr-form" onSubmit={(event) => void submit(event)}>
    <ApiKeyFields provider={provider} alias={alias} secret={key} pending={busy} lockedIdentity={Boolean(account)} onProviderChange={setProvider} onAliasChange={setAlias} onSecretChange={setKey} />
    <p className="fui-description">Requests with this account are billed by the provider's API. The key is stored in your system's credential store.</p>
    <ErrorAlert title="Couldn't save the key" error={error} />
    <div className="fui-actions"><Button type="submit" disabled={busy}>{busy ? "Saving…" : account ? "Replace key" : "Save key"}</Button></div>
  </form>;
}

function flowTitle(flow: Flow) {
  switch (flow.kind) {
    case "signin": return { title: `Sign in to ${deviceProviderName(flow.provider)}`, description: "Approve the sign-in in your browser. Spanreed keeps the resulting credentials on this computer." };
    case "apikey": return { title: "Add an API key", description: "Use an existing OpenAI or Nous API key." };
    case "reauthorize": return { title: `Sign in to ${flow.account.alias} again`, description: `Renews the ${providerName(flow.account.provider)} sign-in for this account.` };
    case "replace": return { title: `Replace the key for ${flow.account.alias}`, description: "The old key is overwritten on this computer. It isn't revoked at the provider." };
    case "models": return { title: `Models for ${flow.account.alias}`, description: `The model list ${providerName(flow.account.provider)} reports for this account.` };
  }
}

function AccountRow({ account, role, busy, onFlow, onRemove, onActivate }: {
  account: AccountSummary; role?: { usedPct: number | null; role: string; exhausted: boolean; autosteer: boolean };
  busy: boolean; onFlow: (flow: Flow) => void; onRemove: () => void; onActivate: () => void;
}) {
  const actions = [
    deviceProviders.has(account.provider) && { label: "Sign in again", run: () => onFlow({ kind: "reauthorize", account }) },
    keyProviders.has(account.provider) && { label: "Replace API key", run: () => onFlow({ kind: "replace", account }) },
    modelProviders.includes(account.provider) && { label: "View models", run: () => onFlow({ kind: "models", account }) },
  ].filter((action): action is { label: string; run: () => void } => Boolean(action));
  return <li className="sr-account-row">
    <div className="sr-account-main">
      <span className="sr-account-alias">{account.alias}</span>
      <span className="fui-description">{account.plan_label || "Plan not reported"}{role?.usedPct != null ? ` · ${Math.round(role.usedPct)}% used` : ""}</span>
    </div>
    <div className="sr-account-badges">
      {account.active && <Badge tone="success">Active</Badge>}
      {role?.autosteer && role.role === "next" && <Badge>Next in routing</Badge>}
      {role?.autosteer && role.exhausted && <Badge tone="warning">Skipped by routing</Badge>}
    </div>
    <div className="fui-actions">
      {!account.active && <Button variant="outline" size="sm" disabled={busy} onClick={onActivate}>Make active</Button>}
      <DropdownMenu>
        <DropdownMenuTrigger render={<Button variant="ghost" size="icon-sm" aria-label={`More actions for ${account.alias}`} />}><Ellipsis aria-hidden size={16} /></DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {actions.map((action) => <DropdownMenuItem key={action.label} onClick={action.run}>{action.label}</DropdownMenuItem>)}
          {actions.length > 0 && <DropdownMenuSeparator />}
          <DropdownMenuItem destructive onClick={onRemove}>Remove from Spanreed…</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  </li>;
}

function DetectedTools({ expanded }: { expanded: boolean }) {
  const { detection, outputs } = useLocalData();
  const found = detection.filter((provider) => provider.detected);
  const missing = detection.filter((provider) => !provider.detected);
  const status = (id: string) => {
    const output = outputs.find((item) => item.providerId === id);
    if (!output) return <Badge>Found</Badge>;
    return output.lines.some((line) => line.kind === "error") && !output.lines.some((line) => line.type === "progress")
      ? <Badge tone="danger">Can't read</Badge> : <Badge tone="success">Reading limits</Badge>;
  };
  return <section id="detected" aria-labelledby="detected-title" className="sr-stack">
    <SectionHeader title={<span id="detected-title">Detected on this computer</span>}
      description="Sign-ins that your AI tools already saved. Spanreed reads their limits without changing them; manage them in the tool itself." />
    <Card className="sr-flush">
      {found.length ? <ul className="sr-detected">{found.map((provider) => <li key={provider.id}>
        <ProviderIcon provider={provider.id} size={18} /><span>{provider.name}</span>{status(provider.id)}
      </li>)}</ul> : <p className="sr-card-note">None of the {detection.length} supported tools has a sign-in on this computer yet.</p>}
      {missing.length > 0 && <details className="sr-disclosure sr-card-note" open={expanded || undefined}>
        <summary>Also supported ({missing.length})</summary>
        <p className="fui-description">{missing.map((provider) => provider.name).join(", ")}</p>
      </details>}
    </Card>
  </section>;
}

export function AccountsPage({ tab }: { tab: string | null }) {
  const { accounts, loaded, error, refresh, signal } = useLocalData();
  const [menuOpen, setMenuOpen] = React.useState(tab === "add");
  const [flow, setFlow] = React.useState<Flow | null>(null);
  const [removing, setRemoving] = React.useState<AccountSummary | null>(null);
  const [routing, setRouting] = React.useState<RoutingSnapshot | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [actionError, setActionError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  React.useEffect(() => { if (tab === "add") setMenuOpen(true); if (tab === "detected") document.getElementById("detected")?.scrollIntoView(); }, [tab]);
  React.useEffect(() => { void invoke<RoutingSnapshot>("routing").then(setRouting).catch(() => setRouting(null)); }, [signal, accounts]);
  async function mutate(action: () => Promise<unknown>, message: string) {
    setBusy(true); setActionError(null); setNotice(null);
    try { await action(); await refresh(false); setNotice(message); }
    catch (error) { setActionError(String(error)); }
    finally { setBusy(false); }
  }
  const providers = [...new Set(accounts.map((account) => account.provider))];
  const heading = flow ? flowTitle(flow) : null;
  return <>
    <PageHeader title="Accounts" description="Accounts you added to Spanreed. They can be routed, pinned and connected to other tools." actions={<AddAccountMenu open={menuOpen} onOpenChange={setMenuOpen} onChoose={setFlow} />} />
    <ErrorAlert title="Couldn't read accounts" error={error} />
    <ErrorAlert title="That didn't work" error={actionError} />
    <Done>{notice}</Done>
    {loaded && !accounts.length && <StatePanel state="empty" title="No accounts added yet"
      description="Add a SuperGrok, Codex or Nous sign-in, or an API key. Tools you're already signed in to are detected automatically below." />}
    {providers.map((provider) => {
      const pool = routing?.limits.providers.find((item) => item.provider === provider);
      const policy = routing?.policies.find((item) => item.provider === provider);
      return <Card key={provider} className="sr-flush" aria-labelledby={`provider-${provider}`}>
        <header className="sr-provider-header">
          <ProviderIcon provider={provider} size={20} />
          <h2 id={`provider-${provider}`}>{providerName(provider)}</h2>
          {policy && <a className="sr-provider-routing" href={routeHref({ workspace: "local", page: "routing" })}>
            {policy.autosteer ? `Routing on · skips at ${policy.exhausted_pct}%` : "Routing off"}
          </a>}
        </header>
        <ul className="sr-account-list">{accounts.filter((account) => account.provider === provider).map((account) => {
          const pooled = pool?.accounts.find((item) => item.alias === account.alias);
          return <AccountRow key={`${account.id}:${account.generation ?? "legacy"}`} account={account} busy={busy}
            role={pooled ? { usedPct: pooled.used_pct, role: pooled.role, exhausted: pooled.exhausted, autosteer: pool!.autosteer } : undefined}
            onFlow={setFlow} onRemove={() => setRemoving(account)}
            onActivate={() => void mutate(() => invoke("activate_account", { id: account.id }), `${account.alias} is now the active ${providerName(provider)} account.`)} />;
        })}</ul>
      </Card>;
    })}
    <DetectedTools expanded={tab === "detected"} />
    <Dialog open={flow !== null} onOpenChange={(open) => { if (!open) setFlow(null); }}>
      <DialogContent className="sr-dialog">
        {flow && heading && <>
          <DialogHeader><DialogTitle>{heading.title}</DialogTitle><DialogDescription>{heading.description}</DialogDescription></DialogHeader>
          {flow.kind === "signin" && <DeviceLogin provider={flow.provider} onConnected={() => void refresh(false)} />}
          {flow.kind === "reauthorize" && deviceProviders.has(flow.account.provider) && <DeviceLogin provider={flow.account.provider as DeviceProvider} accountId={flow.account.id} initialAlias={flow.account.alias} onConnected={() => void refresh(false)} />}
          {flow.kind === "apikey" && <ApiKeyFlow onSaved={() => refresh(false)} />}
          {flow.kind === "replace" && <ApiKeyFlow account={flow.account} onSaved={() => refresh(false)} />}
          {flow.kind === "models" && <ModelCatalogList accountId={flow.account.id} />}
          <DialogFooter><Button variant="outline" onClick={() => setFlow(null)}>Close</Button></DialogFooter>
        </>}
      </DialogContent>
    </Dialog>
    <AlertDialog open={removing !== null} onOpenChange={(open) => { if (!open) setRemoving(null); }}>
      <AlertDialogContent>
        {removing && <>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove {removing.alias} from Spanreed?</AlertDialogTitle>
            <AlertDialogDescription>Its saved credentials are deleted from this computer and its usage history stays. Access at {providerName(removing.provider)} isn't revoked. If it's the active account, another {providerName(removing.provider)} account becomes active.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep account</AlertDialogCancel>
            <Button variant="destructive" disabled={busy} onClick={() => { const account = removing; setRemoving(null); void mutate(() => invoke("remove_account", { id: account.id, generation: account.generation ?? null }), `${account.alias} was removed.`); }}>Remove</Button>
          </AlertDialogFooter>
        </>}
      </AlertDialogContent>
    </AlertDialog>
    <p className="fui-description sr-footnote">Moving accounts to or from the hosted relay? Use <a href={routeHref({ workspace: "local", page: "settings", tab: "transfer" })}>Settings → Transfer accounts</a>.</p>
  </>;
}
