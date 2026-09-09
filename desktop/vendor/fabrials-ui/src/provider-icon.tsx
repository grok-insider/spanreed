import * as React from "react";
import { providerIcons } from "./provider-icon-data";

export function providerBrand(provider: string): keyof typeof providerIcons | null {
  const name = provider.split("/")[0].toLowerCase();
  const aliases: Record<string, keyof typeof providerIcons> = {
    codex: "openai", openai: "openai", claude: "anthropic", anthropic: "anthropic",
    cursor: "cursor", grok: "grok", xai: "grok", nous: "nousresearch",
  };
  return aliases[name] ?? null;
}

export function ProviderIcon({ provider, size = 24, className = "" }: { provider: string; size?: number; className?: string }) {
  const brand = providerBrand(provider);
  return <span aria-hidden="true" className={`fb-provider-icon ${className}`} style={{ width: size, height: size, display: "inline-flex", flexShrink: 0 }}>
    {brand ? <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: providerIcons[brand] }} /> :
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><rect x="4" y="4" width="16" height="16" rx="4"/><path d="M8 9h8M8 12h8M8 15h5"/></svg>}
  </span>;
}
