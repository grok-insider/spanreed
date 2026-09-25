import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { Input, Label, Skeleton, StatePanel } from "@fabrials/ui";
import { ErrorAlert } from "./feedback";
import { absoluteTime } from "./format";
import type { ModelCatalog } from "./contracts";

export const modelProviders = ["grok", "codex", "nous", "openai"];

export function useModelCatalog(accountId: string | null) {
  const [catalog, setCatalog] = React.useState<ModelCatalog | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  React.useEffect(() => {
    setCatalog(null); setError(null);
    if (!accountId) return;
    let active = true;
    void invoke<ModelCatalog>("models", { accountId }).then((value) => { if (active) setCatalog(value); }).catch((error) => { if (active) setError(String(error)); });
    return () => { active = false; };
  }, [accountId]);
  return { catalog, error };
}

export function ModelCatalogList({ accountId }: { accountId: string }) {
  const { catalog, error } = useModelCatalog(accountId);
  const [query, setQuery] = React.useState("");
  if (error) return <ErrorAlert title="Couldn't read the model list" error={error} />;
  if (!catalog) return <Skeleton className="sr-table-skeleton" aria-label="Reading models" />;
  const models = catalog.models.filter((model) => model.toLowerCase().includes(query.trim().toLowerCase()));
  return <div className="sr-form">
    <p className="fui-description">Reported by the provider {absoluteTime(catalog.observedAtMs)}. A listed model may still be unavailable for some request types.</p>
    <Label>Search models<Input type="search" value={query} onChange={(event) => setQuery(event.target.value)} /></Label>
    {!models.length ? <StatePanel state="empty" headingLevel={3} title={catalog.models.length ? "No matching models" : "The provider returned no models"} />
      : <ul className="sr-model-list" aria-label={`${models.length} model${models.length === 1 ? "" : "s"}`}>{models.map((model) => <li key={model}><code>{model}</code></li>)}</ul>}
  </div>;
}
